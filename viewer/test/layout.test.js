// Map layout for crowded scenes: 20 robots must stay legible.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spread, placeLabels } from '../src/layout.js';

const R = 12;
const overlaps = (a, b) => a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0;
const body = (p) => ({ x0: p.sx - R - 2, x1: p.sx + R + 2, y0: p.sy - R - 6, y1: p.sy + R + 13 });

test('spread separates a crew parked in a row, and a pile on one spot', () => {
  const row = Array.from({ length: 20 }, (_, i) => ({ x: 10 + i, y: 10 }));
  spread(row, 1.8);
  const pile = Array.from({ length: 20 }, () => ({ x: 30, y: 30 }));
  spread(pile, 1.8, 60);
  for (const pts of [row, pile]) {
    for (let i = 0; i < pts.length; i++) {
      for (let j = 0; j < i; j++) assert.ok(Math.hypot(pts[i].x - pts[j].x, pts[i].y - pts[j].y) > 1.5, `${i}, ${j}`);
    }
  }
});

function labelScene(points, priority = -1) {
  const items = points.map((p, i) => ({ sx: p.x, sy: p.y, w: 30, priority: i === priority }));
  return { items, spots: placeLabels(items, { R }) };
}

test('labels of 20 robots never cover each other or another robot', () => {
  // A 5 × 4 block with markers just apart, as a 20-robot crew at its start.
  const points = [];
  for (let i = 0; i < 20; i++) points.push({ x: 200 + (i % 5) * 30, y: 200 + Math.floor(i / 5) * 46 });
  const { items, spots } = labelScene(points, 7);
  const placed = spots.map((s, i) => [s, i]).filter(([s]) => s);
  assert.ok(placed.length >= 10, `${placed.length} labels shown`);
  assert.ok(spots[7], 'the selected robot is always labelled');
  for (const [a, i] of placed) {
    for (const [b, j] of placed) if (i < j) assert.ok(!overlaps(a.box, b.box), `labels ${i} and ${j}`);
    // Only the selected robot may cover a neighbour, when nowhere is clear.
    if (i === 7) continue;
    items.forEach((p, j) => {
      if (j !== i) assert.ok(!overlaps(a.box, body(p)), `label ${i} covers robot ${j}`);
    });
  }
});

test('robots apart from each other all get labels above them', () => {
  const points = Array.from({ length: 20 }, (_, i) => ({ x: 60 + (i % 10) * 80, y: 80 + Math.floor(i / 10) * 90 }));
  const { spots } = labelScene(points);
  assert.ok(spots.every((s) => s && s.base === 'bottom' && s.align === 'center'));
});
