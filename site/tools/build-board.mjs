#!/usr/bin/env node
// Build leaderboard/board.json, the file the front page draws, from the
// entries in leaderboard/entries/<id>/<version>/.
//
//   node site/tools/build-board.mjs [OUT]     # OUT defaults to leaderboard/board.json
//
// Each entry is an entry.json and, under public/, a report.json (as
// `arcadiabench score --json` writes it) and a runs.json (one score per seed
// for the nominal scenarios). The board is not committed: the Pages workflow
// builds it when the site is deployed, and so does this command for a local
// preview. The same entries always give the same file.
//
// Once a round has been published (leaderboard/rounds/round-N.json) the board
// ranks on the last one: each entry's results on it are in its round-N/
// directory, beside public/.

import { readFileSync, readdirSync, writeFileSync, existsSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildBoard } from '../src/model.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

const dirs = (path) => readdirSync(path).filter((name) => statSync(join(path, name)).isDirectory()).sort();
const json = (path) => JSON.parse(readFileSync(path, 'utf8'));

/** Every entry under `leaderboard/entries`, in id then version order. */
export function readEntries(base = join(root, 'leaderboard/entries'), basis = 'public') {
  const entries = [];
  for (const id of dirs(base)) {
    for (const version of dirs(join(base, id))) {
      const dir = join(base, id, version);
      const entry = json(join(dir, 'entry.json'));
      if (entry.id !== id || entry.version !== version) {
        throw new Error(`${dir}/entry.json says ${entry.id} ${entry.version}`);
      }
      const report = join(dir, basis, 'report.json');
      if (!existsSync(report)) continue;
      const runs = join(dir, basis, 'runs.json');
      entries.push({ entry, report: json(report), runs: existsSync(runs) ? json(runs) : null });
    }
  }
  return entries;
}

/** The last round published under `base`, or null. */
export function latestRound(base = join(root, 'leaderboard/rounds')) {
  if (!existsSync(base)) return null;
  const numbers = readdirSync(base).map((name) => /^round-([1-9][0-9]{0,3})\.json$/.exec(name)).filter(Boolean).map((m) => Number(m[1]));
  if (numbers.length === 0) return null;
  const round = json(join(base, `round-${Math.max(...numbers)}.json`));
  if (round.round !== Math.max(...numbers) || round.tag !== `round-${round.round}`) throw new Error(`${base}/${round.tag}.json is another round`);
  return round;
}

/** The board as it stands: on the last round if there is one, on the public
 * seeds until then. */
export function currentBoard(entriesBase = join(root, 'leaderboard/entries'), roundsBase = join(root, 'leaderboard/rounds')) {
  const open = readEntries(entriesBase);
  const round = latestRound(roundsBase);
  if (!round) return buildBoard(open);
  // Only what the round's own file names is ranked on it.
  const named = new Set(round.entries.map((e) => `${e.id}/${e.version}`));
  const ranked = readEntries(entriesBase, round.tag).filter((e) => named.has(`${e.entry.id}/${e.entry.version}`));
  const basis = { kind: round.tag, label: `round ${round.round}`, round: round.round, tag: round.tag, commit: round.commit, seeds: round.seeds.length };
  return buildBoard(ranked, basis, open);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const out = resolve(process.argv[2] ?? join(root, 'leaderboard/board.json'));
  const board = currentBoard();
  writeFileSync(out, `${JSON.stringify(board, null, 2)}\n`);
  const count = (track) => board.tracks[track].rows.length;
  console.log(`build-board: ${count('standard')} entries on the Standard track, ${count('fleet')} on the fleet design track -> ${out}`);
}
