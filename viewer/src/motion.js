// Robot motion between truth snapshots.
//
// Truth records are written only at decision points, so a hauler can drive to
// a stockpile and back between two snapshots. Straight-line interpolation would
// show it gliding across the pad instead. This module rebuilds plausible motion
// from what each command makes a robot do: the command's waypoints (a haul's
// source then destination, an area's cells in sweep order, a station, a depot),
// driven at the robot's top speed, with the time left over spent working at the
// waypoints. Snapshot positions are always honoured exactly. When a snapshot
// does not fit the command's path, the robot heads straight for it.
//
// The result is a keyframe track per robot: position is linear between
// keyframes; status and payload hold from one keyframe to the next.

import { lastAtOrBefore } from './series.js';

/** Truth positions are whole cells, so a robot is "on" a path within this distance. */
const PATH_TOL = 0.75;
/** Robots that work on another robot stop next to it rather than on it. */
const NEAR_TOL = 1.6;
/**
 * A tow moves at a fraction of the tower's speed (ArcadiaBench documents 0.5).
 * Only the shape of the timing depends on it: legs are fitted to the
 * snapshots either way.
 */
export const TOW_PACE = 0.5;

export function dist(a, b) {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function isPoint(p) {
  return p != null && Number.isFinite(p.x) && Number.isFinite(p.y);
}

/** Cells of an area row by row, alternating direction, so a sweep never doubles back. */
export function serpentine(area) {
  const out = [];
  if (!area?.min || !area?.max) return out;
  const { min, max } = area;
  if (!(max.x >= min.x && max.y >= min.y)) return out;
  for (let y = min.y, row = 0; y <= max.y; y++, row++) {
    if (row % 2 === 0) for (let x = min.x; x <= max.x; x++) out.push({ x, y });
    else for (let x = max.x; x >= min.x; x--) out.push({ x, y });
  }
  return out;
}

/** The cell of `area` nearest to `p`. */
export function clampInto(p, area) {
  return {
    x: Math.min(Math.max(p.x, area.min.x), area.max.x),
    y: Math.min(Math.max(p.y, area.min.y), area.max.y),
  };
}

/** Intersect an area with the site; null when empty or malformed. */
export function clipArea(area, bounds) {
  if (!area?.min || !area?.max || !isPoint(area.min) || !isPoint(area.max)) return null;
  const min = { x: Math.max(0, area.min.x), y: Math.max(0, area.min.y) };
  const max = {
    x: Math.min(bounds ? bounds.width - 1 : area.max.x, area.max.x),
    y: Math.min(bounds ? bounds.height - 1 : area.max.y, area.max.y),
  };
  return max.x >= min.x && max.y >= min.y ? { min, max } : null;
}

/**
 * Waypoints a command makes a robot visit, in order. Each waypoint is
 * `{x, y, w, status, ...}` where `w` is its share of the working time.
 *
 * - `cell`: work there changes the cell (dig, grade, haul load/dump, ...).
 * - `load` / `dump`: payload effects of a haul.
 * - `scan`: the scan happens there.
 * - `near`: the robot stops next to it (another robot).
 * - `pickup`: a tow picks up this robot there.
 * - `pace`: speed on the leg into this waypoint, as a fraction of top speed.
 *
 * `ctx` supplies `start` (position when the command began), `stations` and
 * `depots` (id -> position), `robotPos(id)` and the site `bounds`.
 * Unknown command types yield no waypoints, so they degrade to plain
 * interpolation.
 */
export function commandWaypoints(cmd, ctx) {
  const points = [];
  const add = (p, extra = {}) => {
    if (isPoint(p)) points.push({ x: p.x, y: p.y, w: 0, ...extra });
  };
  const work = { w: 1, status: 'working' };
  switch (cmd?.type) {
    case 'move_to':
      add(cmd.target);
      break;
    case 'scan': {
      const area = clipArea(cmd.area, ctx.bounds);
      if (area && isPoint(ctx.start)) add(clampInto(ctx.start, area), { ...work, scan: true });
      break;
    }
    case 'dig':
    case 'grade':
    case 'compact':
    case 'sinter': {
      const area = clipArea(cmd.area, ctx.bounds);
      if (area) for (const c of serpentine(area)) add(c, { ...work, cell: true });
      break;
    }
    case 'haul':
      add(cmd.from, { ...work, cell: true, load: Number(cmd.mass_kg) || 0 });
      add(cmd.to, { ...work, cell: true, dump: true });
      break;
    case 'charge': {
      const station = ctx.stations?.get(cmd.source);
      if (station) add(station, { w: 1, status: 'charging' });
      else add(ctx.robotPos?.(cmd.source), { w: 1, status: 'charging', near: true });
      break;
    }
    case 'relay':
      add(cmd.position, work);
      break;
    case 'repair':
      add(ctx.robotPos?.(cmd.target_robot), { ...work, near: true });
      break;
    case 'tow':
      add(ctx.robotPos?.(cmd.target_robot), { near: true, pickup: cmd.target_robot });
      add(cmd.to, { pace: TOW_PACE });
      break;
    case 'cannibalize':
      add(ctx.robotPos?.(cmd.target_robot), { ...work, near: true });
      if (cmd.beneficiary_robot) add(ctx.robotPos?.(cmd.beneficiary_robot), { ...work, near: true });
      break;
    case 'service':
      add(ctx.depots?.get(cmd.depot), work);
      break;
    default:
      break;
  }
  return { points, holdStatus: cmd?.type === 'wait' ? 'idle' : 'working' };
}

/**
 * Lay a path out in time between t0 and t1. `path[0]` is the start; each later
 * vertex may carry a dwell weight `w`. The robot drives at `speed` (cells per
 * second) and the time left over is shared among dwells by weight. Without
 * dwells, or without enough time, it drives the whole path at the constant
 * speed that fits. Returns arrival and leave times per vertex.
 */
export function schedule(path, t0, t1, speed) {
  const n = path.length;
  const span = Math.max(0, t1 - t0);
  const len = [0];
  const legTime = [0];
  let travel = 0;
  let weights = 0;
  const v = speed > 0 ? speed : 1;
  for (let i = 1; i < n; i++) {
    const d = dist(path[i - 1], path[i]);
    len.push(d);
    const pace = path[i].pace > 0 ? path[i].pace : 1;
    legTime.push(d / (v * pace));
    travel += legTime[i];
    weights += path[i].w || 0;
  }
  let scale = 1;
  let spare = 0;
  if (weights > 0 && travel < span) spare = span - travel;
  else if (travel > 0) scale = span / travel;
  const arrive = [t0];
  const leave = [t0];
  let t = t0;
  for (let i = 1; i < n; i++) {
    t += legTime[i] * scale;
    arrive.push(Math.min(t, t1));
    if (weights > 0) t += (spare * (path[i].w || 0)) / weights;
    leave.push(Math.min(t, t1));
  }
  return { arrive, leave, len, idle: travel === 0 && weights === 0 };
}

/** Arc length along `path` (cumulative lengths `S`) reached at time t under `sched`. */
function arcAt(sched, S, t) {
  const n = S.length;
  if (t <= sched.leave[0]) return 0;
  for (let i = 1; i < n; i++) {
    if (t < sched.arrive[i]) {
      const a = sched.leave[i - 1];
      const b = sched.arrive[i];
      const f = b > a ? (t - a) / (b - a) : 1;
      return S[i - 1] + (S[i] - S[i - 1]) * f;
    }
    if (t < sched.leave[i]) return S[i];
  }
  return S[n - 1];
}

/** Arc lengths at which `path` passes within tolerance of `pos`. */
function projections(path, S, pos) {
  const out = [];
  for (let i = 1; i < path.length; i++) {
    const a = path[i - 1];
    const b = path[i];
    const L = S[i] - S[i - 1];
    let u = 0;
    if (L > 1e-9) {
      u = ((pos.x - a.x) * (b.x - a.x) + (pos.y - a.y) * (b.y - a.y)) / (L * L);
      u = Math.min(1, Math.max(0, u));
    }
    const q = { x: a.x + (b.x - a.x) * u, y: a.y + (b.y - a.y) * u };
    if (dist(pos, q) <= PATH_TOL) out.push(S[i - 1] + L * u);
    if (b.near && dist(pos, b) <= NEAR_TOL) out.push(S[i]);
  }
  return out;
}

/** Keyframes: parallel arrays of time, position, status and payload. */
export class Track {
  constructor() {
    this.t = [];
    this.x = [];
    this.y = [];
    this.s = [];
    this.p = [];
  }

  get length() {
    return this.t.length;
  }

  push(t, x, y, s, p) {
    const n = this.t.length;
    const tt = n > 0 && t < this.t[n - 1] ? this.t[n - 1] : t;
    this.t.push(tt);
    this.x.push(x);
    this.y.push(y);
    this.s.push(s);
    this.p.push(p);
  }

  /** State at time t: interpolated position, held status and payload, and heading when moving. */
  at(t) {
    const n = this.t.length;
    if (n === 0) return null;
    let i = lastAtOrBefore(this.t, t);
    if (i < 0) i = 0;
    let x = this.x[i];
    let y = this.y[i];
    let heading = null;
    if (i + 1 < n) {
      const t0 = this.t[i];
      const t1 = this.t[i + 1];
      const dx = this.x[i + 1] - x;
      const dy = this.y[i + 1] - y;
      if (t1 > t0 && t >= t0 && (dx !== 0 || dy !== 0)) {
        const f = Math.min(1, (t - t0) / (t1 - t0));
        x += dx * f;
        y += dy * f;
        heading = Math.atan2(dy, dx);
      }
    }
    return { x, y, status: this.s[i], payload: this.p[i], heading, index: i };
  }

  /** Positions over [t0, t1] (keyframes inside plus both ends), for drawing paths. */
  between(t0, t1) {
    const pts = [];
    const a = this.at(t0);
    if (a) pts.push({ x: a.x, y: a.y, t: t0 });
    let i = lastAtOrBefore(this.t, t0) + 1;
    for (; i < this.t.length && this.t[i] < t1; i++) pts.push({ x: this.x[i], y: this.y[i], t: this.t[i] });
    const b = this.at(t1);
    if (b) pts.push({ x: b.x, y: b.y, t: t1 });
    return pts;
  }
}

function layoutLeg(track, A, B, visited, speed, holdStatus, payload, capacity, out, cmd) {
  const path = [{ x: A.x, y: A.y, w: 0 }];
  for (const w of visited) path.push(w);
  const last = visited[visited.length - 1];
  if (last && dist(last, B) <= (last.near ? NEAR_TOL : PATH_TOL)) {
    // The snapshot is at the last waypoint: keep its work, take the exact position.
    path[path.length - 1] = { ...last, x: B.x, y: B.y, cx: last.cx ?? last.x, cy: last.cy ?? last.y };
  } else {
    path.push({ x: B.x, y: B.y, w: 0 });
  }
  const sched = schedule(path, A.t, B.t, speed);
  const n = path.length;
  const legStatus = (i) => (sched.len[i] > 1e-9 ? 'moving' : path[i].w ? path[i].status : holdStatus);
  track.push(A.t, A.x, A.y, sched.idle || n < 2 ? holdStatus : legStatus(1), payload);
  for (let i = 1; i < n; i++) {
    const p = path[i];
    const dwell = sched.leave[i] - sched.arrive[i];
    const next = i + 1 < n ? legStatus(i + 1) : null;
    if (dwell > 0) track.push(sched.arrive[i], p.x, p.y, p.status ?? holdStatus, payload);
    else if (next) track.push(sched.arrive[i], p.x, p.y, next, payload);
    if (p.load != null) payload = Math.max(payload, capacity > 0 ? Math.min(p.load, capacity) : p.load);
    if (p.dump) payload = 0;
    if (p.cell) out.work.push({ x: p.cx ?? p.x, y: p.cy ?? p.y, t: sched.leave[i] });
    if (p.scan) out.scans.push({ id: cmd.id, start: sched.arrive[i], end: sched.leave[i] });
    if (p.pickup) out.pickups.push({ id: cmd.id, target: p.pickup, t: sched.leave[i] });
    if (dwell > 0 && next) track.push(sched.leave[i], p.x, p.y, next, payload);
  }
  return payload;
}

function layoutCommand(track, anchors, waypoints, cmd, speed, holdStatus, payload, capacity, out) {
  const path = [{ x: anchors[0].x, y: anchors[0].y, w: 0 }, ...waypoints];
  const S = [0];
  for (let i = 1; i < path.length; i++) S.push(S[i - 1] + dist(path[i - 1], path[i]));
  const total = S[S.length - 1];

  // Expected progress over the whole command places snapshots that could sit
  // on more than one stretch of the path (out-and-back hauls overlap).
  const modelEnd = cmd.completed ? cmd.end : Math.max(cmd.end, cmd.start + (Number.isFinite(cmd.est) ? cmd.est : 0));
  const model = schedule(path, cmd.start, modelEnd, speed);
  const arc = [0];
  for (let k = 1; k < anchors.length; k++) {
    if (k === anchors.length - 1 && cmd.completed) {
      arc.push(total);
      continue;
    }
    const prev = arc[k - 1];
    const expected = arcAt(model, S, anchors[k].t);
    let best = null;
    for (const s of projections(path, S, anchors[k])) {
      if (s < prev - 1e-6) continue;
      if (best === null || Math.abs(s - expected) < Math.abs(best - expected)) best = s;
    }
    arc.push(best ?? prev);
  }

  let next = 1;
  for (let k = 0; k + 1 < anchors.length; k++) {
    const finalLeg = k + 2 === anchors.length;
    const visited = [];
    while (next < path.length && (S[next] <= arc[k + 1] + 0.5 || (finalLeg && cmd.completed))) {
      visited.push(path[next]);
      next++;
    }
    payload = layoutLeg(track, anchors[k], anchors[k + 1], visited, speed, holdStatus, payload, capacity, out, cmd);
  }
  return payload;
}

function interpolateSnaps(snaps, t) {
  const n = snaps.t.length;
  const i = lastAtOrBefore(snaps.t, t);
  if (i < 0) return { x: snaps.x[0], y: snaps.y[0] };
  if (i >= n - 1) return { x: snaps.x[n - 1], y: snaps.y[n - 1] };
  const t0 = snaps.t[i];
  const t1 = snaps.t[i + 1];
  const f = t1 > t0 ? (t - t0) / (t1 - t0) : 0;
  return { x: snaps.x[i] + (snaps.x[i + 1] - snaps.x[i]) * f, y: snaps.y[i] + (snaps.y[i + 1] - snaps.y[i]) * f };
}

/**
 * Build one robot's keyframe track.
 *
 * - `snaps`: truth samples `{t[], x[], y[], status[], payload[]}`, sorted by time.
 * - `commands`: the robot's accepted commands, sorted by start:
 *   `{id, cmd, start, end, completed, est}`; `end` is when it completed or failed.
 * - `opts.speed`: top speed in cells per second; `opts.capacity`: haul capacity, kg;
 *   `opts.ctxFor(command, startAnchor)`: context for `commandWaypoints`.
 *
 * Returns the track plus what the layout learned: when each worked cell was
 * finished (`work`), scan windows (`scans`) and tow pickups (`pickups`).
 */
export function buildTrack(snaps, commands, opts) {
  const track = new Track();
  const out = { work: [], scans: [], pickups: [] };
  const n = snaps.t.length;
  if (n === 0) return { track, ...out };
  const speed = opts.speed > 0 ? opts.speed : 1;
  const anchorAt = (i) => ({
    t: snaps.t[i],
    x: snaps.x[i],
    y: snaps.y[i],
    status: snaps.status[i],
    payload: snaps.payload[i],
  });
  const pushSnap = (i) => track.push(snaps.t[i], snaps.x[i], snaps.y[i], snaps.status[i], snaps.payload[i]);
  let si = 0;

  for (const cmd of commands) {
    if (!Number.isFinite(cmd.start) || !Number.isFinite(cmd.end)) continue;
    const lastT = track.length ? track.t[track.length - 1] : -Infinity;
    const start = Math.max(cmd.start, lastT);
    const end = cmd.end;
    if (end - start < 1e-6) continue;

    while (si < n && snaps.t[si] < start) pushSnap(si++);
    const anchors = [];
    let at = -1;
    while (si < n && snaps.t[si] === start) at = si++;
    anchors.push(at >= 0 ? anchorAt(at) : { t: start, ...interpolateSnaps(snaps, start), payload: null });
    if (anchors[0].payload == null) anchors[0].payload = snaps.payload[Math.max(0, lastAtOrBefore(snaps.t, start))];
    while (si < n && snaps.t[si] < end) anchors.push(anchorAt(si++));
    at = -1;
    while (si < n && snaps.t[si] === end) at = si++;
    let endAnchor;
    if (at >= 0) {
      endAnchor = anchorAt(at);
    } else {
      // No snapshot at the end: the robot is idle afterwards, so the next one shows where it stopped.
      const j = si < n ? si : n - 1;
      endAnchor = {
        t: end,
        x: snaps.x[j],
        y: snaps.y[j],
        status: si < n ? snaps.status[si] : 'idle',
        payload: snaps.payload[j],
      };
    }
    anchors.push(endAnchor);

    const ctx = opts.ctxFor ? opts.ctxFor(cmd, anchors[0]) : { start: anchors[0] };
    const { points, holdStatus } = commandWaypoints(cmd.cmd, ctx);
    layoutCommand(track, anchors, points, { ...cmd, start, end }, speed, holdStatus, anchors[0].payload ?? 0, opts.capacity ?? 0, out);
    track.push(end, endAnchor.x, endAnchor.y, endAnchor.status ?? 'idle', endAnchor.payload ?? 0);
  }
  while (si < n) pushSnap(si++);
  return { track, ...out };
}
