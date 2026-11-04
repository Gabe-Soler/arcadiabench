// Site state over time: the ground-truth grid at the start, every cell change
// reported by truth records, and landing-pad progress derived from them.

import { lastAtOrBefore, Series } from './series.js';

export const SURFACES = ['natural', 'cleared', 'graded', 'compacted', 'stabilized', 'berm', 'stockpile'];
/** Code for surface names this viewer does not know (newer protocol versions). */
export const OTHER_SURFACE = SURFACES.length;
const SURFACE_CODE = new Map(SURFACES.map((s, i) => [s, i]));

export function surfaceCode(name) {
  if (name == null) return 0;
  return SURFACE_CODE.get(name) ?? OTHER_SURFACE;
}

function boulderList(cell) {
  if (!Array.isArray(cell?.boulders)) return undefined;
  const out = [];
  for (const b of cell.boulders) if (b && Number.isFinite(b.diameter_m) && b.diameter_m > 0) out.push(b.diameter_m);
  return out;
}

/**
 * The grid: initial ground truth plus per-cell change timelines.
 * Cells absent from the true site map are unknown (`known0` is 0).
 */
export class SiteIndex {
  constructor(site) {
    const width = Math.max(1, Math.floor(Number(site?.width) || 0));
    const height = Math.max(1, Math.floor(Number(site?.height) || 0));
    this.width = width;
    this.height = height;
    this.cellSize = Number(site?.cell_size_m) > 0 ? Number(site.cell_size_m) : 1;
    this.siteId = site?.site_id ?? 'site';
    this.name = site?.name ?? null;
    const n = width * height;
    this.elev0 = new Float64Array(n).fill(NaN);
    this.surf0 = new Uint8Array(n);
    this.known0 = new Uint8Array(n);
    this.boulders0 = new Array(n).fill(null);
    let lo = Infinity;
    let hi = -Infinity;
    for (const obs of Array.isArray(site?.cells) ? site.cells : []) {
      const i = this.index(obs?.at);
      if (i < 0) continue;
      const c = obs.cell ?? {};
      this.known0[i] = 1;
      if (Number.isFinite(c.elevation_m)) {
        this.elev0[i] = c.elevation_m;
        if (c.elevation_m < lo) lo = c.elevation_m;
        if (c.elevation_m > hi) hi = c.elevation_m;
      }
      this.surf0[i] = surfaceCode(c.surface);
      const b = boulderList(c);
      if (b && b.length) this.boulders0[i] = b;
    }
    this.elevRange = Number.isFinite(lo) ? [lo, hi] : [0, 0];
    /** idx -> { t: [], elev: [], surf: [], boulders: [] } */
    this.timelines = new Map();
    /** Every change, sorted by time once `seal()` runs: { t, idx, k }. */
    this.changes = [];
  }

  index(at) {
    if (!at || !Number.isInteger(at.x) || !Number.isInteger(at.y)) return -1;
    if (at.x < 0 || at.y < 0 || at.x >= this.width || at.y >= this.height) return -1;
    return at.y * this.width + at.x;
  }

  /** Record that cell `idx` became `cell` at time t. Missing fields keep their previous value. */
  addChange(idx, t, cell) {
    let tl = this.timelines.get(idx);
    if (!tl) {
      tl = { t: [], elev: [], surf: [], boulders: [] };
      this.timelines.set(idx, tl);
    }
    const k = tl.t.length;
    const prevElev = k ? tl.elev[k - 1] : this.elev0[idx];
    const prevSurf = k ? tl.surf[k - 1] : this.surf0[idx];
    const prevB = k ? tl.boulders[k - 1] : this.boulders0[idx];
    const tt = k && t < tl.t[k - 1] ? tl.t[k - 1] : t;
    const b = boulderList(cell);
    tl.t.push(tt);
    tl.elev.push(Number.isFinite(cell?.elevation_m) ? cell.elevation_m : prevElev);
    tl.surf.push(cell?.surface != null ? surfaceCode(cell.surface) : prevSurf);
    tl.boulders.push(b === undefined ? prevB : b.length ? b : null);
    this.known0[idx] = 1;
    this.changes.push({ t: tt, idx, k });
  }

  /** Sort the global change list; call once after all changes are added. */
  seal() {
    this.changes.sort((a, b) => a.t - b.t || a.idx - b.idx || a.k - b.k);
    this.changeTimes = this.changes.map((c) => c.t);
  }

  /** Index into cell idx's timeline in effect at t, or -1 for the initial state. */
  stateIndex(idx, t) {
    const tl = this.timelines.get(idx);
    return tl ? lastAtOrBefore(tl.t, t) : -1;
  }

  /** `{elev, surf, boulders}` of cell idx at time t. */
  cellAt(idx, t) {
    const tl = this.timelines.get(idx);
    const k = tl ? lastAtOrBefore(tl.t, t) : -1;
    if (k < 0) return { elev: this.elev0[idx], surf: this.surf0[idx], boulders: this.boulders0[idx] };
    return { elev: tl.elev[k], surf: tl.surf[k], boulders: tl.boulders[k] };
  }

  elevationAt(idx, t) {
    const tl = this.timelines.get(idx);
    const k = tl ? lastAtOrBefore(tl.t, t) : -1;
    return k < 0 ? this.elev0[idx] : tl.elev[k];
  }

  /** Number of changes at or before t (an index into `changes`). */
  changesUpTo(t) {
    return lastAtOrBefore(this.changeTimes ?? [], t) + 1;
  }
}

/** Cells of an inclusive rectangle, clipped to the site, as indices. */
export function rectIndices(site, area) {
  const out = [];
  if (!area?.min || !area?.max) return out;
  const x0 = Math.max(0, area.min.x);
  const y0 = Math.max(0, area.min.y);
  const x1 = Math.min(site.width - 1, area.max.x);
  const y1 = Math.min(site.height - 1, area.max.y);
  for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) out.push(y * site.width + x);
  return out;
}

/**
 * Landing-pad progress over time, by the simulator's rules: a pad cell
 * counts when it has the required surface, no boulders and is within
 * tolerance of the target; it is verified when a scan that started after
 * its last change has finished; a berm cell is done when it is berm at
 * least `height_m` above its starting elevation.
 *
 * `scans` is `[{start, end, cells: [idx]}]`.
 */
export function padProgress(site, pad, scans = []) {
  const cells = rectIndices(site, pad.area);
  const required = pad.stabilize === false ? SURFACES.indexOf('compacted') : SURFACES.indexOf('stabilized');
  const tol = Number.isFinite(pad.surface_tolerance_m) ? pad.surface_tolerance_m : 0.05;
  const target = pad.target_elevation_m;
  const inPad = new Map(cells.map((idx, i) => [idx, i]));
  const ok = (surf, elev, boulders) =>
    surf === required && !(boulders && boulders.length) && Number.isFinite(elev) && Math.abs(elev - target) <= tol + 1e-9;

  const inSpec = new Uint8Array(cells.length);
  let count = 0;
  cells.forEach((idx, i) => {
    if (ok(site.surf0[idx], site.elev0[idx], site.boulders0[idx])) {
      inSpec[i] = 1;
      count++;
    }
  });

  const bermCells = [];
  const bermIndex = new Map();
  const bermHeight = Number(pad.berm?.height_m) || 0;
  for (const c of pad.berm?.cells ?? []) {
    const idx = site.index(c);
    if (idx < 0 || bermIndex.has(idx)) continue;
    bermIndex.set(idx, bermCells.length);
    bermCells.push({ idx, base: site.elev0[idx] });
  }
  const bermSurf = SURFACES.indexOf('berm');
  const bermOk = (b, surf, elev) => surf === bermSurf && Number.isFinite(elev) && elev >= b.base + bermHeight - 1e-9;
  const bermDone = new Uint8Array(bermCells.length);
  let bermCount = 0;
  bermCells.forEach((b, i) => {
    if (bermOk(b, site.surf0[b.idx], site.elev0[b.idx])) {
      bermDone[i] = 1;
      bermCount++;
    }
  });

  // Merge cell changes and scan completions in time order (changes first on ties).
  const events = [];
  for (const ch of site.changes) {
    if (inPad.has(ch.idx) || bermIndex.has(ch.idx)) events.push({ t: ch.t, type: 0, ch });
  }
  for (const s of scans) events.push({ t: s.end, type: 1, scan: s });
  events.sort((a, b) => a.t - b.t || a.type - b.type);

  const specSeries = new Series();
  const verifiedSeries = new Series();
  const bermSeries = new Series();
  specSeries.push(-Number.MAX_VALUE, count);
  verifiedSeries.push(-Number.MAX_VALUE, 0);
  bermSeries.push(-Number.MAX_VALUE, bermCount);
  const verified = new Uint8Array(cells.length);
  const lastChange = new Float64Array(cells.length).fill(-Infinity);
  let verifiedCount = 0;

  for (const ev of events) {
    if (ev.type === 0) {
      const { idx, k } = ev.ch;
      const tl = site.timelines.get(idx);
      const i = inPad.get(idx);
      if (i !== undefined) {
        const now = ok(tl.surf[k], tl.elev[k], tl.boulders[k]) ? 1 : 0;
        if (now !== inSpec[i]) {
          count += now ? 1 : -1;
          inSpec[i] = now;
          specSeries.push(ev.t, count);
        }
        lastChange[i] = ev.t;
        if (verified[i]) {
          verified[i] = 0;
          verifiedCount--;
          verifiedSeries.push(ev.t, verifiedCount);
        }
      }
      const bi = bermIndex.get(idx);
      if (bi !== undefined) {
        const now = bermOk(bermCells[bi], tl.surf[k], tl.elev[k]) ? 1 : 0;
        if (now !== bermDone[bi]) {
          bermCount += now ? 1 : -1;
          bermDone[bi] = now;
          bermSeries.push(ev.t, bermCount);
        }
      }
    } else {
      let changed = false;
      for (const idx of ev.scan.cells) {
        const i = inPad.get(idx);
        if (i === undefined || verified[i]) continue;
        if (lastChange[i] <= ev.scan.start + 1e-9) {
          verified[i] = 1;
          verifiedCount++;
          changed = true;
        }
      }
      if (changed) verifiedSeries.push(ev.t, verifiedCount);
    }
  }

  return {
    cells,
    bermCells,
    total: cells.length,
    bermTotal: bermCells.length,
    inSpec: specSeries,
    verified: verifiedSeries,
    berm: bermSeries,
    /** Steepest slope between adjacent pad cells at time t, degrees. */
    maxSlopeDeg(t) {
      let worst = 0;
      const { min, max } = pad.area;
      for (let y = Math.max(0, min.y); y <= Math.min(site.height - 1, max.y); y++) {
        for (let x = Math.max(0, min.x); x <= Math.min(site.width - 1, max.x); x++) {
          const here = site.elevationAt(y * site.width + x, t);
          if (x + 1 <= Math.min(site.width - 1, max.x)) {
            const d = Math.abs(site.elevationAt(y * site.width + x + 1, t) - here);
            if (d > worst) worst = d;
          }
          if (y + 1 <= Math.min(site.height - 1, max.y)) {
            const d = Math.abs(site.elevationAt((y + 1) * site.width + x, t) - here);
            if (d > worst) worst = d;
          }
        }
      }
      return (Math.atan(worst / site.cellSize) * 180) / Math.PI;
    },
  };
}

/**
 * Where the pad stands, for the panel's tag. `counts` is a padAt() result,
 * `surfaces` the pad cells per surface code, and `opts` gives the time, when
 * the goal arrived and whether the run finished the goal by then.
 *
 * - `waiting`: the goal has not arrived yet.
 * - `not_started`: every pad cell is untouched.
 * - `in_progress`: work has begun, even if no cell is in spec yet.
 * - `verifying`: every cell is in spec (and the berm built), awaiting scans.
 * - `complete`: in spec, berm built and verified, or the run ended complete.
 */
export function padStage(counts, surfaces, { t, goalT = -Infinity, complete = false } = {}) {
  if (!counts) return { key: 'none', label: 'no goal' };
  if (t < goalT) return { key: 'waiting', label: 'no goal yet' };
  const built = counts.inSpec >= counts.total && counts.berm >= counts.bermTotal;
  if (complete || (built && counts.verified >= counts.total)) return { key: 'complete', label: 'complete' };
  if (built) return { key: 'verifying', label: 'awaiting verification' };
  const natural = surfaces?.[0] ?? counts.total;
  const worked = counts.total - natural;
  if (worked > 0 || counts.inSpec > 0 || counts.berm > 0) return { key: 'in_progress', label: 'in progress' };
  return { key: 'not_started', label: 'not started' };
}
