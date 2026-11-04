// The terrain layer: surface-state colours with hill shading and boulders,
// drawn once into an offscreen canvas and then patched cell by cell as the
// replay time moves, so a frame redraws only cells whose state changed.

import { SURFACES } from '../site.js';
import { lastAtOrBefore } from '../series.js';
import { SURFACE_RGB, OTHER_SURFACE_RGB, rgb } from './palette.js';
import { theme } from './theme.js';

/** Vertical exaggeration for shading: the site's relief is tens of centimetres. */
const EXAGGERATION = 5;
const LIGHT = (() => {
  const l = [-0.62, 0.62, 0.9];
  const n = Math.hypot(...l);
  return l.map((v) => v / n);
})();

function hash(n) {
  let x = n | 0;
  x = Math.imul(x ^ (x >>> 16), 0x45d9f3b);
  x = Math.imul(x ^ (x >>> 16), 0x45d9f3b);
  x ^= x >>> 16;
  return (x >>> 0) / 4294967296;
}

export class TerrainLayer {
  constructor(site) {
    this.site = site;
    this.canvas = document.createElement('canvas');
    this.ctx = this.canvas.getContext('2d');
    this.px = 0;
    this.elev = Float64Array.from(site.elev0);
    this.drawn = new Map();
    this.full = true;
    const [lo, hi] = site.elevRange;
    this.mid = (lo + hi) / 2;
    this.span = Math.max(0.05, hi - lo);
  }

  /** Redraw every cell on the next update, for a new colour scheme. */
  invalidate() {
    this.full = true;
  }

  /** Pick a resolution (canvas pixels per cell) for the current zoom. */
  setScale(pxPerCell) {
    const px = Math.max(3, Math.min(28, Math.round(pxPerCell)));
    if (px === this.px) return;
    this.px = px;
    this.canvas.width = this.site.width * px;
    this.canvas.height = this.site.height * px;
    this.full = true;
  }

  /**
   * Bring the layer to time t. Only cells with a change between the last
   * drawn time and t are looked at, found by binary search in the site's
   * time-sorted change list; a long jump revisits every changing cell
   * instead. Returns true when anything was redrawn.
   */
  update(t) {
    const site = this.site;
    const dirty = new Set();
    if (this.full) {
      this.full = false;
      this.t = t;
      this.elev = Float64Array.from(site.elev0);
      this.drawn.clear();
      for (const [idx, tl] of site.timelines) {
        const k = lastAtOrBefore(tl.t, t);
        this.drawn.set(idx, k);
        if (k >= 0) this.elev[idx] = tl.elev[k];
      }
      for (let i = 0; i < site.width * site.height; i++) this.drawCell(i, t);
      return true;
    }
    if (t === this.t) return false;
    const a = site.changesUpTo(Math.min(t, this.t));
    const b = site.changesUpTo(Math.max(t, this.t));
    this.t = t;
    let candidates;
    if (b - a <= site.timelines.size) {
      candidates = new Set();
      for (let i = a; i < b; i++) candidates.add(site.changes[i].idx);
    } else {
      candidates = site.timelines.keys();
    }
    for (const idx of candidates) {
      const tl = site.timelines.get(idx);
      const k = lastAtOrBefore(tl.t, t);
      if (k === (this.drawn.get(idx) ?? -1)) continue;
      this.drawn.set(idx, k);
      const e = k >= 0 ? tl.elev[k] : site.elev0[idx];
      dirty.add(idx);
      if (e !== this.elev[idx] && !(Number.isNaN(e) && Number.isNaN(this.elev[idx]))) {
        this.elev[idx] = e;
        // Shading depends on neighbours' elevations.
        const x = idx % site.width;
        const y = (idx - x) / site.width;
        if (x > 0) dirty.add(idx - 1);
        if (x < site.width - 1) dirty.add(idx + 1);
        if (y > 0) dirty.add(idx - site.width);
        if (y < site.height - 1) dirty.add(idx + site.width);
      }
    }
    for (const idx of dirty) this.drawCell(idx, t);
    return dirty.size > 0;
  }

  shade(idx) {
    const { width, height, cellSize } = this.site;
    const x = idx % width;
    const y = (idx - x) / width;
    const e = this.elev;
    const here = e[idx];
    if (!Number.isFinite(here)) return 1;
    const at = (xx, yy) => {
      const v = e[yy * width + xx];
      return Number.isFinite(v) ? v : here;
    };
    const xl = x > 0 ? x - 1 : x;
    const xr = x < width - 1 ? x + 1 : x;
    const yd = y > 0 ? y - 1 : y;
    const yu = y < height - 1 ? y + 1 : y;
    const dzdx = (at(xr, y) - at(xl, y)) / (Math.max(1, xr - xl) * cellSize);
    const dzdy = (at(x, yu) - at(x, yd)) / (Math.max(1, yu - yd) * cellSize);
    const nx = -dzdx * EXAGGERATION;
    const ny = -dzdy * EXAGGERATION;
    const n = Math.hypot(nx, ny, 1);
    const lit = (nx * LIGHT[0] + ny * LIGHT[1] + LIGHT[2]) / n;
    const relief = 1 + 1.35 * (lit - LIGHT[2]);
    const height_ = 1 + ((here - this.mid) / this.span) * 0.16;
    return Math.max(0.5, Math.min(1.45, relief * height_));
  }

  drawCell(idx, t) {
    const { width, height } = this.site;
    const px = this.px;
    const ctx = this.ctx;
    const x = idx % width;
    const y = (idx - x) / width;
    const sx = x * px;
    const sy = (height - 1 - y) * px;
    const c = this.site.cellAt(idx, t);
    if (!this.site.known0[idx]) {
      ctx.fillStyle = theme.unknown;
      ctx.fillRect(sx, sy, px, px);
      return;
    }
    const name = SURFACES[c.surf];
    const base = SURFACE_RGB[name] ?? OTHER_SURFACE_RGB;
    const f = this.shade(idx);
    ctx.fillStyle = rgb(base, f);
    ctx.fillRect(sx, sy, px, px);

    if (px >= 6) {
      if (name === 'berm') {
        ctx.save();
        ctx.beginPath();
        ctx.rect(sx, sy, px, px);
        ctx.clip();
        ctx.strokeStyle = 'rgba(70, 40, 10, 0.55)';
        ctx.lineWidth = Math.max(1, px / 9);
        ctx.beginPath();
        for (let d = -px; d < px; d += px / 3) {
          ctx.moveTo(sx + d, sy + px);
          ctx.lineTo(sx + d + px, sy);
        }
        ctx.stroke();
        ctx.restore();
      } else if (name === 'stockpile') {
        ctx.fillStyle = 'rgba(230, 190, 150, 0.35)';
        const r = Math.max(0.7, px / 14);
        for (let i = 0; i < 4; i++) {
          ctx.beginPath();
          ctx.arc(sx + (0.2 + 0.6 * hash(idx * 7 + i)) * px, sy + (0.2 + 0.6 * hash(idx * 13 + i)) * px, r, 0, Math.PI * 2);
          ctx.fill();
        }
      } else if (name === 'stabilized') {
        ctx.strokeStyle = 'rgba(190, 215, 240, 0.16)';
        ctx.lineWidth = 1;
        ctx.strokeRect(sx + 0.5, sy + 0.5, px - 1, px - 1);
      }
    }

    const boulders = c.boulders;
    if (boulders && boulders.length) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(sx, sy, px, px);
      ctx.clip();
      boulders.forEach((d, i) => {
        const r = Math.max(0.9, (d / this.site.cellSize) * px * 0.5);
        const bx = sx + (0.25 + 0.5 * hash(idx * 31 + i)) * px;
        const by = sy + (0.25 + 0.5 * hash(idx * 17 + i * 5 + 3)) * px;
        ctx.fillStyle = '#2b1f1a';
        ctx.beginPath();
        ctx.arc(bx, by, r, 0, Math.PI * 2);
        ctx.fill();
        if (r > 1.6) {
          ctx.fillStyle = 'rgba(235, 200, 170, 0.45)';
          ctx.beginPath();
          ctx.arc(bx - r * 0.3, by - r * 0.3, r * 0.4, 0, Math.PI * 2);
          ctx.fill();
        }
      });
      ctx.restore();
    }
  }
}
