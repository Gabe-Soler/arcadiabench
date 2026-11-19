#!/usr/bin/env node
// What a run of the leaderboard's workflows is going to do, worked out and
// checked before any of it is done.
//
//   node site/tools/plan.mjs accept ID/VERSION|pending [ROOT]
//   node site/tools/plan.mjs next-round [ROOT]
//   node site/tools/plan.mjs round TAG COMMIT [ROOT]
//
// Each prints key=value lines for $GITHUB_OUTPUT. `entries` is a JSON list
// with one object per entry, which the workflow turns into one job each.
// Every value in it has been matched against a pattern
// (site/tools/entry-fields.mjs), and the workflow hands them to git and
// docker as environment variables.
//
// - accept: the entry named, or with `pending` every entry that has no image
//   yet. An entry that has one is never accepted again: it does not change.
// - next-round: the number and tag of the round after the last one published
//   in leaderboard/rounds/.
// - round: the entries a round ranks and its seeds. ROOT is the repository
//   as it was at the round's tag. A round ranks every entry that had been
//   accepted by then, and does not start while one is still waiting.

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { entryFields, parseRef } from './entry-fields.mjs';
import { roundSeeds } from './round-seeds.mjs';

const here = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const json = (path) => JSON.parse(readFileSync(path, 'utf8'));
const dirs = (path) => (existsSync(path) ? readdirSync(path).filter((name) => statSync(join(path, name)).isDirectory()).sort() : []);

export const ROUND_TAG = /^round-([1-9][0-9]{0,3})$/;

/** Every entry under `root`, as the checked fields the workflows use, in id
 * then version order. `key` names the entry in artifact and job names. */
export function listEntries(root = here) {
  const base = join(root, 'leaderboard/entries');
  const found = [];
  for (const id of dirs(base)) {
    for (const version of dirs(join(base, id))) {
      const fields = entryFields(json(join(base, id, version, 'entry.json')), parseRef(`${id}/${version}`));
      found.push({ key: fields.image_tag, ...fields });
    }
  }
  const keys = new Set(found.map((f) => f.key));
  if (keys.size !== found.length) throw new Error('two entries would share an image tag');
  return found;
}

/** The entries one run of the accepting workflow takes. */
export function acceptPlan(ref, root = here) {
  const all = listEntries(root);
  if (ref === 'pending') {
    const waiting = all.filter((f) => f.image_digest === '');
    if (waiting.length === 0) throw new Error('every entry has been accepted already');
    return waiting;
  }
  const where = parseRef(ref);
  const entry = all.find((f) => f.id === where.id && f.version === where.version);
  if (!entry) throw new Error(`there is no entry ${where.id}/${where.version}`);
  if (entry.image_digest !== '') throw new Error(`${entry.id}/${entry.version} has been accepted already, and an accepted entry does not change`);
  return [entry];
}

/** The number of the last round published under `root`, or 0. */
export function lastRound(root = here) {
  const base = join(root, 'leaderboard/rounds');
  if (!existsSync(base)) return 0;
  const numbers = readdirSync(base).map((name) => /^round-([1-9][0-9]{0,3})\.json$/.exec(name)).filter(Boolean).map((m) => Number(m[1]));
  return numbers.length ? Math.max(...numbers) : 0;
}

/** What a round ranks and on which seeds, from the repository at its tag. */
export function roundPlan(tag, commit, root = here) {
  const m = ROUND_TAG.exec(String(tag));
  if (!m) throw new Error('a round is tagged round-N');
  const config = json(join(root, 'leaderboard/config.json'));
  const all = listEntries(root);
  const waiting = all.filter((f) => f.image_digest === '');
  if (waiting.length > 0) {
    throw new Error(`not accepted yet: ${waiting.map((f) => `${f.id}/${f.version}`).join(', ')}. Run the Leaderboard workflow with "pending" first, or take the entry out`);
  }
  if (all.length === 0) throw new Error('there are no entries');
  return { number: Number(m[1]), tag: m[0], commit, seeds: roundSeeds(commit, config.round_seeds), entries: all };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  const out = (pairs) => { for (const [key, value] of Object.entries(pairs)) console.log(`${key}=${value}`); };
  try {
    if (command === 'accept') {
      const entries = acceptPlan(args[0] ?? '', args[1] ? resolve(args[1]) : here);
      out({ entries: JSON.stringify(entries), names: entries.map((f) => `${f.id} ${f.version}`).join(', ') });
    } else if (command === 'next-round') {
      const number = lastRound(args[0] ? resolve(args[0]) : here) + 1;
      out({ number, tag: `round-${number}` });
    } else if (command === 'round') {
      const plan = roundPlan(args[0] ?? '', args[1] ?? '', args[2] ? resolve(args[2]) : here);
      out({ number: plan.number, tag: plan.tag, commit: plan.commit, seeds: plan.seeds.join(','), entries: JSON.stringify(plan.entries) });
    } else {
      console.error('usage: plan.mjs accept ID/VERSION|pending [ROOT] | next-round [ROOT] | round TAG COMMIT [ROOT]');
      process.exit(2);
    }
  } catch (e) {
    console.error(`plan: ${e.message}`);
    process.exit(1);
  }
}
