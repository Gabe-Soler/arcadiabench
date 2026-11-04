// Display formatting: the sol clock, durations, power, energy and command summaries.

/** Mars sol in seconds, used when a log does not say. */
export const DEFAULT_SOL_S = 88775;

/** Daylight runs from 25% to 75% of each sol; t = 0 is local midnight. */
export const SUNRISE = 0.25;
export const SUNSET = 0.75;

/**
 * Local sol clock for sim time `t`. The sol is split into 24 local hours,
 * so 06:00 is sunrise and 18:00 sunset.
 */
export function solClock(t, solLength = DEFAULT_SOL_S) {
  const len = solLength > 0 ? solLength : DEFAULT_SOL_S;
  const safe = Math.max(0, Number.isFinite(t) ? t : 0);
  let sol = Math.floor(safe / len);
  let phase = (safe - sol * len) / len;
  if (phase >= 1) {
    sol += 1;
    phase = 0;
  }
  const minutes = Math.floor(phase * 24 * 60 + 1e-9);
  const hh = Math.floor(minutes / 60);
  const mm = minutes % 60;
  const daylight = phase >= SUNRISE && phase < SUNSET;
  return {
    sol,
    phase,
    hh,
    mm,
    daylight,
    elapsedSols: safe / len,
    text: `Sol ${sol} · ${pad2(hh)}:${pad2(mm)}`,
  };
}

/**
 * Darkness of the sky at a sol phase: 0 in full daylight, 1 at night, with a
 * short twilight ramp around sunrise and sunset.
 */
export function nightFactor(phase, twilight = 0.03) {
  // Signed distance (in sol fractions) into daylight: positive by day, negative at night.
  let d;
  if (phase >= SUNRISE && phase < SUNSET) {
    d = Math.min(phase - SUNRISE, SUNSET - phase);
  } else if (phase < SUNRISE) {
    d = -Math.min(SUNRISE - phase, phase + (1 - SUNSET));
  } else {
    d = -Math.min(phase - SUNSET, 1 - phase + SUNRISE);
  }
  return Math.min(1, Math.max(0, 0.5 - d / twilight));
}

export function pad2(n) {
  return String(n).padStart(2, '0');
}

/** Compact duration: "45 s", "12m 40s", "3h 05m", "2.4 sols". */
export function fmtDuration(seconds, solLength = DEFAULT_SOL_S) {
  if (seconds == null || !Number.isFinite(seconds)) return '–';
  const neg = seconds < 0;
  let s = Math.abs(seconds);
  let out;
  if (s < 60) out = `${s < 10 ? s.toFixed(1).replace(/\.0$/, '') : Math.round(s)} s`;
  else if (s < 3600) {
    const m = Math.floor(s / 60);
    out = `${m}m ${pad2(Math.round(s - m * 60) % 60)}s`;
  } else if (s < solLength) {
    const h = Math.floor(s / 3600);
    out = `${h}h ${pad2(Math.floor((s - h * 3600) / 60))}m`;
  } else {
    out = `${(s / solLength).toFixed(s < 10 * solLength ? 2 : 1)} sols`;
  }
  return neg ? `−${out}` : out;
}

export function fmtPower(w) {
  if (w == null || !Number.isFinite(w)) return '–';
  const a = Math.abs(w);
  if (a >= 1e6) return `${(w / 1e6).toFixed(2)} MW`;
  if (a >= 1000) return `${(w / 1000).toFixed(a >= 1e4 ? 1 : 2)} kW`;
  return `${Math.round(w)} W`;
}

export function fmtEnergy(wh) {
  if (wh == null || !Number.isFinite(wh)) return '–';
  const a = Math.abs(wh);
  if (a >= 1e6) return `${(wh / 1e6).toFixed(2)} MWh`;
  if (a >= 1000) return `${(wh / 1000).toFixed(a >= 1e4 ? 1 : 2)} kWh`;
  return `${Math.round(wh)} Wh`;
}

export function fmtMass(kg) {
  if (kg == null || !Number.isFinite(kg)) return '–';
  if (Math.abs(kg) >= 1000) return `${(kg / 1000).toFixed(2)} t`;
  return `${Math.round(kg)} kg`;
}

export function fmtPct(f, digits = 0) {
  if (f == null || !Number.isFinite(f)) return '–';
  return `${(f * 100).toFixed(digits)}%`;
}

export function fmtNumber(v, digits = 2) {
  if (v == null || !Number.isFinite(v)) return '–';
  if (Number.isInteger(v)) return v.toLocaleString('en-US');
  return v.toLocaleString('en-US', { maximumFractionDigits: digits });
}

export function fmtCoord(c) {
  if (!c || typeof c !== 'object') return '?';
  return `(${c.x}, ${c.y})`;
}

export function fmtArea(a) {
  if (!a || !a.min || !a.max) return '?';
  if (a.min.x === a.max.x && a.min.y === a.max.y) return fmtCoord(a.min);
  return `${fmtCoord(a.min)}–${fmtCoord(a.max)}`;
}

/** Sentence-case label for snake_case protocol names: "drive_motor" -> "drive motor". */
export function humanize(name) {
  return String(name ?? '').replaceAll('_', ' ');
}

/** One-line description of a command, without its id or robot. */
export function describeCommand(cmd) {
  if (!cmd || typeof cmd !== 'object') return '?';
  switch (cmd.type) {
    case 'move_to':
      return `move to ${fmtCoord(cmd.target)}`;
    case 'scan':
      return `scan ${fmtArea(cmd.area)}`;
    case 'dig':
      return `dig ${fmtArea(cmd.area)} to ${fmtElevation(cmd.target_elevation_m)}${cmd.spoil_to ? `, spoil to ${fmtCoord(cmd.spoil_to)}` : ''}`;
    case 'grade':
      return `grade ${fmtArea(cmd.area)} to ${fmtElevation(cmd.target_elevation_m)}`;
    case 'haul':
      return `haul ${fmtMass(cmd.mass_kg)} ${fmtCoord(cmd.from)} → ${fmtCoord(cmd.to)}`;
    case 'compact':
      return `compact ${fmtArea(cmd.area)}`;
    case 'sinter':
      return `sinter ${fmtArea(cmd.area)}`;
    case 'repair':
      return `repair ${cmd.target_robot}`;
    case 'tow':
      return `tow ${cmd.target_robot} to ${fmtCoord(cmd.to)}`;
    case 'cannibalize':
      return `cannibalize ${cmd.target_robot}${cmd.beneficiary_robot ? ` for ${cmd.beneficiary_robot}` : ''}`;
    case 'charge':
      return `charge at ${cmd.source}${cmd.until_wh ? ` to ${fmtEnergy(cmd.until_wh)}` : ''}`;
    case 'relay':
      return `relay at ${fmtCoord(cmd.position)}`;
    case 'service':
      return `service at ${cmd.depot}`;
    case 'wait':
      return `wait ${fmtDuration(cmd.duration_s)}`;
    case 'cancel':
      return `cancel ${cmd.target_command}`;
    default:
      return humanize(cmd.type ?? 'command');
  }
}

export function fmtElevation(m) {
  if (m == null || !Number.isFinite(m)) return '–';
  return `${m.toFixed(2)} m`;
}

/** "Sol 1 · 14:32" with an optional precise seconds suffix. */
export function fmtTime(t, solLength, precise = false) {
  const c = solClock(t, solLength);
  return precise ? `${c.text} (t = ${fmtNumber(Math.round(t * 10) / 10, 1)} s)` : c.text;
}

/** Format one run metric by its key's unit suffix. Unknown keys still show. */
export function formatMetric(key, value, solLength = DEFAULT_SOL_S) {
  if (typeof value === 'boolean') return value ? 'yes' : 'no';
  if (typeof value !== 'number') return String(value);
  if (key.endsWith('_wh')) return fmtEnergy(value);
  if (key.endsWith('_s')) return fmtDuration(value, solLength);
  if (key.endsWith('_deg')) return `${fmtNumber(value, 2)}°`;
  if (key.endsWith('_m')) return `${fmtNumber(value, 3)} m`;
  if (key.endsWith('fraction')) return fmtPct(value, 1);
  return fmtNumber(value, 2);
}

/**
 * Parse the `t` URL parameter: sim time in seconds, optionally with a unit
 * ("90", "90s", "15m", "2h", "1.5sol"). Returns null when it is not a time.
 */
export function parseTimeParam(raw, solLength = DEFAULT_SOL_S) {
  if (raw == null) return null;
  const m = /^\s*(\d+(?:\.\d+)?|\.\d+)\s*(s|sec|m|min|h|sol|sols)?\s*$/i.exec(String(raw));
  if (!m) return null;
  const v = Number(m[1]);
  const unit = (m[2] ?? 's').toLowerCase();
  const k = { s: 1, sec: 1, m: 60, min: 60, h: 3600, sol: solLength, sols: solLength }[unit];
  return v * k;
}
