// Time-series helpers: binary search and interpolation over sorted sample times.
// Every query the viewer makes per frame goes through these, so they stay O(log n).

/** Index of the last element of sorted `times` that is <= t, or -1 if none. */
export function lastAtOrBefore(times, t) {
  let lo = 0;
  let hi = times.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] <= t) {
      ans = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return ans;
}

/** Index of the first element of sorted `times` that is > t (times.length if none). */
export function firstAfter(times, t) {
  return lastAtOrBefore(times, t) + 1;
}

/** Index of the first element of sorted `times` that is >= t (times.length if none). */
export function firstAtOrAfter(times, t) {
  let lo = 0;
  let hi = times.length - 1;
  let ans = times.length;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] >= t) {
      ans = mid;
      hi = mid - 1;
    } else {
      lo = mid + 1;
    }
  }
  return ans;
}

export function lerp(a, b, f) {
  return a + (b - a) * f;
}

export function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

/**
 * A numeric time series. Samples must be pushed in non-decreasing time order;
 * a sample at the same time as the previous one replaces it.
 */
export class Series {
  constructor() {
    this.t = [];
    this.v = [];
  }

  get length() {
    return this.t.length;
  }

  push(t, v) {
    if (!Number.isFinite(t) || !Number.isFinite(v)) return;
    const n = this.t.length;
    if (n > 0 && t < this.t[n - 1]) return;
    if (n > 0 && t === this.t[n - 1]) {
      this.v[n - 1] = v;
      return;
    }
    this.t.push(t);
    this.v.push(v);
  }

  /** Value at t: linear between samples, held flat outside them. Null when empty. */
  linear(t) {
    const n = this.t.length;
    if (n === 0) return null;
    const i = lastAtOrBefore(this.t, t);
    if (i < 0) return this.v[0];
    if (i >= n - 1) return this.v[n - 1];
    const t0 = this.t[i];
    const t1 = this.t[i + 1];
    const f = t1 > t0 ? (t - t0) / (t1 - t0) : 1;
    return lerp(this.v[i], this.v[i + 1], f);
  }

  /** Value of the latest sample at or before t, or null before the first sample. */
  step(t) {
    const i = lastAtOrBefore(this.t, t);
    return i < 0 ? null : this.v[i];
  }

  min() {
    let m = Infinity;
    for (const v of this.v) if (v < m) m = v;
    return this.v.length ? m : null;
  }

  max() {
    let m = -Infinity;
    for (const v of this.v) if (v > m) m = v;
    return this.v.length ? m : null;
  }
}

/** Intervals [start, end) as parallel arrays, sorted and non-overlapping. */
export class Intervals {
  constructor(items = []) {
    this.items = items.slice().sort((a, b) => a.start - b.start);
    this.starts = this.items.map((i) => i.start);
  }

  /** The interval containing t, or null. */
  at(t) {
    const i = lastAtOrBefore(this.starts, t);
    if (i < 0) return null;
    const it = this.items[i];
    return t < it.end ? it : null;
  }

  /** Total length of the intervals clipped to [from, to]. */
  total(from = -Infinity, to = Infinity) {
    let s = 0;
    for (const it of this.items) {
      const a = Math.max(it.start, from);
      const b = Math.min(it.end, to);
      if (b > a) s += b - a;
    }
    return s;
  }
}
