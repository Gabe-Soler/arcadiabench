import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scorecard, scorecardHeadline } from '../src/scorecard.js';

const metrics = {
  completed: true,
  sols: 0.8798,
  decisions: 1889,
  commands: 1863,
  commands_rejected: 2,
  quality: { cells_total: 400, cells_in_spec: 398, rms_error_m: 0.0113, max_abs_error_m: 0.0234, max_slope_deg: 1.29, berm_complete: true, verified: true },
  energy: { robot_energy_wh: 36391.9, grid_energy_wh: 26841.4, min_storage_wh: 9000, min_storage_fraction: 0.09, deficit_s: 600 },
  fleet: { faults: 1, robot_hours_lost: 2.07, robots_lost: 0 },
  autonomy: { downlink_messages: 5, approval_requests: 0, notifications: 4, status_reports: 1, approval_wait_s: 0, policy_violations: 1 },
  conditions: { storm_s: 0, blackout_s: 41588.4, outage_s: 0 },
};

test('metrics group into the scorecard in its order', () => {
  const groups = scorecard(metrics, 88775);
  assert.deepEqual(
    groups.map((g) => [g.id, g.title]),
    [
      ['completion', 'Completion'],
      ['quality', 'Quality'],
      ['energy', 'Energy'],
      ['fleet', 'Fleet health'],
      ['autonomy', 'Autonomy'],
      ['conditions', 'Conditions'],
    ],
  );
  const row = (g, key) => groups.find((x) => x.id === g).rows.find((r) => r.key === key);
  assert.deepEqual([row('completion', 'completed').text, row('completion', 'completed').tone], ['yes', 'good']);
  assert.equal(row('completion', 'sols').text, '0.88');
  assert.equal(row('completion', 'commands_rejected').tone, 'warn');
  assert.deepEqual([row('quality', 'cells_in_spec').text, row('quality', 'cells_in_spec').tone], ['398 / 400', 'bad']);
  assert.equal(row('quality', 'rms_error_m').text, '1.1 cm');
  assert.equal(row('quality', 'cells_total'), undefined, 'folded into cells in spec');
  assert.deepEqual([row('energy', 'min_storage_wh').text, row('energy', 'min_storage_wh').tone], ['9%', 'warn']);
  assert.match(row('energy', 'min_storage_wh').hint, /9\.00 kWh/);
  assert.equal(row('energy', 'deficit_s').text, '10m 00s');
  assert.equal(row('fleet', 'robots_lost').tone, 'good');
  assert.equal(row('autonomy', 'policy_violations').tone, 'bad');
  assert.equal(row('conditions', 'blackout_s').text, '11h 33m');
  for (const g of groups) for (const r of g.rows) assert.ok(r.label.length <= 14, `${r.label} is short enough for a card`);
});

test('unknown metrics from newer simulators are kept', () => {
  const groups = scorecard({ ...metrics, crew_size: 20, fleet: { ...metrics.fleet, spare_parts: 3 }, earth: { approvals_vetoed: 2 } }, 88775);
  const completion = groups.find((g) => g.id === 'completion');
  assert.ok(completion.rows.some((r) => r.key === 'crew_size' && r.text === '20'));
  assert.ok(groups.find((g) => g.id === 'fleet').rows.some((r) => r.key === 'spare_parts'));
  const other = groups.find((g) => g.id === 'other');
  assert.deepEqual(other.rows.map((r) => [r.key, r.text]), [['earth.approvals_vetoed', '2']]);
  assert.deepEqual(scorecard(null, 88775), []);
});

test('the headline sums up the run', () => {
  assert.equal(scorecardHeadline(metrics), 'Pad finished, in 0.88 sols, 398/400 cells in spec');
  assert.equal(scorecardHeadline({ completed: false, sols: 17.35, quality: { cells_in_spec: 0, cells_total: 400 } }), 'Pad not finished, after 17.35 sols, 0/400 cells in spec');
  assert.equal(scorecardHeadline(undefined), null);
});
