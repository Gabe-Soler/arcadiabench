#!/usr/bin/env node
// The seeds of a leaderboard round, from the commit its tag points at.
//
//   node site/tools/round-seeds.mjs COMMIT [COUNT]     # prints s1,s2,...
//
// A round is a tag on main. Its seeds are computed from the tag's commit
// hash, which nobody can know while the entries are being written: seed i is
// the first 48 bits of SHA-256("arcadiabench leaderboard round\n" + commit +
// "\n" + i), for i = 1, 2, ... A seed that falls on the public seeds 1-50, or
// repeats an earlier one, is skipped. 48 bits keep every seed exact in JSON.

import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The seeds reserved for everyone: entrants check their setup on them. */
export const PUBLIC_SEEDS = 50;

export function roundSeeds(commit, count = 50) {
  if (!/^[0-9a-f]{40}$/.test(commit)) throw new Error(`not a full commit hash: ${commit}`);
  if (!Number.isInteger(count) || count < 1 || count > 1000) throw new Error(`bad seed count: ${count}`);
  const seeds = [];
  for (let i = 1; seeds.length < count; i++) {
    const digest = createHash('sha256').update(`arcadiabench leaderboard round\n${commit}\n${i}`).digest();
    const seed = digest.readUIntBE(0, 6);
    if (seed > PUBLIC_SEEDS && !seeds.includes(seed)) seeds.push(seed);
  }
  // In ascending order, as the benchmark runs and records them.
  return seeds.sort((a, b) => a - b);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [commit, count] = process.argv.slice(2);
  try {
    console.log(roundSeeds(commit ?? '', count === undefined ? 50 : Number(count)).join(','));
  } catch (e) {
    console.error(`round-seeds: ${e.message}`);
    process.exit(2);
  }
}
