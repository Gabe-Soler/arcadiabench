// The model on small protocol 0.1 logs built inline: no truth.site, no
// faults, no metrics, the shape the week-2 simulator writes.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { modelFromRecords, ModelBuilder } from '../src/model.js';
import { LogError } from '../src/parse.js';

const SOL = 88775;
const W = 8;
const H = 6;

function runStart(extra = {}) {
  const cells = [];
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) cells.push({ at: { x, y }, cell: { elevation_m: -4102 + 0.01 * x, surface: 'natural', boulders: [] } });
  return {
    t: 0,
    kind: 'run_start',
    protocol_version: '0.1.0',
    simulator: { name: 'arcadiabench', version: '0.1.0' },
    coordinator: { name: 'test', version: '0' },
    run_id: 'r-1',
    seed: 3,
    scenario: {
      scenario_id: 'mini',
      start_t: 0,
      time_limit_t: SOL,
      sol_length_s: SOL,
      site: { site_id: 's', width: W, height: H, cell_size_m: 1, cells: [] },
      robots: [
        { robot_id: 'hl-1', model: 'hauler', mass_kg: 300, battery_capacity_wh: 1000, max_speed_m_s: 1, drive_energy_wh_per_m: 1, idle_power_w: 1, capabilities: [{ name: 'haul', rate: 3, energy_wh_per_unit: 0, capacity: 400 }] },
        { robot_id: 'sc-1', mass_kg: 90, battery_capacity_wh: 500, max_speed_m_s: 1, drive_energy_wh_per_m: 1, idle_power_w: 1, capabilities: [{ name: 'scan', rate: 4, energy_wh_per_unit: 0 }] },
      ],
      stations: [{ station_id: 'st-1', position: { x: 0, y: 0 }, max_power_w: 6000, slots: 2 }],
      depots: [{ depot_id: 'dp-1', position: { x: 0, y: 5 } }],
      policy: { policy_version: 1, default_level: 2, levels: {}, level3: { max_wait_s: 3600, default_fallback: 'abort' } },
    },
    true_site: { site_id: 's', width: W, height: H, cell_size_m: 1, cells },
    ...extra,
  };
}

const robot = (id, x, y, status = 'idle', battery = 900, payload = 0, cmd) => ({
  robot_id: id,
  position: { x, y },
  status,
  battery_wh: battery,
  payload_kg: payload,
  ...(cmd ? { current_command: cmd } : {}),
  health: { wheel_slip: 0.02, motor_current_ratio: 1, dust_coverage: 0, temperature_c: -60 },
  in_relay_coverage: true,
});
const truth = (t, robots, changed = []) => ({ t, kind: 'truth', robots, changed_cells: changed });
const ev = (t, event) => ({ t, kind: 'observed', event: { t, ...event } });
const goal = {
  type: 'goal',
  message_id: 'earth-0001',
  goal_id: 'pad-a',
  priority: 10,
  objective: { kind: 'landing_pad', pad: { area: { min: { x: 2, y: 2 }, max: { x: 3, y: 2 } }, target_elevation_m: -4102, surface_tolerance_m: 0.05, max_slope_deg: 2, stabilize: false } },
};

function log({ end = 'time_limit', endT = 1000, extra = [] } = {}) {
  const recs = [
    runStart(),
    ev(0, { type: 'comms', link: 'up', cause: 'nominal', one_way_delay_s: 760 }),
    ev(0, { type: 'weather', condition: 'clear', optical_depth: 0.5, temperature_c: -70 }),
    ev(0, { type: 'power', solar_w: 0, fission_w: 10000, demand_w: 2000, storage_wh: 60000, storage_capacity_wh: 100000, daylight: false }),
    ev(0, { type: 'earth_message', sent_t: 0, message: goal }),
    truth(0, [robot('hl-1', 1, 1), robot('sc-1', 5, 5)]),
    { t: 0, kind: 'commanded', command: { command_id: 'c-1', robot_id: 'hl-1', type: 'haul', from: { x: 7, y: 0 }, to: { x: 2, y: 2 }, mass_kg: 400 } },
    ev(0, { type: 'command_accepted', command_id: 'c-1', robot_id: 'hl-1', estimated_duration_s: 300 }),
    { t: 0, kind: 'commanded', command: { command_id: 'c-2', robot_id: 'sc-1', type: 'scan', area: { min: { x: 9, y: 9 }, max: { x: 9, y: 9 } } } },
    ev(0, { type: 'command_rejected', command_id: 'c-2', robot_id: 'sc-1', reason: 'invalid_target', detail: 'area is empty or off the site' }),
    ev(200, { type: 'weather', condition: 'dust_storm', optical_depth: 0.5, temperature_c: -60 }),
    ev(300, { type: 'command_completed', command_id: 'c-1', robot_id: 'hl-1', energy_used_wh: 20 }),
    truth(300, [robot('hl-1', 2, 2, 'idle', 880), robot('sc-1', 5, 5)], [
      { at: { x: 7, y: 0 }, cell: { elevation_m: -4102.3, surface: 'cleared', boulders: [] } },
      { at: { x: 2, y: 2 }, cell: { elevation_m: -4102.0, surface: 'compacted', boulders: [] } },
    ]),
    ev(600, { type: 'weather', condition: 'dust_storm', optical_depth: 3.0, temperature_c: -40 }),
    ev(600, { type: 'power', solar_w: 5000, fission_w: 10000, demand_w: 2000, storage_wh: 100000, storage_capacity_wh: 100000, daylight: true }),
    truth(600, [robot('hl-1', 2, 2, 'idle', 870), robot('sc-1', 5, 5)]),
    ...extra,
  ];
  if (end) recs.push({ t: endT, kind: 'run_end', reason: end });
  return recs;
}

test('a 0.1 log builds without warnings and reports its run', () => {
  const m = modelFromRecords(log());
  assert.deepEqual(m.warnings, []);
  assert.equal(m.info.scenarioId, 'mini');
  assert.equal(m.info.seed, 3);
  assert.equal(m.info.endReason, 'time_limit');
  assert.equal(m.tEnd, 1000);
  assert.equal(m.robots.length, 2);
  assert.equal(m.hasV2.siteTruth, false);
  assert.equal(m.hasV2.metrics, false);
  assert.equal(m.info.metrics, null);
  assert.deepEqual(m.decisionTimes, [0, 300, 600]);
  assert.equal(m.prevDecision(300), 0);
  assert.equal(m.nextDecision(300), 600);
});

test('commands get outcomes and the hauler visits the source cell between snapshots', () => {
  const m = modelFromRecords(log());
  assert.equal(m.stats.commands, 2);
  assert.equal(m.stats.completed, 1);
  assert.equal(m.stats.rejected, 1);
  const i = m.robotIndex.get('hl-1');
  const at = (t) => m.robotAt(i, t);
  assert.equal(at(0).command.id, 'c-1');
  assert.equal(at(300).command, null);
  let reached = false;
  for (let t = 0; t <= 300; t += 2) if (Math.abs(at(t).x - 7) < 1e-6 && Math.abs(at(t).y) < 1e-6) reached = true;
  assert.ok(reached);
  assert.ok(at(150).status === 'moving' || at(150).status === 'working');
  assert.equal(at(450).status, 'idle');
  assert.ok(Math.abs(at(450).battery - 875) < 1e-9, 'battery is interpolated between snapshots');
  assert.equal(at(450).batteryFrac, 0.875);
});

test('cell changes land when the robot finishes them, inside the snapshot interval', () => {
  const m = modelFromRecords(log());
  const src = m.site.index({ x: 7, y: 0 });
  const dst = m.site.index({ x: 2, y: 2 });
  const tSrc = m.site.timelines.get(src).t[0];
  const tDst = m.site.timelines.get(dst).t[0];
  assert.ok(tSrc > 0 && tSrc < tDst && tDst <= 300, `${tSrc} ${tDst}`);
  assert.equal(m.padAt(tDst - 1e-3).inSpec, 0);
  assert.equal(m.padAt(tDst).inSpec, 1);
  assert.equal(m.padAt(1000).total, 2);
});

test('power, weather and comms come from events when there is no truth.site', () => {
  const m = modelFromRecords(log());
  const p = m.powerAt(300);
  assert.equal(m.power.source, 'events');
  assert.equal(p.capacity, 100000);
  assert.equal(p.storage, 80000);
  assert.equal(p.solar, 2500);
  const storm = m.weatherAt(400).storm;
  assert.equal(storm.start, 200);
  assert.equal(storm.open, true, 'still raging at the end');
  assert.equal(storm.peakTau, 3.0);
  assert.equal(m.weatherAt(400).tau, 0.5 + (2.5 * 200) / 400);
  assert.equal(m.weatherAt(1000).storm.start, 200);
  assert.equal(m.weatherAt(100).storm, null);
  assert.equal(m.weatherAt(300).temp, -60, 'robot health temperature fills in between weather reports');
  const c = m.commsAt(500);
  assert.equal(c.link, 'up');
  assert.equal(c.delay, 760);
  assert.equal(m.comms.down.items.length, 0);
});

test('the feed lists commands and outcomes in log order, skipping acceptances', () => {
  const m = modelFromRecords(log());
  const kinds = m.feed.items.map((i) => `${i.cat}:${i.text.split(' ')[0]}`);
  assert.deepEqual(kinds, [
    'site:Earth',
    'site:clear',
    'earth:received',
    'command:haul',
    'command:scan',
    'result:rejected',
    'site:dust',
    'result:completed',
    'site:dust',
    'site:run',
  ]);
  assert.equal(m.feedIndexAt(0), 5);
  assert.equal(m.feedIndexAt(-1), -1);
});

test('a missing run_end, unknown kinds, newer protocols and disorder are warned about, not fatal', () => {
  const recs = log({ end: null, extra: [{ t: 700, kind: 'hologram', data: 1 }, { t: 650, kind: 'truth', robots: [], changed_cells: [] }] });
  recs[0] = runStart({ protocol_version: '0.9.1' });
  const m = modelFromRecords(recs);
  const text = m.warnings.join('\n');
  assert.match(text, /no run_end/);
  assert.match(text, /unknown kind "hologram"/);
  assert.match(text, /protocol 0\.9\.1/);
  assert.match(text, /out of time order/);
  assert.equal(m.tEnd, 700);
});

test('protocol 1.0 and 1.1 logs load without a version notice, and a newer one gets it', () => {
  const recs = log();
  for (const v of ['1.0.0', '1.1.0']) {
    recs[0] = runStart({ protocol_version: v, simulator: { name: 'arcadiabench', version: v } });
    const m = modelFromRecords(recs);
    assert.deepEqual(m.warnings, [], v);
    assert.equal(m.info.protocol, v);
  }
  recs[0] = runStart({ protocol_version: '1.2.0' });
  assert.deepEqual(modelFromRecords(recs).warnings, [
    'This log uses protocol 1.2.0; the viewer was written for 0.1, 0.2, 1.0 and 1.1. Unknown records and fields are ignored.',
  ]);
});

test('commands issued at the last decision point count as running to the end', () => {
  const recs = log({ end: 'goals_complete', endT: 650 });
  recs.splice(recs.length - 1, 0, { t: 600, kind: 'commanded', command: { command_id: 'c-3', robot_id: 'sc-1', type: 'scan', area: goal.objective.pad.area } });
  const m = modelFromRecords(recs);
  const i = m.robotIndex.get('sc-1');
  assert.equal(m.robotAt(i, 620).command.id, 'c-3');
  assert.equal(m.padAt(650).verified, 2, 'goals_complete means the final scan verified the pad');
});

test('runs without run_start or a site map are rejected with a clear error', () => {
  const b = new ModelBuilder();
  b.add({ t: 0, kind: 'truth', robots: [], changed_cells: [] }, 1);
  assert.throws(() => b.finish(), (e) => e instanceof LogError && /no run_start/.test(e.message));
  const noSite = runStart();
  delete noSite.true_site;
  assert.throws(() => modelFromRecords([noSite]), (e) => e instanceof LogError && /true_site/.test(e.message));
});

test('a log with the final instant is taken at its word, and its final truth is not a decision', () => {
  const recs = log({ end: null });
  const area = goal.objective.pad.area;
  recs.push({ t: 600, kind: 'commanded', command: { command_id: 'c-3', robot_id: 'sc-1', type: 'scan', area } });
  // Protocol 0.2: the acceptance rides with the next observation, here the final instant.
  recs.push({ t: 640, kind: 'observed', event: { t: 600, type: 'command_accepted', command_id: 'c-3', robot_id: 'sc-1', estimated_duration_s: 40 } });
  recs.push(ev(640, { type: 'scan_result', command_id: 'c-3', robot_id: 'sc-1', cells: [2, 3].map((x) => ({ at: { x, y: 2 }, cell: { elevation_m: -4102, surface: 'compacted' } })) }));
  recs.push(ev(640, { type: 'command_completed', command_id: 'c-3', robot_id: 'sc-1', energy_used_wh: 1 }));
  recs.push(truth(640, [robot('hl-1', 2, 2, 'idle', 860), robot('sc-1', 3, 2)]));
  recs.push({ t: 640, kind: 'run_end', reason: 'goals_complete' });
  const m = modelFromRecords(recs);
  assert.equal(m.finalInstant, true);
  assert.equal(m.decisionCount, 3, 'truth records at 0, 300 and 600; 640 is the final instant');
  assert.ok(m.commandEntries.every((c) => !c.assumed));
  assert.ok(!m.scans.some((s) => s.synthetic));
  const scan = m.commandEntries.find((c) => c.id === 'c-3');
  assert.deepEqual([scan.start, scan.end, scan.outcome], [600, 640, 'completed']);
  assert.deepEqual(m.padAt(639), { inSpec: 1, verified: 0, berm: 0, total: 2, bermTotal: 0 });
  assert.equal(m.padAt(640).verified, 2, 'the logged scan verifies both pad cells');
  const late = m.feed.items.filter((i) => i.late > 0);
  assert.deepEqual(late, [], 'acceptances that ride along are not reported as late');
});

test('a 20-robot crew replays: every robot moves, works and shows in the feed', () => {
  const rs = runStart();
  const base = rs.scenario.robots[0];
  rs.scenario.robots = Array.from({ length: 20 }, (_, i) => ({ ...base, robot_id: `r-${i + 1}` }));
  const at = (t, k) => rs.scenario.robots.map((spec, i) => robot(spec.robot_id, i % 8, Math.floor(i / 8) + k, k === 1 ? 'moving' : 'idle'));
  const recs = [rs, truth(0, at(0, 0))];
  rs.scenario.robots.forEach((spec, i) => {
    recs.push({ t: 0, kind: 'commanded', command: { command_id: `m-${i}`, robot_id: spec.robot_id, type: 'move_to', target: { x: i % 8, y: Math.floor(i / 8) + 2 } } });
    recs.push(ev(0, { type: 'command_accepted', command_id: `m-${i}`, robot_id: spec.robot_id, estimated_duration_s: 2 }));
  });
  rs.scenario.robots.forEach((spec, i) => recs.push(ev(2, { type: 'command_completed', command_id: `m-${i}`, robot_id: spec.robot_id, energy_used_wh: 1 })));
  recs.push(truth(2, at(2, 2)));
  recs.push({ t: 3, kind: 'run_end', reason: 'time_limit' });
  const m = modelFromRecords(recs);
  assert.deepEqual(m.warnings, []);
  assert.equal(m.robots.length, 20);
  for (let i = 0; i < 20; i++) {
    const mid = m.robotAt(i, 1);
    assert.equal(mid.status, 'moving');
    assert.equal(mid.command.id, `m-${i}`);
    assert.equal(m.robotAt(i, 2.5).y, Math.floor(i / 8) + 2);
  }
  assert.equal(m.feed.items.filter((it) => it.cat === 'command').length, 20);
  assert.equal(new Set(m.feed.items.filter((it) => it.robot).map((it) => it.robot)).size, 20);
});
