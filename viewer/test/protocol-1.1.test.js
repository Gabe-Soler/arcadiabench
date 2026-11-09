// What protocol 1.1 adds, on a small log built inline: a link that runs in
// pass windows, a mobile charger's tank, and a run that starts mid-sol.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { modelFromRecords } from '../src/model.js';

const SOL = 88775;
const START = 100;

function runStart() {
  return {
    t: START,
    kind: 'run_start',
    protocol_version: '1.1.0',
    simulator: { name: 'arcadiabench', version: '1.1.0' },
    coordinator: { name: 'test', version: '0' },
    run_id: 'r-1',
    seed: 3,
    scenario: {
      scenario_id: 'mini',
      start_t: START,
      time_limit_t: START + 2 * SOL,
      sol_length_s: SOL,
      site: { site_id: 's', width: 8, height: 6, cell_size_m: 1, cells: [] },
      robots: [
        {
          robot_id: 'hl-1',
          model: 'hauler',
          mass_kg: 300,
          battery_capacity_wh: 12000,
          max_speed_m_s: 1,
          drive_energy_wh_per_m: 1,
          idle_power_w: 1,
          capabilities: [
            { name: 'haul', rate: 3, energy_wh_per_unit: 0, capacity: 400 },
            { name: 'charge', rate: 0.5, energy_wh_per_unit: 0, capacity: 8000 },
          ],
        },
        { robot_id: 'sc-1', mass_kg: 90, battery_capacity_wh: 500, max_speed_m_s: 1, drive_energy_wh_per_m: 1, idle_power_w: 1, capabilities: [{ name: 'scan', rate: 4, energy_wh_per_unit: 0 }] },
      ],
      stations: [],
      depots: [],
      power: { mode: 'hybrid', fission_w: 10000, solar_peak_w: 20000, storage_capacity_wh: 100000, essential_load_w: 2000 },
      base_station: { position: { x: 0, y: 0 }, range_m: 33 },
      policy: { policy_version: 1, default_level: 2, levels: {}, level3: { max_wait_s: 3600, default_fallback: 'abort' } },
      constants: { grade_window_m: 0.15, lost_link_timeout_s: 1800 },
    },
    true_site: { site_id: 's', width: 8, height: 6, cell_size_m: 1, cells: [] },
  };
}

const health = { wheel_slip: 0.02, motor_current_ratio: 1, dust_coverage: 0, temperature_c: -60 };
const robots = (tank) => [
  { robot_id: 'hl-1', position: { x: 1, y: 1 }, status: 'idle', battery_wh: 11000, payload_kg: 0, tank_wh: tank, health, faults: [], in_relay_coverage: true, telemetry_t: 0 },
  { robot_id: 'sc-1', position: { x: 5, y: 5 }, status: 'idle', battery_wh: 400, payload_kg: 0, health, faults: [], in_relay_coverage: true, telemetry_t: 0 },
];
const site = (link) => ({ storage_wh: 60000, solar_w: 0, fission_w: 10000, demand_w: 2000, optical_depth: 0.5, temperature_c: -60, link, one_way_delay_s: 760 });
const truth = (t, link, tank = 8000) => ({ t, kind: 'truth', robots: robots(tank), changed_cells: [], site: site(link) });
const comms = (t, fields) => ({ t, kind: 'observed', event: { t, type: 'comms', one_way_delay_s: 760, ...fields } });

function log() {
  return [
    runStart(),
    comms(100, { link: 'up', cause: 'nominal', window_end_t: 400 }),
    truth(100, 'up'),
    comms(400, { link: 'down', cause: 'between_passes', expected_restore_t: 900 }),
    truth(400, 'down'),
    // An outage outranks the gap, and nobody knows when it ends.
    comms(500, { link: 'down', cause: 'outage' }),
    truth(500, 'down', 6500),
    // It ends in the gap: the link stays down, and its return is known again.
    comms(700, { link: 'down', cause: 'between_passes', expected_restore_t: 900 }),
    truth(700, 'down', 6500),
    comms(900, { link: 'up', cause: 'nominal', window_end_t: 1200 }),
    truth(900, 'up', 6500),
    comms(1200, { link: 'down', cause: 'between_passes', expected_restore_t: 5000 }),
    truth(1200, 'down', 6500),
    truth(2000, 'down', 6500),
    { t: 2000, kind: 'run_end', reason: 'time_limit' },
  ];
}

test('a protocol 1.1 log loads without warnings, from a start that is not t = 0', () => {
  const m = modelFromRecords(log());
  assert.deepEqual(m.warnings, []);
  assert.equal(m.info.protocol, '1.1.0');
  assert.equal(m.t0, START);
  assert.equal(m.tEnd, 2000);
  // The time limit counts from the start of the run.
  assert.equal((m.info.timeLimitT - m.t0) / m.info.solLength, 2);
});

test('a stretch of link down time keeps each of its causes', () => {
  const m = modelFromRecords(log());
  const parts = m.comms.down.items.map((d) => [d.start, d.open ? null : d.end, d.cause]);
  assert.deepEqual(parts, [
    [400, 500, 'between_passes'],
    [500, 700, 'outage'],
    [700, 900, 'between_passes'],
    [1200, null, 'between_passes'],
  ]);
  // Every part knows when the whole stretch began and when the link came back.
  for (const t of [450, 600, 750]) {
    const c = m.commsAt(t);
    assert.equal(c.link, 'down', `t ${t}`);
    assert.equal(c.since, 400, `t ${t}`);
    assert.equal(c.until, 900, `t ${t}`);
  }
  assert.equal(m.commsAt(450).cause, 'between_passes');
  assert.equal(m.commsAt(450).restore, 900);
  // During the outage the return is unknown, as it was at the time.
  assert.equal(m.commsAt(600).cause, 'outage');
  assert.equal(m.commsAt(600).restore, null);
  assert.equal(m.commsAt(750).cause, 'between_passes');
  assert.equal(m.commsAt(750).restore, 900);
  // Still down at the end of the log.
  const last = m.commsAt(1500);
  assert.equal(last.cause, 'between_passes');
  assert.equal(last.restore, 5000);
  assert.ok(last.until > m.tEnd);
  assert.equal(Math.round(m.comms.down.total(m.t0, m.tEnd)), 500 + 800);
});

test('a pass window reports when it closes', () => {
  const m = modelFromRecords(log());
  assert.equal(m.commsAt(300).link, 'up');
  assert.equal(m.commsAt(300).windowEnd, 400);
  assert.equal(m.commsAt(1000).windowEnd, 1200);
  // Down: no window. And a log without windows never has one.
  assert.equal(m.commsAt(600).windowEnd, null);
  const plain = log().map((r) => (r.kind === 'observed' ? { ...r, event: { ...r.event, window_end_t: undefined } } : r));
  assert.equal(modelFromRecords(plain).commsAt(300).windowEnd, null);
});

test('a mobile charger shows its tank, and other robots have none', () => {
  const m = modelFromRecords(log());
  const hl = m.robotIndex.get('hl-1');
  const sc = m.robotIndex.get('sc-1');
  assert.equal(m.robots[hl].spec.tankWh, 8000);
  assert.equal(m.robots[sc].spec.tankWh, null);
  assert.equal(m.robotAt(hl, 200).tank, 8000);
  assert.equal(m.robotAt(hl, 600).tank, 6500, 'after it charged another robot');
  assert.equal(m.robotAt(sc, 600).tank, null);
});
