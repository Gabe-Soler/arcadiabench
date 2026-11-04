// The committed sample logs (viewer/samples/*.jsonl.gz) load through the
// same gzip path as the browser and replay sensibly from start to end: three
// real protocol 0.2 runs and one protocol 0.1 run.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { parseLog, openBytes } from '../src/parse.js';
import { ModelBuilder } from '../src/model.js';
import { scorecard } from '../src/scorecard.js';
import { padStage } from '../src/site.js';
import { sweep } from './sweep.js';

const SAMPLES = new URL('../samples/', import.meta.url);

async function load(name) {
  const bytes = readFileSync(new URL(name, SAMPLES));
  const b = new ModelBuilder();
  const result = await parseLog(openBytes(bytes, name), { onRecord: (r, l) => b.add(r, l) });
  return { m: b.finish(), result };
}

const near = (r, x, y, tol = 1e-6) => Math.abs(r.x - x) <= tol && Math.abs(r.y - y) <= tol;

test('samples are commit-sized: at most 1.5 MB each and 3.5 MB together', () => {
  const files = readdirSync(SAMPLES).filter((f) => f.endsWith('.jsonl.gz'));
  assert.equal(files.length, 4);
  let total = 0;
  for (const f of files) {
    const size = statSync(new URL(f, SAMPLES)).size;
    assert.ok(size <= 1.5e6, `${f} is ${size} bytes`);
    total += size;
  }
  assert.ok(total <= 3.5e6, `${total} bytes in all`);
});

test('ArcadiaOS, conjunction, seed 13: a fault in the blackout, a tow and depot service', async () => {
  const { m, result } = await load('arcadiaos-conjunction-seed13.jsonl.gz');
  assert.equal(result.gzip, true);
  assert.equal(result.badLines, 0);
  assert.deepEqual(m.warnings, []);
  assert.equal(m.info.protocol, '0.2.0');
  assert.equal(m.info.coordinator.name, 'arcadiaos');
  assert.equal(m.info.endReason, 'goals_complete');
  assert.deepEqual(m.hasV2, { siteTruth: true, faults: true, metrics: true, power: true, baseStation: true });
  assert.deepEqual(m.baseStation, { x: 30, y: 8, rangeM: 33 });

  // The final instant is in the log, so nothing is inferred and nothing is counted twice.
  assert.equal(m.finalInstant, true);
  assert.equal(m.decisionCount, m.info.metrics.decisions);
  assert.ok(m.commandEntries.every((c) => !c.assumed));
  assert.ok(!m.scans.some((s) => s.synthetic));
  assert.deepEqual(m.padAt(m.tEnd), { inSpec: 400, verified: 400, berm: 12, total: 400, bermTotal: 12 });
  assert.equal(m.info.metrics.quality.cells_in_spec, 400);

  // The conjunction blackout starts mid-run and lasts beyond its end.
  const down = m.commsAt(70000);
  assert.equal(down.link, 'down');
  assert.equal(down.cause, 'conjunction');
  assert.ok(Math.abs(down.since - 66534.144) < 1e-6);
  assert.ok(down.restore > m.tEnd);
  assert.equal(m.commsAt(60000).link, 'up');

  // cp-1's battery fails during the blackout; hl-1 tows it to the depot.
  const fault = m.markers.find((k) => k.type === 'fault');
  assert.equal(fault.robot, 'cp-1');
  assert.equal(fault.sev, 'disabled');
  const cp = m.robotIndex.get('cp-1');
  const hl = m.robotIndex.get('hl-1');
  const waiting = m.robotAt(cp, 68300);
  assert.ok(near(waiting, 37, 39), 'stays where it failed until the tow arrives');
  assert.equal(waiting.status, 'failed');
  assert.equal(waiting.faults[0].component, 'battery');
  const riding = m.robotAt(cp, 68550);
  const tower = m.robotAt(hl, 68550);
  assert.equal(riding.status, 'being_towed');
  assert.ok(Math.hypot(riding.x - tower.x, riding.y - tower.y) < 1.6);
  assert.equal(tower.command.cmd.type, 'tow');
  const serviced = m.robotAt(cp, 72000);
  assert.ok(near(serviced, 8, 14));
  assert.equal(serviced.status, 'failed', 'disabled until the service finishes');
  assert.equal(serviced.command.cmd.type, 'service');
  assert.equal(m.robotAt(cp, 76000).faults.length, 0);

  // The notifies wait at Mars behind the dark link.
  const queued = m.inFlight(68500).filter((q) => q.dir === 'down' && q.queued);
  assert.equal(queued.length, 3);
  const notify = m.feed.items.find((i) => i.text.startsWith('sent to Earth: notify (tow)'));
  assert.match(notify.note, /arrives in 1[34]\.\d+ sols/);

  assert.deepEqual(
    scorecard(m.info.metrics, m.info.solLength).map((g) => g.id),
    ['completion', 'quality', 'energy', 'fleet', 'autonomy', 'conditions'],
  );
  const counts = new Array(8).fill(0);
  for (const idx of m.pad.progress.cells) counts[m.site.cellAt(idx, 40000).surf]++;
  assert.equal(padStage(m.padAt(40000), counts, { t: 40000 }).key, 'in_progress');
  sweep(m);
});

test('ArcadiaOS, cascading faults, seed 2: the towed robot shows as being towed', async () => {
  const { m } = await load('arcadiaos-cascading-faults-seed2.jsonl.gz');
  assert.deepEqual(m.warnings, []);
  assert.equal(m.finalInstant, true);
  assert.equal(m.decisionCount, m.info.metrics.decisions);
  const ex = m.robotIndex.get('ex-1');
  const hl = m.robotIndex.get('hl-1');
  assert.equal(m.robotAt(ex, 15300).status, 'being_towed');
  const a = m.robotAt(ex, 15300);
  const b = m.robotAt(hl, 15300);
  assert.ok(Math.hypot(a.x - b.x, a.y - b.y) < 1.6);
  const types = new Set(m.commandEntries.map((c) => c.cmd.type));
  for (const t of ['tow', 'service', 'repair']) assert.ok(types.has(t), t);
  assert.equal(m.info.metrics.fleet.faults, 1);
  sweep(m, 200);
});

test('null coordinator, conjunction, seed 7: a 14-sol blackout and a policy update', async () => {
  const { m } = await load('null-conjunction-seed7.jsonl.gz');
  assert.deepEqual(m.warnings, []);
  assert.equal(m.info.endReason, 'fleet_lost');
  assert.equal(m.stats.commands, 0);
  const blackout = m.comms.down.items.find((d) => d.cause === 'conjunction');
  assert.ok((blackout.end - blackout.start) / m.info.solLength > 13.9);
  assert.ok(m.comms.down.items.some((d) => d.cause === 'outage'));
  const policy = m.feed.items.find((i) => i.text.startsWith('Earth sent: policy update'));
  assert.equal(policy.text, 'Earth sent: policy update v2: reassign task 2→1, replan 2→1, tow 2→1');
  const arrived = m.feed.items.find((i) => i.text.startsWith('received from Earth: policy update'));
  assert.ok(arrived.t > policy.t);
  const offline = m.robots.filter((_, i) => m.robotAt(i, m.tEnd).status === 'offline').length;
  assert.equal(offline, 4);
  assert.equal(padStage(m.padAt(m.tEnd), [400], { t: m.tEnd }).key, 'not_started');
  sweep(m, 200);
});

test('the scripted builder sample (protocol 0.1) still builds and verifies the pad', async () => {
  const { m, result } = await load('builder-seed34.jsonl.gz');
  assert.equal(result.gzip, true);
  assert.deepEqual(m.warnings, []);
  assert.equal(m.info.protocol, '0.1.0');
  assert.equal(m.finalInstant, false, 'older logs end at the last decision');
  assert.equal(m.decisionCount, 241);
  assert.deepEqual(m.padAt(m.t0), { inSpec: 0, verified: 0, berm: 0, total: 400, bermTotal: 12 });
  assert.deepEqual(m.padAt(m.tEnd), { inSpec: 400, verified: 400, berm: 12, total: 400, bermTotal: 12 });
  assert.ok(m.scans.some((s) => s.synthetic), 'the closing scan is inferred');
  assert.equal(m.weather.storms.items.length, 1);

  // The hauler's trips to the stockpile happen between snapshots.
  const hl = m.robotIndex.get('hl-1');
  const haul = m.robots[hl].commands.find((c) => c.cmd.type === 'haul');
  let atStockpile = false;
  for (let t = haul.start; t <= haul.end; t += 2) if (near(m.robotAt(hl, t), haul.cmd.from.x, haul.cmd.from.y)) atStockpile = true;
  assert.ok(atStockpile);
  sweep(m);
});
