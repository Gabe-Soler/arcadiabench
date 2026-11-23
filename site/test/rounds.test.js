// Rounds: what a run of the workflows plans to do (site/tools/plan.mjs), the
// checks on the results they are handed (site/tools/publish-results.mjs),
// and the board on a round.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { acceptPlan, lastRound, listEntries, roundPlan } from '../tools/plan.mjs';
import { publishAccept, publishRound, readResults, seedList } from '../tools/publish-results.mjs';
import { currentBoard } from '../tools/build-board.mjs';
import { roundSeeds } from '../tools/round-seeds.mjs';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const json = (path) => JSON.parse(readFileSync(path, 'utf8'));
const FOUNDING = ['arcadiaos', 'baseline-fcfs', 'baseline-greedy', 'baseline-random'];
const COMMIT = '0123456789abcdef0123456789abcdef01234567';
const digest = (ch) => `sha256:${ch.repeat(64)}`;

/** A copy of the repository's leaderboard with the four founding entries and
 * no rounds. `accepted` names the entries that have an image. */
function scratch(accepted = FOUNDING) {
  const root = mkdtempSync(join(tmpdir(), 'arcadia-rounds-'));
  mkdirSync(join(root, 'leaderboard/entries'), { recursive: true });
  cpSync(join(repo, 'leaderboard/config.json'), join(root, 'leaderboard/config.json'));
  for (const [i, id] of FOUNDING.entries()) {
    const dir = join(root, 'leaderboard/entries', id, '1.1.0');
    mkdirSync(dir, { recursive: true });
    cpSync(join(repo, 'leaderboard/entries', id, '1.1.0/public'), join(dir, 'public'), { recursive: true });
    const { image_digest: _, ...entry } = json(join(repo, 'leaderboard/entries', id, '1.1.0/entry.json'));
    if (accepted.includes(id)) entry.image_digest = digest(String(i + 1));
    writeFileSync(join(dir, 'entry.json'), JSON.stringify(entry));
  }
  return root;
}

/** An entry's public results as if they had been run on `seeds`: what a run
 * job hands over. */
function handOver(root, artifacts, id, seeds, change = (x) => x) {
  const from = join(root, 'leaderboard/entries', id, '1.1.0/public');
  const runs = json(join(from, 'runs.json'));
  for (const s of Object.values(runs.scenarios)) {
    s.seeds = seeds;
    s.scores = s.scores.slice(0, seeds.length);
  }
  const out = join(artifacts, `results-${id}-1.1.0`);
  mkdirSync(out, { recursive: true });
  const made = change({ report: json(join(from, 'report.json')), runs });
  writeFileSync(join(out, 'report.json'), JSON.stringify(made.report));
  writeFileSync(join(out, 'runs.json'), JSON.stringify(made.runs));
}

test('a run of the accepting workflow takes the entries without an image', () => {
  const root = scratch(['baseline-greedy']);
  try {
    assert.deepEqual(listEntries(root).map((f) => f.key), FOUNDING.map((id) => `${id}-1.1.0`));
    assert.deepEqual(acceptPlan('pending', root).map((f) => f.id), ['arcadiaos', 'baseline-fcfs', 'baseline-random']);
    const [one] = acceptPlan('arcadiaos/1.1.0', root);
    assert.deepEqual([one.id, one.dir, one.image_tag, one.image_digest], ['arcadiaos', 'leaderboard/entries/arcadiaos/1.1.0', 'arcadiaos-1.1.0', '']);
    // An accepted entry does not change, so it is not accepted twice.
    assert.throws(() => acceptPlan('baseline-greedy/1.1.0', root), /accepted already/);
    assert.throws(() => acceptPlan('nobody/1.0', root), /no entry/);
    assert.throws(() => acceptPlan('../../etc', root), /ID\/VERSION|not acceptable/);
    const all = scratch();
    assert.throws(() => acceptPlan('pending', all), /every entry has been accepted/);
    rmSync(all, { recursive: true });
  } finally {
    rmSync(root, { recursive: true });
  }
});

test('a round ranks every entry accepted by its tag, on seeds from its commit', () => {
  const root = scratch();
  try {
    assert.equal(lastRound(root), 0);
    const plan = roundPlan('round-1', COMMIT, root);
    assert.deepEqual([plan.number, plan.tag, plan.commit], [1, 'round-1', COMMIT]);
    assert.deepEqual(plan.seeds, roundSeeds(COMMIT, json(join(root, 'leaderboard/config.json')).round_seeds));
    assert.deepEqual(plan.entries.map((f) => f.id), FOUNDING);
    assert.ok(plan.entries.every((f) => /^sha256:[0-9a-f]{64}$/.test(f.image_digest)));
    assert.throws(() => roundPlan('v1.1.0', COMMIT, root), /round-N/);
    assert.throws(() => roundPlan('round-1', 'main', root), /commit/);
    // A round does not start while an entry is waiting to be accepted.
    const waiting = scratch(['arcadiaos', 'baseline-greedy']);
    assert.throws(() => roundPlan('round-1', COMMIT, waiting), /not accepted yet: baseline-fcfs\/1\.1\.0, baseline-random\/1\.1\.0/);
    rmSync(waiting, { recursive: true });
  } finally {
    rmSync(root, { recursive: true });
  }
});

test('results are checked before they are kept', () => {
  const root = scratch();
  const artifacts = join(root, 'artifacts');
  try {
    const entry = json(join(root, 'leaderboard/entries/arcadiaos/1.1.0/entry.json'));
    const seeds = seedList('1-50');
    assert.deepEqual(seedList('3,7,12'), [3, 7, 12]);
    assert.throws(() => seedList('all'), /not a seed list/);
    const dir = join(artifacts, 'results-arcadiaos-1.1.0');
    assert.throws(() => readResults(dir, entry, seeds), /did not come back/);
    handOver(root, artifacts, 'arcadiaos', seeds);
    assert.equal(readResults(dir, entry, seeds).report.scorecards.length, 24);
    assert.throws(() => readResults(dir, entry, seeds.slice(1)), /not on the seeds asked for/);
    // Another entry's results, under this entry's directory name.
    handOver(root, artifacts, 'arcadiaos', seeds, (r) => { r.report.scorecards[0].coordinator.name = 'someone-else'; return r; });
    assert.throws(() => readResults(dir, entry, seeds), /under another name/);
    // A report that leaves a track out.
    handOver(root, artifacts, 'arcadiaos', seeds, (r) => { r.report.scorecards = r.report.scorecards.filter((c) => !c.crew); return r; });
    assert.throws(() => readResults(dir, entry, seeds), /fleet design track/);
    handOver(root, artifacts, 'arcadiaos', seeds, (r) => { delete r.runs.scenarios[Object.keys(r.runs.scenarios)[0]]; return r; });
    assert.throws(() => readResults(dir, entry, seeds), /do not cover its tracks/);
    writeFileSync(join(dir, 'report.json'), '{"scorecards":');
    assert.throws(() => readResults(dir, entry, seeds), /not JSON/);
  } finally {
    rmSync(root, { recursive: true });
  }
});

test('accepting writes the public results and the image digest', () => {
  const root = scratch(['baseline-greedy']);
  const artifacts = join(root, 'artifacts');
  const digests = join(root, 'digests');
  try {
    const plan = acceptPlan('pending', root);
    const seeds = seedList('1-50');
    for (const id of ['arcadiaos', 'baseline-fcfs', 'baseline-random']) handOver(root, artifacts, id, seeds);
    mkdirSync(digests);
    writeFileSync(join(digests, 'arcadiaos-1.1.0'), `${digest('a')}\n`);
    writeFileSync(join(digests, 'baseline-fcfs-1.1.0'), 'latest\n');
    const before = readFileSync(join(root, 'leaderboard/entries/baseline-random/1.1.0/entry.json'), 'utf8');
    const done = publishAccept({ plan, artifacts, digests, image: 'ghcr.io/x/arcadiabench-entries', root });
    assert.deepEqual(done.accepted, [{ id: 'arcadiaos', version: '1.1.0', image_digest: digest('a') }]);
    assert.deepEqual(done.failed.map((f) => [f.id, f.reason]), [['baseline-fcfs', 'its image was not pushed'], ['baseline-random', 'its image was not pushed']]);
    assert.equal(json(join(root, 'leaderboard/entries/arcadiaos/1.1.0/entry.json')).image_digest, digest('a'));
    // An entry that did not come through is left as it was.
    assert.equal(readFileSync(join(root, 'leaderboard/entries/baseline-random/1.1.0/entry.json'), 'utf8'), before);
    assert.match(done.message, /^Leaderboard: accept arcadiaos 1\.1\.0\n/);
    assert.match(done.message, /ghcr\.io\/x\/arcadiabench-entries@sha256:a{64}/);
    assert.match(done.summary, /baseline-fcfs 1\.1\.0\*\* not accepted/);
  } finally {
    rmSync(root, { recursive: true });
  }
});

test('a round is published as its file and each entry\'s results, and the board ranks on it', () => {
  const root = scratch();
  const artifacts = join(root, 'artifacts');
  try {
    const plan = roundPlan('round-1', COMMIT, root);
    // Three entries' runs come back; the fourth's do not.
    for (const id of ['arcadiaos', 'baseline-greedy', 'baseline-random']) handOver(root, artifacts, id, plan.seeds);
    const done = publishRound({ plan: plan.entries, artifacts, tag: plan.tag, commit: COMMIT, root });
    const round = json(join(root, 'leaderboard/rounds/round-1.json'));
    assert.deepEqual(round, done.round);
    assert.deepEqual([round.round, round.tag, round.commit, round.catalog, round.simulator], [1, 'round-1', COMMIT, '1.1', 'v1.1.0']);
    assert.deepEqual(round.seeds, plan.seeds);
    assert.deepEqual(round.entries.map((e) => e.id), ['arcadiaos', 'baseline-greedy', 'baseline-random']);
    assert.deepEqual(round.not_ranked.map((e) => [e.id, e.reason]), [['baseline-fcfs', 'its runs did not come back']]);
    assert.ok(existsSync(join(root, 'leaderboard/entries/arcadiaos/1.1.0/round-1/report.json')));
    assert.ok(!existsSync(join(root, 'leaderboard/entries/baseline-fcfs/1.1.0/round-1')));
    assert.match(done.message, /^Leaderboard: round 1\n/);
    assert.match(done.message, /Not ranked: baseline-fcfs 1\.1\.0/);
    assert.equal(lastRound(root), 1);

    const board = currentBoard(join(root, 'leaderboard/entries'), join(root, 'leaderboard/rounds'));
    assert.deepEqual(board.basis, { kind: 'round-1', label: 'round 1', round: 1, tag: 'round-1', commit: COMMIT, seeds: plan.seeds.length });
    const rows = board.tracks.standard.rows;
    assert.deepEqual(rows.map((r) => r.id), ['arcadiaos', 'baseline-greedy', 'baseline-random']);
    assert.equal(rows[0].rank, 1);
    // Beside each rank, the entry's headline on the public seeds.
    assert.equal(rows[0].public.headline.toFixed(3), '0.740');
    assert.equal(rows[0].public.scorecards, 'leaderboard/entries/arcadiaos/1.1.0/public/report.json');
    assert.equal(rows[0].links.scorecards, 'leaderboard/entries/arcadiaos/1.1.0/round-1/report.json');
    // An entry the round does not rank waits with its public-seed figures.
    assert.deepEqual(board.tracks.standard.pending.map((r) => [r.id, r.rank, r.headline.toFixed(3)]), [['baseline-fcfs', null, '0.642']]);
    assert.deepEqual(board.tracks.fleet.pending, []);
    assert.ok(!('scores' in board.tracks.standard.pending[0]));
    // Results in a round's directory do not rank an entry the round's file
    // does not name.
    cpSync(join(root, 'leaderboard/entries/arcadiaos/1.1.0/round-1'), join(root, 'leaderboard/entries/baseline-fcfs/1.1.0/round-1'), { recursive: true });
    assert.equal(currentBoard(join(root, 'leaderboard/entries'), join(root, 'leaderboard/rounds')).tracks.standard.rows.length, 3);
    assert.throws(() => publishRound({ plan: plan.entries, artifacts, tag: 'v2', commit: COMMIT, root }), /round-N/);
  } finally {
    rmSync(root, { recursive: true });
  }
});

test('every published round is the one its tag\'s commit gives', () => {
  // Nothing to check until a round has been published; from then on, this
  // is what keeps a round's file honest.
  const base = join(repo, 'leaderboard/rounds');
  const files = existsSync(base) ? readdirSync(base).filter((name) => name.endsWith('.json')) : [];
  for (const name of files) {
    const round = json(join(base, name));
    assert.equal(name, `round-${round.round}.json`);
    assert.equal(round.tag, `round-${round.round}`);
    assert.deepEqual(round.seeds, roundSeeds(round.commit, round.seeds.length), name);
    for (const e of round.entries) {
      const dir = join(repo, 'leaderboard/entries', e.id, e.version);
      const entry = json(join(dir, 'entry.json'));
      // The image the round ran is the one the entry was accepted with.
      assert.equal(entry.image_digest, e.image_digest, `${name}: ${e.id}`);
      readResults(join(dir, round.tag), entry, round.seeds);
    }
  }
});
