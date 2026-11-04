// The committed sample logs (viewer/samples/*.jsonl.gz) load through the
// same gzip path as the browser and replay sensibly from start to end.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, statSync } from 'node:fs';
import { parseLog, openBytes } from '../src/parse.js';
import { ModelBuilder } from '../src/model.js';

const SAMPLES = new URL('../samples/', import.meta.url);

async function load(name) {
  const bytes = readFileSync(new URL(name, SAMPLES));
  const b = new ModelBuilder();
  const result = await parseLog(openBytes(bytes, name), { onRecord: (r, l) => b.add(r, l) });
  return { m: b.finish(), result };
}

/** Query everything the viewer draws at many times across the run. */
function sweep(m, steps = 400) {
  for (let k = 0; k <= steps; k++) {
    const t = m.t0 + ((m.tEnd - m.t0) * k) / steps;
    for (let i = 0; i < m.robots.length; i++) {
      const r = m.robotAt(i, t);
      assert.ok(Number.isFinite(r.x) && Number.isFinite(r.y), `${r.id} at ${t}`);
      assert.ok(r.x >= 0 && r.y >= 0 && r.x < m.site.width && r.y < m.site.height);
    }
    m.padAt(t);
    m.powerAt(t);
    m.weatherAt(t);
    m.commsAt(t);
    m.inFlight(t);
    m.activeRelays(t);
    m.feedIndexAt(t);
  }
}

test('samples are commit-sized', () => {
  for (const name of ['builder-seed34.jsonl.gz', 'null-seed7.jsonl.gz']) {
    assert.ok(statSync(new URL(name, SAMPLES)).size <= 1.5e6, name);
  }
});

test('the scripted builder sample builds and verifies the pad through a dust storm', async () => {
  const { m, result } = await load('builder-seed34.jsonl.gz');
  assert.equal(result.gzip, true);
  assert.equal(result.badLines, 0);
  assert.deepEqual(m.warnings, []);
  assert.equal(m.info.coordinator.name, 'scripted-builder');
  assert.equal(m.info.seed, 34);
  assert.equal(m.info.endReason, 'goals_complete');
  assert.deepEqual(m.padAt(m.t0), { inSpec: 0, verified: 0, berm: 0, total: 400, bermTotal: 12 });
  assert.deepEqual(m.padAt(m.tEnd), { inSpec: 400, verified: 400, berm: 12, total: 400, bermTotal: 12 });
  assert.equal(m.weather.storms.items.length, 1);
  assert.ok(m.weather.storms.items[0].peakTau > 3);

  // The hauler's trips to the stockpile happen between snapshots.
  const hl = m.robotIndex.get('hl-1');
  const haul = m.robots[hl].commands.find((c) => c.cmd.type === 'haul');
  let atStockpile = false;
  for (let t = haul.start; t <= haul.end; t += 2) {
    const r = m.robotAt(hl, t);
    if (Math.abs(r.x - haul.cmd.from.x) < 1e-6 && Math.abs(r.y - haul.cmd.from.y) < 1e-6) atStockpile = true;
  }
  assert.ok(atStockpile);
  sweep(m);
});

test('the null-coordinator sample sits idle for 60 sols until the batteries run out', async () => {
  const { m } = await load('null-seed7.jsonl.gz');
  assert.deepEqual(m.warnings, []);
  assert.equal(m.info.coordinator.name, 'null-coordinator');
  assert.equal(m.info.endReason, 'time_limit');
  assert.equal(m.stats.commands, 0);
  assert.ok(Math.abs((m.tEnd - m.t0) / m.info.solLength - 60) < 1e-9);
  for (let i = 0; i < m.robots.length; i++) {
    const a = m.robotAt(i, m.t0);
    const b = m.robotAt(i, m.tEnd);
    assert.deepEqual([a.x, a.y], [b.x, b.y], 'nobody moves');
  }
  const offline = m.robots.filter((_, i) => m.robotAt(i, m.tEnd).status === 'offline').length;
  assert.ok(offline >= 3, `${offline} robots offline at the end`);
  assert.equal(m.padAt(m.tEnd).inSpec, 0);
  sweep(m, 200);
});
