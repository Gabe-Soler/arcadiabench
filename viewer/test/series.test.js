import { test } from 'node:test';
import assert from 'node:assert/strict';
import { lastAtOrBefore, firstAfter, firstAtOrAfter, Series, Intervals } from '../src/series.js';

test('lastAtOrBefore finds the last sample at or before t', () => {
  const t = [0, 10, 10, 20, 30];
  assert.equal(lastAtOrBefore(t, -1), -1);
  assert.equal(lastAtOrBefore(t, 0), 0);
  assert.equal(lastAtOrBefore(t, 5), 0);
  assert.equal(lastAtOrBefore(t, 10), 2);
  assert.equal(lastAtOrBefore(t, 29.9), 3);
  assert.equal(lastAtOrBefore(t, 1e9), 4);
  assert.equal(lastAtOrBefore([], 3), -1);
  assert.equal(firstAfter(t, 10), 3);
  assert.equal(firstAtOrAfter(t, 10), 1);
  assert.equal(firstAtOrAfter(t, 31), 5);
});

test('Series interpolates linearly and holds flat outside its samples', () => {
  const s = new Series();
  s.push(10, 100);
  s.push(20, 200);
  s.push(20, 250); // same time replaces
  s.push(15, 999); // out of order is ignored
  s.push(30, Number.NaN); // non-finite is ignored
  assert.deepEqual(s.t, [10, 20]);
  assert.equal(s.linear(0), 100);
  assert.equal(s.linear(15), 175);
  assert.equal(s.linear(25), 250);
  assert.equal(s.step(9), null);
  assert.equal(s.step(19.9), 100);
  assert.equal(s.min(), 100);
  assert.equal(s.max(), 250);
  assert.equal(new Series().linear(5), null);
});

test('Intervals answer membership and clipped totals', () => {
  const iv = new Intervals([
    { start: 50, end: 60 },
    { start: 10, end: 20 },
  ]);
  assert.equal(iv.at(5), null);
  assert.equal(iv.at(10).start, 10);
  assert.equal(iv.at(20), null, 'end is exclusive');
  assert.equal(iv.at(55).start, 50);
  assert.equal(iv.total(), 20);
  assert.equal(iv.total(15, 55), 10);
});
