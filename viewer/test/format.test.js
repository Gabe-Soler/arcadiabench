import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  solClock,
  nightFactor,
  fmtDuration,
  fmtPower,
  fmtEnergy,
  describeCommand,
  parseTimeParam,
  formatMetric,
  DEFAULT_SOL_S,
} from '../src/format.js';

const SOL = DEFAULT_SOL_S;

test('the sol clock splits a sol into 24 local hours from midnight', () => {
  assert.equal(solClock(0, SOL).text, 'Sol 0 · 00:00');
  const sunrise = solClock(SOL * 0.25, SOL);
  assert.equal(sunrise.text, 'Sol 0 · 06:00');
  assert.equal(sunrise.daylight, true);
  assert.equal(solClock(SOL * 0.75, SOL).daylight, false, 'sunset ends daylight');
  const c = solClock(SOL * 1.5 + 1, SOL);
  assert.equal(c.sol, 1);
  assert.equal(c.hh, 12);
  assert.ok(Math.abs(c.elapsedSols - 1.5) < 1e-4);
  assert.equal(solClock(100, 0).sol, 0, 'a missing sol length falls back to the Mars sol');
});

test('night factor is 0 by day, 1 at night, with twilight ramps at 25% and 75%', () => {
  assert.equal(nightFactor(0.5), 0);
  assert.equal(nightFactor(0.0), 1);
  assert.equal(nightFactor(0.9), 1);
  assert.equal(nightFactor(0.25), 0.5);
  assert.equal(nightFactor(0.75), 0.5);
  assert.ok(nightFactor(0.24) > 0.5 && nightFactor(0.26) < 0.5);
});

test('durations, power and energy read naturally', () => {
  assert.equal(fmtDuration(4.25), '4.3 s');
  assert.equal(fmtDuration(45), '45 s');
  assert.equal(fmtDuration(760), '12m 40s');
  assert.equal(fmtDuration(3 * 3600 + 5 * 60), '3h 05m');
  assert.equal(fmtDuration(2.5 * SOL), '2.50 sols');
  assert.equal(fmtDuration(-90), '−1m 30s');
  assert.equal(fmtDuration(null), '–');
  assert.equal(fmtPower(850), '850 W');
  assert.equal(fmtPower(4240), '4.24 kW');
  assert.equal(fmtPower(20000), '20.0 kW');
  assert.equal(fmtEnergy(100000), '100.0 kWh');
});

test('commands are described in one line', () => {
  assert.equal(describeCommand({ type: 'haul', from: { x: 45, y: 30 }, to: { x: 20, y: 20 }, mass_kg: 400 }), 'haul 400 kg (45, 30) → (20, 20)');
  assert.equal(describeCommand({ type: 'scan', area: { min: { x: 1, y: 2 }, max: { x: 3, y: 4 } } }), 'scan (1, 2)–(3, 4)');
  assert.equal(describeCommand({ type: 'service', depot: 'dp-1' }), 'service at dp-1');
  assert.equal(describeCommand({ type: 'charge', source: 'st-1' }), 'charge at st-1');
  assert.equal(describeCommand({ type: 'drill_ice' }), 'drill ice', 'unknown commands fall back to their type');
});

test('the t parameter accepts seconds and units', () => {
  assert.equal(parseTimeParam('90'), 90);
  assert.equal(parseTimeParam('12.5s'), 12.5);
  assert.equal(parseTimeParam('15m'), 900);
  assert.equal(parseTimeParam('2h'), 7200);
  assert.equal(parseTimeParam('1.5sol', 1000), 1500);
  assert.equal(parseTimeParam('soon'), null);
  assert.equal(parseTimeParam('-5'), null);
  assert.equal(parseTimeParam(null), null);
});

test('metrics are formatted by their unit suffix', () => {
  assert.equal(formatMetric('min_storage_wh', 23020), '23.0 kWh');
  assert.equal(formatMetric('min_storage_fraction', 0.3837), '38.4%');
  assert.equal(formatMetric('deficit_s', 0), '0 s');
  assert.equal(formatMetric('max_slope_deg', 4.2), '4.2°');
  assert.equal(formatMetric('rms_error_m', 0.081), '0.081 m');
  assert.equal(formatMetric('verified', false), 'no');
  assert.equal(formatMetric('robots_lost', 1), '1');
  assert.equal(formatMetric('future_metric', 'text'), 'text');
});
