// The timeline scrubber: day/night strip, storm and link-down bands, event
// markers and the playhead. The static part is drawn once per resize.

import { SUNRISE, SUNSET, solClock, fmtDuration } from '../format.js';
import { theme } from './theme.js';

const PAD_X = 10;
const ROW_MARK = [4, 16];
const ROW_BAND = [19, 31];
const ROW_DAY = [33, 41];
const ROW_TICK = 44;

function markerColor(mk) {
  if (mk.type === 'fault') return mk.sev === 'disabled' ? theme.err : theme.warn;
  return { failure: theme.failure, lost: theme.ink, end: theme.ok }[mk.type] ?? theme.grey;
}

export class Timeline {
  constructor(canvas, { onSeek, onHover } = {}) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.model = null;
    this.onSeek = onSeek;
    this.onHover = onHover;
    this.layer = document.createElement('canvas');
    this.w = 0;
    this.h = 0;
    this.dpr = 1;
    this.dragging = false;
    this.hoverX = null;
    canvas.addEventListener('pointerdown', (e) => {
      if (!this.model) return;
      this.dragging = true;
      canvas.setPointerCapture(e.pointerId);
      this.onSeek?.(this.timeAt(e.offsetX));
    });
    canvas.addEventListener('pointermove', (e) => {
      if (!this.model) return;
      this.hoverX = e.offsetX;
      if (this.dragging) this.onSeek?.(this.timeAt(e.offsetX));
      this.onHover?.(this.describe(e.offsetX), e.offsetX);
    });
    canvas.addEventListener('pointerup', (e) => {
      this.dragging = false;
      canvas.releasePointerCapture(e.pointerId);
    });
    canvas.addEventListener('pointerleave', () => {
      this.hoverX = null;
      this.onHover?.(null);
    });
  }

  setModel(model) {
    this.model = model;
    this.dragging = false;
    this.hoverX = null;
  }

  get t0() {
    return this.model.t0;
  }

  get t1() {
    return Math.max(this.model.tEnd, this.model.t0 + 1);
  }

  xAt(t) {
    return PAD_X + ((t - this.t0) / (this.t1 - this.t0)) * (this.w - 2 * PAD_X);
  }

  timeAt(x) {
    const f = Math.min(1, Math.max(0, (x - PAD_X) / Math.max(1, this.w - 2 * PAD_X)));
    return this.t0 + f * (this.t1 - this.t0);
  }

  resize() {
    if (!this.model) return;
    const r = this.canvas.getBoundingClientRect();
    this.dpr = Math.max(1, Math.min(3, globalThis.devicePixelRatio || 1));
    this.w = Math.max(1, r.width);
    this.h = Math.max(1, r.height);
    for (const c of [this.canvas, this.layer]) {
      c.width = Math.round(this.w * this.dpr);
      c.height = Math.round(this.h * this.dpr);
    }
    this.drawStatic();
  }

  drawStatic() {
    const ctx = this.layer.getContext('2d');
    const m = this.model;
    const sol = m.info.solLength;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    // The page paints the background (style.css), so the strip sits on the card.
    ctx.clearRect(0, 0, this.w, this.h);

    // Day and night.
    const firstSol = Math.floor(this.t0 / sol);
    const lastSol = Math.ceil(this.t1 / sol);
    const nSols = lastSol - firstSol;
    for (let s = firstSol; s < lastSol; s++) {
      const day0 = this.xAt(s * sol + SUNRISE * sol);
      const day1 = this.xAt(s * sol + SUNSET * sol);
      const n0 = this.xAt(s * sol);
      const n1 = this.xAt((s + 1) * sol);
      ctx.fillStyle = theme.night;
      ctx.fillRect(Math.max(PAD_X, n0), ROW_DAY[0], Math.min(this.w - PAD_X, n1) - Math.max(PAD_X, n0), ROW_DAY[1] - ROW_DAY[0]);
      ctx.fillStyle = theme.day;
      const a = Math.max(PAD_X, day0);
      const b = Math.min(this.w - PAD_X, day1);
      if (b > a) ctx.fillRect(a, ROW_DAY[0], b - a, ROW_DAY[1] - ROW_DAY[0]);
    }

    // Bands: dust storms (amber) and Earth link down (red).
    const band = (items, color, label) => {
      for (const it of items) {
        const a = Math.max(PAD_X, this.xAt(it.start));
        const b = Math.min(this.w - PAD_X, this.xAt(Math.min(it.end, this.t1)));
        ctx.fillStyle = color;
        ctx.fillRect(a, ROW_BAND[0], Math.max(2, b - a), ROW_BAND[1] - ROW_BAND[0]);
        if (b - a > 60) {
          ctx.fillStyle = '#fff';
          ctx.font = `500 9.5px ${theme.font}`;
          ctx.textBaseline = 'middle';
          ctx.textAlign = 'left';
          ctx.fillText(label(it), a + 4, (ROW_BAND[0] + ROW_BAND[1]) / 2 + 0.5);
        }
      }
    };
    band(m.weather.storms.items, theme.bandStorm, (s) => `dust storm τ ${s.peakTau.toFixed(1)}`);
    band(m.comms.down.items, theme.bandLink, (d) => `link down${d.cause ? ` (${d.cause})` : ''}`);

    // Markers.
    for (const mk of m.markers) {
      const x = Math.round(this.xAt(mk.t)) + 0.5;
      const color = markerColor(mk);
      ctx.strokeStyle = color;
      ctx.fillStyle = color;
      ctx.lineWidth = 1.5;
      if (mk.type === 'end') {
        ctx.beginPath();
        ctx.moveTo(x, ROW_MARK[0]);
        ctx.lineTo(x, ROW_DAY[1]);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(x, ROW_MARK[0]);
        ctx.lineTo(x - 8, ROW_MARK[0] + 4);
        ctx.lineTo(x, ROW_MARK[0] + 8);
        ctx.fill();
      } else if (mk.type === 'lost') {
        ctx.beginPath();
        ctx.moveTo(x - 4, ROW_MARK[0] + 2);
        ctx.lineTo(x + 4, ROW_MARK[1] - 2);
        ctx.moveTo(x + 4, ROW_MARK[0] + 2);
        ctx.lineTo(x - 4, ROW_MARK[1] - 2);
        ctx.stroke();
      } else if (mk.type === 'fault') {
        ctx.beginPath();
        ctx.moveTo(x, ROW_MARK[0] + 1);
        ctx.lineTo(x + 4.5, ROW_MARK[1] - 1);
        ctx.lineTo(x - 4.5, ROW_MARK[1] - 1);
        ctx.closePath();
        ctx.fill();
      } else {
        ctx.beginPath();
        ctx.moveTo(x, ROW_MARK[0] + 3);
        ctx.lineTo(x, ROW_MARK[1] - 1);
        ctx.stroke();
      }
    }

    // Sol ticks and labels, thinned to fit.
    const pxPerSol = (this.w - 2 * PAD_X) / Math.max(1e-9, (this.t1 - this.t0) / sol);
    const every = [1, 2, 5, 10, 20, 50, 100].find((k) => k * pxPerSol >= 46) ?? 100;
    ctx.fillStyle = theme.grey;
    ctx.strokeStyle = theme.tick;
    ctx.font = `10px ${theme.font}`;
    ctx.textBaseline = 'top';
    ctx.textAlign = 'center';
    ctx.lineWidth = 1;
    // A label is skipped where it would overlap one already drawn.
    const taken = [];
    const label = (text, x) => {
      const cx = Math.min(this.w - 24, Math.max(24, x));
      const half = ctx.measureText(text).width / 2 + 4;
      if (taken.some(([a, b]) => cx - half < b && cx + half > a)) return;
      taken.push([cx - half, cx + half]);
      ctx.fillText(text, cx, ROW_TICK + 3);
    };
    for (let s = Math.ceil(this.t0 / sol); s <= Math.floor(this.t1 / sol); s++) {
      const x = Math.round(this.xAt(s * sol)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(x, ROW_DAY[0]);
      ctx.lineTo(x, ROW_TICK + 2);
      ctx.stroke();
      if (s % every === 0) label(`Sol ${s}`, x);
    }
    if (nSols <= 3) {
      // Short runs: label hours too.
      const hour = sol / 24;
      ctx.fillStyle = theme.faint;
      for (let k = Math.ceil(this.t0 / hour); k * hour <= this.t1; k++) {
        if (k % 24 === 0 || k % 6 !== 0) continue;
        label(`${String(k % 24).padStart(2, '0')}:00`, this.xAt(k * hour));
      }
    }
  }

  /** Nearby markers and bands at screen x, for the hover tooltip. */
  describe(x) {
    const t = this.timeAt(x);
    const m = this.model;
    const lines = [];
    for (const mk of m.markers) if (Math.abs(this.xAt(mk.t) - x) <= 4) lines.push(mk.label);
    const storm = m.weather.storms.at(t);
    if (storm) lines.push(`dust storm, peak optical depth ${storm.peakTau.toFixed(2)}`);
    const down = m.comms.down.at(t);
    if (down) lines.push(`Earth link down${down.cause ? ` (${down.cause})` : ''} for ${fmtDuration(down.end - down.start, m.info.solLength)}`);
    return { t, clock: solClock(t, m.info.solLength).text, lines: lines.slice(0, 6) };
  }

  draw(t) {
    const ctx = this.ctx;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.drawImage(this.layer, 0, 0);
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    const x = this.xAt(t);
    ctx.fillStyle = theme.elapsed;
    ctx.fillRect(PAD_X, ROW_MARK[0], Math.max(0, x - PAD_X), ROW_DAY[1] - ROW_MARK[0]);
    if (this.hoverX != null && !this.dragging) {
      ctx.strokeStyle = theme.hover;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(Math.round(this.hoverX) + 0.5, ROW_MARK[0]);
      ctx.lineTo(Math.round(this.hoverX) + 0.5, ROW_DAY[1]);
      ctx.stroke();
    }
    ctx.strokeStyle = theme.ink;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(x, ROW_MARK[0] - 2);
    ctx.lineTo(x, ROW_DAY[1] + 2);
    ctx.stroke();
    ctx.fillStyle = theme.ink;
    ctx.beginPath();
    ctx.moveTo(x - 5, ROW_MARK[0] - 4);
    ctx.lineTo(x + 5, ROW_MARK[0] - 4);
    ctx.lineTo(x, ROW_MARK[0] + 2);
    ctx.closePath();
    ctx.fill();
  }
}
