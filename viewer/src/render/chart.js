// The site power history chart: storage fraction as an area, solar, fission
// and demand as lines, night and dust-storm periods shaded behind them, and a
// playhead. The static part is drawn once per resize.

import { SUNRISE, SUNSET } from '../format.js';

const PAD = { l: 2, r: 2, t: 4, b: 3 };

export class PowerChart {
  constructor(canvas, model) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.model = model;
    this.layer = document.createElement('canvas');
    this.w = 0;
    this.h = 0;
    this.dpr = 1;
  }

  get t0() {
    return this.model.t0;
  }

  get t1() {
    return Math.max(this.model.tEnd, this.model.t0 + 1);
  }

  x(t) {
    return PAD.l + ((t - this.t0) / (this.t1 - this.t0)) * (this.w - PAD.l - PAD.r);
  }

  resize() {
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
    const p = m.power;
    const sol = m.info.solLength;
    const top = PAD.t;
    const bottom = this.h - PAD.b;
    const yFrac = (f) => bottom - f * (bottom - top);
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.clearRect(0, 0, this.w, this.h);
    ctx.fillStyle = '#0b1016';
    ctx.fillRect(0, 0, this.w, this.h);

    for (let s = Math.floor(this.t0 / sol); s < Math.ceil(this.t1 / sol); s++) {
      for (const [a, b] of [
        [s * sol, s * sol + SUNRISE * sol],
        [s * sol + SUNSET * sol, (s + 1) * sol],
      ]) {
        const x0 = Math.max(PAD.l, this.x(a));
        const x1 = Math.min(this.w - PAD.r, this.x(b));
        if (x1 > x0) {
          ctx.fillStyle = 'rgba(40, 60, 100, 0.22)';
          ctx.fillRect(x0, top, x1 - x0, bottom - top);
        }
      }
    }
    for (const st of m.weather.storms.items) {
      const x0 = Math.max(PAD.l, this.x(st.start));
      const x1 = Math.min(this.w - PAD.r, this.x(Math.min(st.end, this.t1)));
      ctx.fillStyle = 'rgba(217, 137, 61, 0.16)';
      ctx.fillRect(x0, top, Math.max(1, x1 - x0), bottom - top);
    }
    ctx.strokeStyle = 'rgba(255,255,255,0.07)';
    ctx.lineWidth = 1;
    for (const f of [0.5, 1]) {
      ctx.beginPath();
      ctx.moveTo(PAD.l, Math.round(yFrac(f)) + 0.5);
      ctx.lineTo(this.w - PAD.r, Math.round(yFrac(f)) + 0.5);
      ctx.stroke();
    }
    if (!p.storage.length) {
      ctx.fillStyle = '#5d6d82';
      ctx.font = '11px system-ui, sans-serif';
      ctx.fillText('No power data in this log', 8, this.h / 2);
      return;
    }

    const series = (s, map) => {
      const pts = [];
      for (let i = 0; i < s.length; i++) pts.push([this.x(s.t[i]), map(s.v[i])]);
      if (pts.length) pts.push([this.x(this.t1), pts[pts.length - 1][1]]);
      if (pts.length && s.t[0] > this.t0) pts.unshift([this.x(this.t0), pts[0][1]]);
      return pts;
    };
    const cap = p.capacity || p.storage.max() || 1;
    const storage = series(p.storage, (v) => yFrac(Math.min(1, v / cap)));
    ctx.beginPath();
    ctx.moveTo(storage[0][0], bottom);
    for (const [x, y] of storage) ctx.lineTo(x, y);
    ctx.lineTo(storage[storage.length - 1][0], bottom);
    ctx.closePath();
    ctx.fillStyle = 'rgba(56, 198, 181, 0.22)';
    ctx.fill();
    ctx.beginPath();
    storage.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.strokeStyle = '#38c6b5';
    ctx.lineWidth = 1.5;
    ctx.stroke();

    const peak = Math.max(
      1,
      Number(m.info.power?.solar_peak_w) || 0,
      p.solar.max() ?? 0,
      p.demand.max() ?? 0,
      p.fission.max() ?? 0,
    );
    const yPow = (v) => bottom - (Math.max(0, v) / peak) * (bottom - top) * 0.95;
    const line = (s, color, dash) => {
      if (!s.length) return;
      const pts = series(s, yPow);
      ctx.beginPath();
      pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      ctx.setLineDash(dash ?? []);
      ctx.strokeStyle = color;
      ctx.lineWidth = 1.3;
      ctx.stroke();
      ctx.setLineDash([]);
    };
    line(p.fission, 'rgba(154, 168, 186, 0.75)', [2, 2]);
    line(p.solar, '#f7c948');
    line(p.demand, 'rgba(244, 114, 182, 0.9)');
    this.peak = peak;
  }

  draw(t) {
    const ctx = this.ctx;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.drawImage(this.layer, 0, 0);
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    const x = Math.round(this.x(t)) + 0.5;
    ctx.strokeStyle = 'rgba(255,255,255,0.85)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, this.h);
    ctx.stroke();
  }
}
