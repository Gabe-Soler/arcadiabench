// The run_end metrics laid out as the benchmark's scorecard: completion,
// quality, energy, fleet health, autonomy and conditions. DOM-free.

import { fmtDuration, fmtEnergy, fmtNumber, fmtPct, formatMetric, humanize } from './format.js';

const yesNo = (v) => (v ? 'yes' : 'no');

/**
 * Scorecard groups. Each row names a metric (a key in the group's object, or
 * at the top level for `at: null`), a label, and optionally how to format it
 * and how to judge it ("good", "warn" or "bad").
 */
const GROUPS = [
  {
    id: 'completion',
    title: 'Completion',
    at: null,
    rows: [
      { key: 'completed', label: 'Finished', hint: 'The run ended goals_complete', text: yesNo, tone: (v) => (v ? 'good' : 'bad') },
      { key: 'sols', label: 'Sols', hint: 'Sols from start to end', text: (v) => fmtNumber(v, 2) },
      { key: 'decisions', label: 'Decisions', hint: 'Observations the coordinator answered', text: (v) => fmtNumber(v) },
      { key: 'commands', label: 'Commands', hint: 'Commands sent', text: (v) => fmtNumber(v) },
      { key: 'commands_rejected', label: 'Rejected', hint: 'Commands the simulator rejected', text: (v) => fmtNumber(v), tone: (v) => (v > 0 ? 'warn' : null) },
    ],
  },
  {
    id: 'quality',
    title: 'Quality',
    at: 'quality',
    folds: ['cells_total'],
    rows: [
      {
        key: 'cells_in_spec',
        label: 'Cells in spec', hint: 'Pad cells meeting the spec at the end',
        text: (v, g) => (Number.isFinite(g.cells_total) ? `${fmtNumber(v)} / ${fmtNumber(g.cells_total)}` : fmtNumber(v)),
        tone: (v, g) => (Number.isFinite(g.cells_total) ? (v >= g.cells_total ? 'good' : 'bad') : null),
      },
      { key: 'rms_error_m', label: 'RMS error', hint: 'RMS of elevation minus target over pad cells', text: (v) => `${fmtNumber(v * 100, 1)} cm` },
      { key: 'max_abs_error_m', label: 'Worst error', hint: 'Largest elevation error on the pad', text: (v) => `${fmtNumber(v * 100, 1)} cm` },
      { key: 'max_slope_deg', label: 'Max slope', hint: 'Steepest slope between adjacent pad cells', text: (v) => `${fmtNumber(v, 2)}°` },
      { key: 'berm_complete', label: 'Berm built', hint: 'Every berm cell at full height', text: yesNo, tone: (v) => (v ? 'good' : 'bad') },
      { key: 'verified', label: 'Verified', hint: 'A covering scan finished after the last pad change', text: yesNo, tone: (v) => (v ? 'good' : 'bad') },
    ],
  },
  {
    id: 'energy',
    title: 'Energy',
    at: 'energy',
    folds: ['min_storage_fraction'],
    rows: [
      { key: 'robot_energy_wh', label: 'Robot energy', hint: 'Energy drawn from robot batteries', text: fmtEnergy },
      { key: 'grid_energy_wh', label: 'From stations', hint: 'Energy delivered by charge stations', text: fmtEnergy },
      {
        key: 'min_storage_wh',
        label: 'Min storage',
        hint: (v) => `Lowest site storage over the run: ${fmtEnergy(v)}`,
        text: (v, g) => (Number.isFinite(g.min_storage_fraction) ? fmtPct(g.min_storage_fraction) : fmtEnergy(v)),
        tone: (v, g) => (g.min_storage_fraction <= 0.05 ? 'bad' : g.min_storage_fraction < 0.2 ? 'warn' : null),
      },
      { key: 'deficit_s', label: 'Power deficit', hint: 'Time the site could not deliver full charging power', text: (v, g, sol) => fmtDuration(v, sol), tone: (v) => (v > 0 ? 'warn' : null) },
    ],
  },
  {
    id: 'fleet',
    title: 'Fleet health',
    at: 'fleet',
    rows: [
      { key: 'faults', label: 'Faults', hint: 'Robot faults that occurred', text: (v) => fmtNumber(v), tone: (v) => (v > 0 ? 'warn' : null) },
      { key: 'robot_hours_lost', label: 'Hours lost', hint: 'Robot-hours spent failed, stuck, towed, offline, in service or lost', text: (v) => fmtNumber(v, 1) },
      { key: 'robots_lost', label: 'Robots lost', hint: 'Robots permanently lost', text: (v) => fmtNumber(v), tone: (v) => (v > 0 ? 'bad' : 'good') },
    ],
  },
  {
    id: 'autonomy',
    title: 'Autonomy',
    at: 'autonomy',
    rows: [
      { key: 'downlink_messages', label: 'To Earth', hint: 'Messages sent to Earth', text: (v) => fmtNumber(v) },
      { key: 'notifications', label: 'Notifications', hint: 'Level-2 notifications', text: (v) => fmtNumber(v) },
      { key: 'status_reports', label: 'Status reports', hint: 'Status reports sent', text: (v) => fmtNumber(v) },
      { key: 'approval_requests', label: 'Approvals', hint: 'Approval requests sent', text: (v) => fmtNumber(v) },
      { key: 'approval_wait_s', label: 'Approval wait', hint: 'Total time spent waiting for approvals', text: (v, g, sol) => fmtDuration(v, sol) },
      { key: 'policy_violations', label: 'Violations', hint: 'Authority policy violations', text: (v) => fmtNumber(v), tone: (v) => (v > 0 ? 'bad' : 'good') },
    ],
  },
  {
    id: 'conditions',
    title: 'Conditions',
    at: 'conditions',
    rows: [
      { key: 'storm_s', label: 'Dust storm', hint: 'Time under a dust storm', text: (v, g, sol) => fmtDuration(v, sol) },
      { key: 'blackout_s', label: 'Blackout', hint: 'Time in the conjunction blackout', text: (v, g, sol) => fmtDuration(v, sol) },
      { key: 'outage_s', label: 'Outages', hint: 'Time the link was down for outages', text: (v, g, sol) => fmtDuration(v, sol) },
    ],
  },
];

function isPlainObject(v) {
  return v != null && typeof v === 'object' && !Array.isArray(v);
}

/**
 * Group run metrics for display. Returns `[{id, title, rows: [{key, label,
 * text, tone}]}]` in scorecard order. Fields the viewer does not know (from
 * newer simulators) are kept: in their group if it exists, else in "Other".
 */
export function scorecard(metrics, solLength) {
  if (!isPlainObject(metrics)) return [];
  const out = [];
  const other = { id: 'other', title: 'Other', rows: [] };
  const knownGroups = new Set(GROUPS.map((g) => g.at).filter(Boolean));
  for (const g of GROUPS) {
    const src = g.at ? metrics[g.at] : metrics;
    if (!isPlainObject(src)) continue;
    const used = new Set([...(g.folds ?? [])]);
    const rows = [];
    for (const r of g.rows) {
      if (!(r.key in src)) continue;
      used.add(r.key);
      const v = src[r.key];
      const ok = typeof v === 'number' ? Number.isFinite(v) : typeof v === 'boolean';
      rows.push({
        key: r.key,
        label: r.label,
        hint: typeof r.hint === 'function' ? (ok ? r.hint(v, src) : r.label) : r.hint ?? r.label,
        text: ok ? r.text(v, src, solLength) : String(v),
        tone: ok && r.tone ? r.tone(v, src) ?? null : null,
      });
    }
    for (const [k, v] of Object.entries(src)) {
      if (used.has(k) || (g.at === null && (knownGroups.has(k) || isPlainObject(v)))) continue;
      const label = humanize(k).replace(/^./, (c) => c.toUpperCase());
      rows.push({ key: k, label, hint: label, text: formatMetric(k, v, solLength), tone: null });
    }
    if (rows.length) out.push({ id: g.id, title: g.title, rows });
  }
  for (const [k, v] of Object.entries(metrics)) {
    if (!isPlainObject(v) || knownGroups.has(k)) continue;
    for (const [kk, vv] of Object.entries(v)) {
      if (isPlainObject(vv)) continue;
      const label = `${humanize(k)}: ${humanize(kk)}`;
      other.rows.push({ key: `${k}.${kk}`, label, hint: label, text: formatMetric(kk, vv, solLength), tone: null });
    }
  }
  if (other.rows.length) out.push(other);
  return out;
}

/** One-line headline for the top of the scorecard. */
export function scorecardHeadline(metrics, solLength) {
  if (!isPlainObject(metrics)) return null;
  const sols = Number.isFinite(metrics.sols) ? `${fmtNumber(metrics.sols, 2)} sols` : null;
  const q = metrics.quality;
  const quality = isPlainObject(q) && Number.isFinite(q.cells_in_spec) && Number.isFinite(q.cells_total) ? `${q.cells_in_spec}/${q.cells_total} cells in spec` : null;
  const lead = metrics.completed === true ? 'Pad finished' : metrics.completed === false ? 'Pad not finished' : 'Run ended';
  return [lead, sols && (metrics.completed ? `in ${sols}` : `after ${sols}`), quality].filter(Boolean).join(', ');
}
