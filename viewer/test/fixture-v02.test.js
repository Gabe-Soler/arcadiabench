// The hand-made protocol 0.2 fixture (fixtures/v0.2-synthetic.jsonl). Its story:
//
// - t=0: a solar-only site with a 60 kWh bank and a base station at (2, 2),
//   range 12 m. sc-1 scans the 4 × 4 pad, ex-1 digs its first row, hl-1
//   drives out to relay at (12, 6).
// - t=600: ex-1's wheels wear to a degraded fault.
// - t=900: the row is cleared; cp-1 drives to (15, 11), out of coverage.
// - t=1000-5000: the Earth link is down (outage).
// - t=2000: cp-1's drive motor fails (disabled, depot only), unseen while out
//   of coverage.
// - t=2500: hl-1 cancels its relay and tows cp-1 home; sc-1's attempt to
//   cannibalize cp-1 is rejected (approval required); an approval request
//   and a notify queue for Earth.
// - t=2700: cp-1, back in coverage, reports its buffered events late.
// - t=2900-4778: ex-1 is serviced at the depot, clearing its fault.
// - t=6920/7650: Earth's approval is sent and arrives; sc-1 cannibalizes cp-1,
//   which is lost at t=8270.
// - t=15000-85000: a dust storm (peak optical depth 3.6 at t=40000).
// - t=45000: a record of an unknown kind; t=90000: run_end with metrics.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parseLog, openBytes } from '../src/parse.js';
import { ModelBuilder } from '../src/model.js';

async function load() {
  const bytes = readFileSync(new URL('./fixtures/v0.2-synthetic.jsonl', import.meta.url));
  const b = new ModelBuilder();
  const result = await parseLog(openBytes(bytes, 'v0.2-synthetic.jsonl'), { onRecord: (r, l) => b.add(r, l) });
  return { m: b.finish(), result };
}

const { m, result } = await load();
const idx = (id) => m.robotIndex.get(id);

test('the fixture parses cleanly and only the unknown record kind is warned about', () => {
  assert.equal(result.badLines, 0);
  assert.equal(m.info.protocol, '0.2.0');
  assert.deepEqual(m.warnings, ['Ignored 1 record of unknown kind "annotation" (written by a newer simulator?).']);
  assert.deepEqual(m.hasV2, { siteTruth: true, faults: true, metrics: true, power: true, baseStation: true });
});

test('site power comes from truth.site and capacity from the scenario', () => {
  assert.equal(m.power.source, 'truth');
  assert.equal(m.power.capacity, 60000);
  assert.equal(m.powerAt(22193.75).storage, 23670);
  assert.equal(m.powerAt(44387.5).solar, 3720);
  assert.equal(m.powerAt(0).fission, 0);
  assert.equal(m.info.power.mode, 'solar_only');
  assert.equal(m.weather.source, 'truth');
  assert.equal(m.weatherAt(40000).tau, 3.6);
});

test('the dust storm spans its weather reports', () => {
  const storms = m.weather.storms.items;
  assert.equal(storms.length, 1);
  assert.equal(storms[0].start, 15000);
  assert.equal(storms[0].end, 85000);
  assert.equal(storms[0].peakTau, 3.6);
});

test('the link outage, its cause and expected restore come from comms events', () => {
  const down = m.commsAt(3000);
  assert.equal(down.link, 'down');
  assert.equal(down.cause, 'outage');
  assert.equal(down.since, 1000);
  assert.equal(down.restore, 5000);
  assert.equal(m.commsAt(5000).link, 'up');
  assert.equal(m.commsAt(6000).delay, 730);
  assert.equal(m.comms.down.total(), 4000);
});

test('faults show on the robots that have them, and service clears them', () => {
  const ex = m.robotAt(idx('ex-1'), 700);
  assert.equal(ex.faults.length, 1);
  assert.equal(ex.faults[0].component, 'wheels');
  assert.equal(ex.faults[0].severity, 'degraded');
  assert.equal(m.robotAt(idx('ex-1'), 4778).faults.length, 0);
  const cp = m.robotAt(idx('cp-1'), 2100);
  assert.equal(cp.status, 'failed');
  assert.equal(cp.faults[0].field_repairable, false);
  const markers = m.markers.filter((k) => k.type === 'fault').map((k) => [k.t, k.robot, k.sev]);
  assert.deepEqual(markers, [
    [600, 'ex-1', 'degraded'],
    [2000, 'cp-1', 'disabled'],
  ]);
});

test('out-of-coverage robots carry stale telemetry; their events arrive late', () => {
  const cp = m.robotAt(idx('cp-1'), 1500);
  assert.equal(cp.inCoverage, false);
  assert.equal(cp.telemetryT, 950);
  const late = m.feed.items.filter((i) => i.late > 0);
  assert.deepEqual(
    late.map((i) => [i.t, i.et, i.text.split(':')[0]]),
    [
      [2700, 975, 'completed c-4'],
      [2700, 2000, 'fault'],
    ],
  );
  assert.equal(m.robotAt(idx('cp-1'), 2900).inCoverage, true);
});

test('a towed robot rides along with its tower', () => {
  for (const t of [2650, 2750, 2850]) {
    const cp = m.robotAt(idx('cp-1'), t);
    const hl = m.robotAt(idx('hl-1'), t);
    assert.equal(cp.status, 'being_towed');
    assert.ok(Math.hypot(cp.x - hl.x, cp.y - hl.y) < 1.6, `t=${t}: ${cp.x},${cp.y} vs ${hl.x},${hl.y}`);
  }
  const before = m.robotAt(idx('cp-1'), 2400);
  assert.deepEqual([before.x, before.y], [15, 11], 'stays put until picked up');
});

test('the relay holds coverage until it is cancelled', () => {
  assert.deepEqual(m.activeRelays(100).map((r) => [r.robot, r.x, r.y, r.rangeM]), [['hl-1', 12, 6, 4]]);
  assert.deepEqual(m.activeRelays(2600), []);
  const relay = m.robots[idx('hl-1')].commands.find((c) => c.id === 'c-3');
  assert.equal(relay.outcome, 'failed');
  assert.equal(relay.reason, 'cancelled');
  assert.equal(m.stats.cancelled, 1);
});

test('the rejection, the approval round trip and the lost robot are all visible', () => {
  const rejected = m.feed.items.find((i) => i.text.startsWith('rejected c-7'));
  assert.match(rejected.text, /approval required/);
  const flight = m.inFlight(3000);
  assert.equal(flight.length, 2);
  assert.ok(flight.every((q) => q.dir === 'down' && q.queued), 'queued while the link is down');
  const reply = m.inFlight(7300);
  assert.equal(reply.length, 1);
  assert.equal(reply[0].dir, 'up');
  assert.ok(reply[0].frac > 0 && reply[0].frac < 1);
  assert.equal(m.robotAt(idx('cp-1'), 8270).status, 'lost');
  assert.equal(m.robotAt(idx('cp-1'), 8270).lost, true);
  assert.ok(m.markers.some((k) => k.type === 'lost' && k.robot === 'cp-1' && k.t === 8270));
  const service = m.robots[idx('ex-1')].commands.find((c) => c.id === 'c-8');
  assert.equal(service.cmd.type, 'service');
  const atDepot = m.robotAt(idx('ex-1'), 4000);
  assert.deepEqual([atDepot.x, atDepot.y], [1, 10], 'serviced at the depot');
});

test('Earth messages in the feed say when they arrive', () => {
  const sent = m.feed.items.filter((i) => i.cat === 'earth' && i.text.startsWith('sent to Earth'));
  assert.deepEqual(
    sent.map((i) => [i.t, i.note]),
    [
      [2500, 'arrives in 53m 40s'],
      [2500, 'arrives in 53m 40s'],
      [50000, 'arrives in 12m 30s'],
    ],
  );
  const reply = m.feed.items.find((i) => i.text.startsWith('Earth sent'));
  assert.equal(reply.t, 6920);
  assert.match(reply.text, /approve a-1/);
});

test('pad progress, base station and run metrics', () => {
  assert.deepEqual(m.padAt(60), { inSpec: 0, verified: 16, berm: 0, total: 16, bermTotal: 4 });
  assert.equal(m.padAt(900).verified, 12, 'digging row 1 unverifies its four cells');
  assert.deepEqual(m.baseStation, { x: 2, y: 2, rangeM: 12 });
  const metrics = m.info.metrics;
  assert.equal(metrics.fleet.robots_lost, 1);
  assert.equal(metrics.conditions.outage_s, 4000);
  assert.equal(metrics.energy.min_storage_fraction, 0.3837);
  assert.equal(m.info.endReason, 'time_limit');
  assert.equal(m.tEnd, 90000);
});
