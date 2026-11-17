// How the front page writes numbers, names and links (site/src/format.js).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { barWidth, crewCounts, finished, fmt3, metaLine, presetName, rankLabel, safeHref, tonnes, variantName } from '../src/format.js';

test('numbers are written the way the results write them', () => {
  assert.equal(fmt3(0.7399), '0.740');
  assert.equal(fmt3(1), '1.000');
  assert.equal(fmt3(null), '–');
  assert.equal(fmt3(NaN), '–');
  assert.equal(finished({ completed: 899, runs: 900 }), '899 / 900');
  assert.equal(tonnes(9900), '9.9 t');
  assert.equal(crewCounts({ scout: 2, excavator: 1, hauler: 1, compactor_sinterer: 10 }), '2 · 1 · 1 · 10');
});

test('a shared rank is marked', () => {
  assert.equal(rankLabel({ rank: 1, tied: false }), '1');
  assert.equal(rankLabel({ rank: 2, tied: true }), '2=');
});

test('variants and outposts have names', () => {
  assert.equal(variantName('cascading_faults'), 'Cascading faults');
  assert.equal(variantName('nuclear_heavy'), 'Nuclear heavy');
  assert.equal(variantName('some_new_variant'), 'some new variant');
  assert.equal(presetName('medium'), 'Medium');
  assert.equal(presetName(null), 'Crew');
});

test('a bar is the headline against the leader, within the track', () => {
  assert.equal(barWidth(0.74, 0.74), 100);
  assert.equal(Math.round(barWidth(0.666, 0.74)), 90);
  assert.equal(barWidth(0.9, 0.74), 100);
  assert.equal(barWidth(0.5, 0), 0);
  assert.equal(barWidth(NaN, 0.74), 0);
});

test('the line under a name leaves out what is missing', () => {
  assert.equal(metaLine({ version: '1.1.0', authors: ['A', 'B'], licence: 'MIT' }), '1.1.0 · A, B · MIT');
  assert.equal(metaLine({ version: '2', authors: ['A'], licence: '' }), '2 · A');
});

test('only links to this site or to https are followed', () => {
  assert.equal(safeHref('https://github.com/x/y/tree/abc'), 'https://github.com/x/y/tree/abc');
  assert.equal(safeHref('leaderboard/entries/a/1/public/report.json'), 'leaderboard/entries/a/1/public/report.json');
  assert.equal(safeHref('viewer/?log=../results/v1.1/replays/run.jsonl.gz'), 'viewer/?log=../results/v1.1/replays/run.jsonl.gz');
  for (const bad of ['javascript:alert(1)', 'http://example.com', '//example.com/x', 'viewer//example.com', 'data:text/html,x', '', null, 42]) {
    assert.equal(safeHref(bad), null, String(bad));
  }
});
