#!/usr/bin/env node
// Check the results the run jobs handed over and write what the repository
// keeps of them. This is the job that can write to the repository, so it
// runs no submitted code and trusts nothing it is handed: every results file
// has to parse, be about the entry it claims, cover the entry's tracks and
// be on the seeds that were asked for.
//
//   node site/tools/publish-results.mjs accept --plan JSON --artifacts DIR --digests DIR --image NAME --out DIR [--root DIR]
//   node site/tools/publish-results.mjs round  --plan JSON --artifacts DIR --tag TAG --commit SHA --out DIR [--root DIR] [--at-tag DIR]
//
// --plan is the `entries` list from site/tools/plan.mjs. --artifacts holds
// one directory per entry, results-<key>, with report.json and runs.json as
// site/tools/entry-results.mjs writes them.
//
// - accept writes <entry>/public/ and the image's digest (--digests holds
//   one file per entry, named by its key) into the entry file.
// - round writes <entry>/round-N/ and leaderboard/rounds/round-N.json. An
//   entry whose runs did not come back is named in the round file and is not
//   ranked. --at-tag is the repository as it was at the round's tag: the
//   round's settings are read there.
//
// --out gets message.txt (the commit message) and summary.md. The exit code
// is 1 when no entry came through.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fleetRow, standardRow } from '../src/model.js';
import { ROUND_TAG } from './plan.mjs';
import { roundSeeds } from './round-seeds.mjs';

const here = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const json = (path) => JSON.parse(readFileSync(path, 'utf8'));
const DIGEST = /^sha256:[0-9a-f]{64}$/;

function check(condition, message) {
  if (!condition) throw new Error(message);
}

/** `1-50` or `3,7,12` as a list of seeds. */
export function seedList(text) {
  const range = /^(\d+)-(\d+)$/.exec(String(text));
  const seeds = range
    ? Array.from({ length: Number(range[2]) - Number(range[1]) + 1 }, (_, i) => Number(range[1]) + i)
    : String(text).split(',').map(Number);
  check(seeds.length > 0 && seeds.every((s) => Number.isSafeInteger(s) && s >= 0), `not a seed list: ${text}`);
  return seeds;
}

/** An entry's results from `dir`, checked: they are this entry's, on these
 * seeds, and they cover its tracks. Throws with the reason otherwise. */
export function readResults(dir, entry, seeds) {
  check(existsSync(join(dir, 'report.json')) && existsSync(join(dir, 'runs.json')), 'its runs did not come back');
  let report;
  let runs;
  try {
    report = json(join(dir, 'report.json'));
    runs = json(join(dir, 'runs.json'));
  } catch {
    throw new Error('its results are not JSON');
  }
  check(Array.isArray(report?.scorecards) && report.scorecards.length > 0 && Array.isArray(report.resilience), 'its report has no scorecards');
  for (const card of [...report.scorecards, ...report.resilience]) {
    check(card?.coordinator?.name === entry.id && card.coordinator.version === entry.version, 'its results are under another name');
  }
  const scenarios = Object.values(runs?.scenarios ?? {});
  const tracks = entry.tracks;
  check(scenarios.length === (tracks.includes('standard') ? 3 : 0) + (tracks.includes('fleet') ? 1 : 0), 'its per-seed scores do not cover its tracks');
  for (const s of scenarios) {
    check(Array.isArray(s.seeds) && Array.isArray(s.scores) && s.seeds.length === s.scores.length, 'its per-seed scores are malformed');
    check(s.seeds.length === seeds.length && s.seeds.every((seed, i) => seed === seeds[i]), 'its runs are not on the seeds asked for');
    check(s.scores.every((x) => typeof x === 'number' && x >= 0 && x <= 1), 'its per-seed scores are malformed');
  }
  if (tracks.includes('standard')) check(standardRow(entry, report, runs), 'its report does not cover the Standard track');
  if (tracks.includes('fleet')) check(fleetRow(entry, report, runs), 'its report does not cover the fleet design track');
  return { report, runs };
}

function write(dir, { report, runs }) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'report.json'), `${JSON.stringify(report)}\n`);
  writeFileSync(join(dir, 'runs.json'), `${JSON.stringify(runs)}\n`);
}

/** Each planned entry with its results, or with the reason it has none. */
function gather(plan, artifacts, root, seeds) {
  const ok = [];
  const failed = [];
  for (const item of plan) {
    const dir = join(root, 'leaderboard/entries', item.id, item.version);
    try {
      const entry = json(join(dir, 'entry.json'));
      check(entry.id === item.id && entry.version === item.version, 'its entry file is another entry');
      ok.push({ item, dir, entry, results: readResults(join(artifacts, `results-${item.key}`), entry, seeds) });
    } catch (e) {
      failed.push({ id: item.id, version: item.version, reason: e.message });
    }
  }
  return { ok, failed };
}

const names = (list) => list.map((x) => `${x.id} ${x.version}`).join(', ');

/** Accept entries: their public-seed results and their image's digest. */
export function publishAccept({ plan, artifacts, digests, image, root = here }) {
  const config = json(join(root, 'leaderboard/config.json'));
  const { ok, failed } = gather(plan, artifacts, root, seedList(config.public_seeds));
  const accepted = [];
  for (const { item, dir, entry, results } of ok) {
    const file = join(digests, item.key);
    const digest = existsSync(file) ? readFileSync(file, 'utf8').trim() : '';
    if (!DIGEST.test(digest)) {
      failed.push({ id: item.id, version: item.version, reason: 'its image was not pushed' });
      continue;
    }
    write(join(dir, 'public'), results);
    writeFileSync(join(dir, 'entry.json'), `${JSON.stringify({ ...entry, image_digest: digest }, null, 2)}\n`);
    accepted.push({ id: item.id, version: item.version, image_digest: digest });
  }
  const message = [
    `Leaderboard: accept ${names(accepted)}`,
    '',
    `Each was built from its entry's pinned commit and run in the sandbox on seeds ${config.public_seeds}, twice, with the same result both times.`,
    '',
    ...accepted.map((a) => `${a.id} ${a.version}: ${image}@${a.image_digest}`),
  ].join('\n');
  const summary = [
    ...accepted.map((a) => `- **${a.id} ${a.version}** accepted. Image: \`${image}@${a.image_digest}\``),
    ...failed.map((f) => `- **${f.id} ${f.version}** not accepted: ${f.reason}.`),
  ].join('\n');
  return { accepted, failed, message, summary };
}

/** Publish a round: every entry's results on the round's seeds, and the
 * round's own file. */
export function publishRound({ plan, artifacts, tag, commit, root = here, atTag = root }) {
  const m = ROUND_TAG.exec(String(tag));
  check(m, 'a round is tagged round-N');
  const number = Number(m[1]);
  const config = json(join(atTag, 'leaderboard/config.json'));
  const seeds = roundSeeds(commit, config.round_seeds);
  const { ok, failed } = gather(plan, artifacts, root, seeds);
  for (const { dir, results } of ok) write(join(dir, tag), results);
  const digestOf = (id, version) => plan.find((p) => p.id === id && p.version === version).image_digest;
  const round = {
    round: number,
    tag,
    commit,
    catalog: config.catalog,
    simulator: config.simulator,
    seeds,
    entries: ok.map(({ item }) => ({ id: item.id, version: item.version, image_digest: item.image_digest })),
    not_ranked: failed.map((f) => ({ ...f, image_digest: digestOf(f.id, f.version) })),
  };
  for (const e of [...round.entries, ...round.not_ranked]) check(DIGEST.test(e.image_digest), `${e.id} ${e.version} has no image`);
  mkdirSync(join(root, 'leaderboard/rounds'), { recursive: true });
  writeFileSync(join(root, 'leaderboard/rounds', `${tag}.json`), `${JSON.stringify(round, null, 2)}\n`);
  const message = [
    `Leaderboard: round ${number}`,
    '',
    `Round ${number} is the tag ${tag}, at ${commit}. Its ${seeds.length} seeds are computed from that commit (site/tools/round-seeds.mjs). Every entry accepted by then was run from its image in the sandbox on those seeds, twice, with the same result both times.`,
    '',
    `Ranked: ${names(round.entries) || 'none'}.`,
    ...(failed.length ? [`Not ranked: ${failed.map((f) => `${f.id} ${f.version} (${f.reason})`).join('; ')}.`] : []),
  ].join('\n');
  const summary = [
    `- Round ${number}: tag \`${tag}\` at \`${commit}\`, ${seeds.length} seeds.`,
    ...round.entries.map((e) => `- **${e.id} ${e.version}** ranked.`),
    ...failed.map((f) => `- **${f.id} ${f.version}** not ranked: ${f.reason}.`),
  ].join('\n');
  return { round, accepted: round.entries, failed, message, summary };
}

function flags(args) {
  const found = {};
  for (let i = 0; i < args.length; i += 2) {
    check(args[i]?.startsWith('--') && args[i + 1] !== undefined, `bad argument: ${args[i]}`);
    found[args[i].slice(2)] = args[i + 1];
  }
  return found;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...rest] = process.argv.slice(2);
  try {
    const f = flags(rest);
    check(f.plan && f.artifacts && f.out, 'usage: publish-results.mjs accept|round --plan JSON --artifacts DIR --out DIR ...');
    const plan = JSON.parse(f.plan);
    const root = f.root ? resolve(f.root) : here;
    let done;
    if (command === 'accept') {
      check(f.digests && f.image, 'accept needs --digests and --image');
      done = publishAccept({ plan, artifacts: resolve(f.artifacts), digests: resolve(f.digests), image: f.image, root });
    } else if (command === 'round') {
      check(f.tag && f.commit, 'round needs --tag and --commit');
      done = publishRound({ plan, artifacts: resolve(f.artifacts), tag: f.tag, commit: f.commit, root, atTag: f['at-tag'] ? resolve(f['at-tag']) : root });
    } else {
      throw new Error('the command is accept or round');
    }
    mkdirSync(resolve(f.out), { recursive: true });
    writeFileSync(join(resolve(f.out), 'message.txt'), `${done.message}\n`);
    writeFileSync(join(resolve(f.out), 'summary.md'), `${done.summary}\n`);
    console.log(done.summary);
    if (done.accepted.length === 0) process.exit(1);
  } catch (e) {
    console.error(`publish-results: ${e.message}`);
    process.exit(1);
  }
}
