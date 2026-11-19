// The leaderboard's arithmetic (site/src/model.js) and the board built from
// the entries in leaderboard/entries/.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildBoard, fleetRow, mean, pairedGap, rankRows, separable, standardRow, TIE_Z } from '../src/model.js';
import { readEntries } from '../tools/build-board.mjs';

const near = (a, b, eps = 1e-9) => Math.abs(a - b) <= eps;

test('a paired gap is the mean difference and its standard error', () => {
  const g = pairedGap([0.7, 0.8, 0.9, 0.6], [0.6, 0.6, 0.8, 0.6]);
  assert.ok(near(g.mean, 0.1));
  // Differences 0.1, 0.2, 0.1, 0: variance 0.02 / 3, over four pairs.
  assert.ok(near(g.se, Math.sqrt(0.02 / 3 / 4)));
  assert.equal(g.n, 4);
  // Scores of different runs cannot be paired.
  assert.ok(Number.isNaN(pairedGap([1, 2], [1]).mean));
  assert.ok(Number.isNaN(mean([])));
});

test('entries are apart only when their paired gap is clear of zero', () => {
  const base = Array.from({ length: 50 }, (_, i) => 0.6 + 0.001 * (i % 7));
  // The same scores, shifted by more than the scatter: apart.
  assert.equal(separable(base.map((x) => x + 0.05), base), true);
  // Ahead on half the runs and behind on the other half by the same: a tie.
  const mixed = base.map((x, i) => x + (i % 2 ? 0.02 : -0.02));
  assert.equal(separable(mixed, base), false);
  // Identical scores tie; a constant shift is apart however small.
  assert.equal(separable(base, base), false);
  assert.equal(separable(base.map((x) => x + 1e-6), base), true);
  // What cannot be compared is not called a tie.
  assert.equal(separable([0.5], [0.5]), true);
  assert.equal(separable([0.5, 0.6], [0.5]), true);
  assert.equal(TIE_Z, 2);
});

test('ranks are shared by entries that cannot be told apart', () => {
  const runs = Array.from({ length: 40 }, (_, i) => i);
  const scores = (level, wobble) => runs.map((i) => level + (i % 2 ? wobble : -wobble));
  const row = (id, headline, s, runsKey = 'k') => ({ id, headline, resilience: 0.9, scores: s, runsKey });
  const rows = rankRows([
    row('c', 0.60, scores(0.60, 0.00)),
    row('a', 0.70, scores(0.70, 0.00)),
    row('b', 0.601, scores(0.601, 0.03)),
    row('d', 0.50, scores(0.50, 0.00)),
    row('e', 0.50, scores(0.50, 0.00), 'other runs'),
  ]);
  assert.deepEqual(rows.map((r) => [r.id, r.rank, r.tied]), [
    ['a', 1, false],
    // b and c differ by 0.001 with a scatter of 0.03: one rank between them.
    ['b', 2, true],
    ['c', 2, true],
    // d is clear of the group's first row, so it is fourth, not third.
    ['d', 4, false],
    // e has the same number on other runs: that is no tie.
    ['e', 5, false],
  ]);
});

test('the board ranks the published entries', () => {
  const entries = readEntries();
  assert.deepEqual(entries.map((e) => e.entry.id), ['arcadiaos', 'baseline-fcfs', 'baseline-greedy', 'baseline-random']);
  const board = buildBoard(entries);
  assert.equal(board.catalog, '1.1');
  assert.deepEqual(board.basis, { kind: 'public', label: 'seeds 1–50' });

  const standard = board.tracks.standard.rows;
  assert.equal(board.tracks.standard.scenarios, 18);
  assert.deepEqual(standard.map((r) => r.id), ['arcadiaos', 'baseline-greedy', 'baseline-random', 'baseline-fcfs']);
  const os = standard[0];
  // The nominal headline averaged over the three outposts, as published.
  assert.equal(os.headline.toFixed(3), '0.740');
  assert.deepEqual(Object.values(os.presets).map((x) => x.toFixed(3)), ['0.764', '0.764', '0.692']);
  assert.equal(os.resilience.toFixed(3), '0.974');
  assert.deepEqual([os.completed, os.runs, os.rank, os.tied], [900, 900, 1, false]);
  assert.equal(os.baseline, false);
  assert.equal(os.grid.length, 3);
  assert.deepEqual(os.grid[0].cells.map((c) => c.variant), ['nominal', 'dust_storm', 'conjunction', 'cascading_faults', 'solar_only', 'nuclear_heavy']);
  assert.equal(os.links.source, 'https://github.com/Gabe-Soler/arcadiaos/tree/e69fe4cffc6e8b1c733cbd4156195d3147561908');
  assert.equal(os.links.scorecards, 'leaderboard/entries/arcadiaos/1.1.0/public/report.json');
  assert.equal(os.links.replay, 'viewer/?log=../results/v1.1/replays/landing-pad-1.1-medium-nominal-1.jsonl.gz');
  const [greedy, random, fcfs] = standard.slice(1);
  assert.deepEqual([greedy.headline, random.headline, fcfs.headline].map((x) => x.toFixed(3)), ['0.666', '0.660', '0.642']);
  assert.deepEqual([greedy.completed, greedy.runs], [899, 900]);
  assert.ok(greedy.baseline && random.baseline && fcfs.baseline);
  // What the page gets carries no per-run scores.
  assert.ok(!('scores' in os) && !('runsKey' in os));

  // Ties follow the paired runs, whatever they say.
  const by = Object.fromEntries(entries.map((e) => [e.entry.id, e]));
  const paired = (id) => ['small', 'medium', 'large'].flatMap((p) => by[id].runs.scenarios[`landing-pad-1.1-${p}-nominal`].scores);
  assert.equal(paired('arcadiaos').length, 150);
  assert.equal(separable(paired('arcadiaos'), paired('baseline-greedy')), true);
  const tie = !separable(paired('baseline-greedy'), paired('baseline-random'));
  assert.deepEqual([greedy.rank, greedy.tied, random.rank, random.tied], tie ? [2, true, 2, true] : [2, false, 3, false]);

  const fleet = board.tracks.fleet.rows;
  assert.deepEqual(fleet.map((r) => r.id), ['arcadiaos', 'baseline-greedy']);
  assert.equal(fleet[0].headline.toFixed(3), '0.634');
  assert.deepEqual(fleet[0].crew, { scout: 2, excavator: 1, hauler: 1, compactor_sinterer: 10 });
  assert.equal(fleet[0].mass_kg, 9900);
  assert.equal(fleet[0].grid[0].cells.length, 6);
  assert.deepEqual(fleet.map((r) => r.rank), [1, 2]);
});

test('an entry that did not take a track has no row on it', () => {
  const [random] = readEntries().filter((e) => e.entry.id === 'baseline-random');
  assert.equal(fleetRow(random.entry, random.report, random.runs), null);
  assert.ok(standardRow(random.entry, random.report, random.runs));
  // A report that misses a scenario does not cover the Standard track.
  const partial = { ...random.report, scorecards: random.report.scorecards.slice(1) };
  assert.equal(standardRow(random.entry, partial, random.runs), null);
});
