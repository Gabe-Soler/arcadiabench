import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SiteIndex, padProgress, surfaceCode, SURFACES, OTHER_SURFACE } from '../src/site.js';

function site(width = 6, height = 4, elev = -4102) {
  const cells = [];
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) cells.push({ at: { x, y }, cell: { elevation_m: elev, surface: 'natural', boulders: x === 1 && y === 1 ? [{ diameter_m: 0.5 }] : [] } });
  return new SiteIndex({ site_id: 's', width, height, cell_size_m: 1, cells });
}

const pad = {
  area: { min: { x: 1, y: 1 }, max: { x: 2, y: 1 } },
  target_elevation_m: -4102,
  surface_tolerance_m: 0.05,
  max_slope_deg: 2,
  stabilize: true,
  berm: { cells: [{ x: 4, y: 3 }], height_m: 0.5 },
};

test('surface names map to codes; unknown names get their own code', () => {
  assert.equal(SURFACES[surfaceCode('graded')], 'graded');
  assert.equal(surfaceCode(undefined), 0);
  assert.equal(surfaceCode('glazed'), OTHER_SURFACE);
});

test('cells keep their initial state until a change, and changes inherit missing fields', () => {
  const s = site();
  const i = s.index({ x: 1, y: 1 });
  assert.equal(s.index({ x: 6, y: 0 }), -1);
  assert.deepEqual(s.boulders0[i], [0.5]);
  s.addChange(i, 100, { surface: 'cleared', boulders: [], elevation_m: -4102.2 });
  s.addChange(i, 200, { surface: 'graded' });
  s.seal();
  assert.equal(s.cellAt(i, 50).surf, surfaceCode('natural'));
  assert.equal(s.cellAt(i, 100).surf, surfaceCode('cleared'));
  assert.equal(s.cellAt(i, 100).boulders, null);
  assert.equal(s.cellAt(i, 250).elev, -4102.2, 'elevation carried over');
  assert.equal(s.stateIndex(i, 150), 0);
  assert.equal(s.changesUpTo(150), 1);
});

test('pad progress counts cells in spec, scan verification and berm cells', () => {
  const s = site();
  const a = s.index({ x: 1, y: 1 });
  const b = s.index({ x: 2, y: 1 });
  const berm = s.index({ x: 4, y: 3 });
  s.addChange(a, 100, { surface: 'stabilized', boulders: [], elevation_m: -4102.01 });
  s.addChange(b, 300, { surface: 'stabilized', boulders: [], elevation_m: -4101.9 }); // out of tolerance
  s.addChange(b, 500, { surface: 'stabilized', boulders: [], elevation_m: -4102.0 });
  s.addChange(berm, 600, { surface: 'berm', elevation_m: -4101.45 });
  s.seal();
  const scans = [
    { start: 150, end: 200, cells: [a, b] }, // verifies a (changed at 100) and b (unchanged so far)
    { start: 400, end: 700, cells: [a, b] }, // b changed at 500, after this scan began
  ];
  const p = padProgress(s, pad, scans);
  assert.equal(p.total, 2);
  assert.equal(p.inSpec.step(50), 0);
  assert.equal(p.inSpec.step(100), 1);
  assert.equal(p.inSpec.step(300), 1);
  assert.equal(p.inSpec.step(500), 2);
  assert.equal(p.verified.step(199), 0);
  assert.equal(p.verified.step(200), 2);
  assert.equal(p.verified.step(300), 1, 'a change unverifies the cell');
  assert.equal(p.verified.step(700), 1, 'a scan that began before the change does not verify it');
  assert.equal(p.bermTotal, 1);
  assert.equal(p.berm.step(599), 0);
  assert.equal(p.berm.step(600), 1);
  assert.ok(Math.abs(p.maxSlopeDeg(400) - (Math.atan(0.11) * 180) / Math.PI) < 1e-9);
});

test('a pad that only needs compaction counts compacted cells', () => {
  const s = site();
  s.addChange(s.index({ x: 1, y: 1 }), 10, { surface: 'compacted', boulders: [] });
  s.seal();
  const p = padProgress(s, { ...pad, stabilize: false, berm: undefined }, []);
  assert.equal(p.inSpec.step(10), 1);
  assert.equal(p.bermTotal, 0);
});
