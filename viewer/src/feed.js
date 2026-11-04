// Event feed items: one readable line per notable log record.

import { describeCommand, fmtDuration, fmtEnergy, humanize, fmtNumber } from './format.js';

/** Feed categories, in the order the filter chips show them. */
export const CATEGORIES = [
  ['command', 'Commands'],
  ['result', 'Results'],
  ['fault', 'Faults'],
  ['earth', 'Earth'],
  ['site', 'Site'],
];

/** Short description of an Earth link message (either direction). */
export function describeEarthMessage(m) {
  if (!m || typeof m !== 'object') return 'message';
  switch (m.type) {
    case 'goal': {
      const pad = m.objective?.kind === 'landing_pad' ? m.objective.pad : null;
      if (pad?.area) {
        const w = pad.area.max.x - pad.area.min.x + 1;
        const h = pad.area.max.y - pad.area.min.y + 1;
        return `goal ${m.goal_id}: landing pad ${w} × ${h} cells${pad.berm ? ' with berm' : ''}`;
      }
      return `goal ${m.goal_id ?? ''}`.trim();
    }
    case 'approve':
      return `approve ${m.request_id}${m.note ? ` (“${m.note}”)` : ''}`;
    case 'veto':
      return `veto ${m.request_id}${m.note ? ` (“${m.note}”)` : ''}`;
    case 'reprioritize':
      return 'reprioritize goals';
    case 'policy_update':
      return `policy update v${m.policy?.policy_version ?? '?'}`;
    case 'notify':
      return `notify ${humanize(m.action)}: ${m.summary ?? ''}`.trim();
    case 'approval_request':
      return `approval request ${humanize(m.action)}: ${m.summary ?? ''}`.trim();
    case 'status_report':
      return `status report: ${m.summary ?? ''}`.trim();
    default:
      return humanize(m.type ?? 'message');
  }
}

function late(recordT, eventT) {
  return Number.isFinite(eventT) && recordT - eventT > 1 ? recordT - eventT : 0;
}

/**
 * Feed item for an observed event, or null when the event is not feed-worthy
 * (acceptances and routine power reports are summarised elsewhere).
 * `t` is when the record was logged (delivery); `et` is when the event happened.
 */
export function itemFromEvent(e, t) {
  const et = Number.isFinite(e.t) ? e.t : t;
  const base = { t, et, late: late(t, et), robot: e.robot_id ?? null, id: e.command_id ?? null };
  switch (e.type) {
    case 'command_accepted':
    case 'power':
      return null;
    case 'command_completed':
      return { ...base, cat: 'result', sev: 'ok', text: `completed ${e.command_id}`, note: fmtEnergy(e.energy_used_wh) };
    case 'command_failed':
      return {
        ...base,
        cat: 'result',
        sev: e.reason === 'cancelled' ? 'info' : 'error',
        text: `failed ${e.command_id}: ${humanize(e.reason)}`,
        note: e.detail ?? '',
      };
    case 'command_rejected':
      return { ...base, cat: 'result', sev: 'warn', text: `rejected ${e.command_id}: ${humanize(e.reason)}`, note: e.detail ?? '' };
    case 'scan_result':
      return { ...base, cat: 'result', sev: 'info', text: `scan result: ${Array.isArray(e.cells) ? e.cells.length : 0} cells` };
    case 'robot_fault': {
      const depot = e.field_repairable === false ? ', needs depot service' : '';
      return {
        ...base,
        cat: 'fault',
        sev: e.severity === 'disabled' ? 'error' : 'warn',
        text: `fault: ${humanize(e.component)} ${e.severity} (${humanize(e.cause)}${depot})`,
        note: e.detail ?? '',
      };
    }
    case 'weather':
      return {
        ...base,
        robot: null,
        cat: 'site',
        sev: e.condition === 'dust_storm' ? 'warn' : 'info',
        text: e.condition === 'dust_storm' ? `dust storm, optical depth ${fmtNumber(e.optical_depth, 2)}` : `clear skies, optical depth ${fmtNumber(e.optical_depth, 2)}`,
      };
    case 'comms':
      return {
        ...base,
        robot: null,
        cat: 'site',
        sev: e.link === 'down' ? 'error' : 'info',
        text: `Earth link ${e.link} (${humanize(e.cause)}), one-way delay ${fmtDuration(e.one_way_delay_s)}`,
        note: Number.isFinite(e.expected_restore_t) ? `expected back at t = ${fmtNumber(Math.round(e.expected_restore_t))} s` : '',
      };
    case 'earth_message':
      return {
        ...base,
        robot: null,
        cat: 'earth',
        sev: 'info',
        text: `received from Earth: ${describeEarthMessage(e.message)}`,
        note: Number.isFinite(e.sent_t) && et - e.sent_t > 0 ? `sent ${fmtDuration(et - e.sent_t)} earlier` : '',
      };
    default:
      return { ...base, cat: 'site', sev: 'info', text: humanize(e.type ?? 'event') };
  }
}

export function itemFromCommand(c, t) {
  return {
    t,
    et: t,
    late: 0,
    robot: c.robot_id ?? null,
    id: c.command_id ?? null,
    cat: 'command',
    sev: 'info',
    text: describeCommand(c),
    note: c.approval_id ? `approval ${c.approval_id}` : '',
  };
}

/** Feed item for an Earth link record; `rec.deliver` (or `deliver_t`) is the arrival time. */
export function itemFromLink(direction, rec) {
  const toEarth = direction === 'down';
  const at = rec.deliver !== undefined ? rec.deliver : rec.deliver_t;
  const deliver = Number.isFinite(at) ? at : null;
  return {
    t: rec.t,
    et: rec.t,
    late: 0,
    robot: null,
    id: rec.message?.message_id ?? null,
    cat: 'earth',
    sev: 'info',
    text: `${toEarth ? 'sent to Earth' : 'Earth sent'}: ${describeEarthMessage(rec.message)}`,
    note: deliver == null ? 'never delivered' : `arrives in ${fmtDuration(deliver - rec.t)}`,
  };
}

export function itemFromRunEnd(rec) {
  const reason = humanize(rec.reason ?? 'unknown');
  return {
    t: rec.t,
    et: rec.t,
    late: 0,
    robot: null,
    id: null,
    cat: 'site',
    sev: rec.reason === 'goals_complete' ? 'ok' : rec.reason === 'time_limit' ? 'warn' : 'error',
    text: `run ended: ${reason}`,
  };
}
