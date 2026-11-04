// The top-down site map: terrain, the pad goal, fixtures, coverage, robots,
// and the environment (night, dust, a dark Earth link) laid over them.

import { drawGlyph, drawStation, drawDepot, drawBaseStation } from './glyphs.js';
import { statusColor, batteryColor } from './palette.js';
import { nightFactor } from '../format.js';
import { spread, placeLabels } from '../layout.js';

const MARGIN = 26;

export class MapView {
  constructor(canvas, model, terrain) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.model = model;
    this.terrain = terrain;
    this.site = model.site;
    this.dpr = 1;
    this.w = 0;
    this.h = 0;
    this.scale = 10;
    this.ox = 0;
    this.oy = 0;
    this.fitted = false;
    this.hitboxes = [];
    /** Screen space kept clear on each side when fitting (legend, tools). */
    this.insets = { left: 0, right: 0 };
  }

  resize() {
    const r = this.canvas.getBoundingClientRect();
    this.dpr = Math.max(1, Math.min(3, globalThis.devicePixelRatio || 1));
    this.w = Math.max(1, r.width);
    this.h = Math.max(1, r.height);
    this.canvas.width = Math.round(this.w * this.dpr);
    this.canvas.height = Math.round(this.h * this.dpr);
    // Keep the site fitted on resize until the user zooms or pans.
    if (!this.fitted || !this.userMoved) this.fit();
    this.syncTerrainScale();
  }

  fit() {
    const { width, height } = this.site;
    const byHeight = (this.h - 2 * MARGIN) / height;
    let left = this.insets.left;
    let right = this.insets.right;
    // Keep the legend clear unless that would shrink the map by over 30%.
    this.insetIgnored = false;
    if ((this.w - left - right - 2 * MARGIN) / width < 0.7 * Math.min(byHeight, (this.w - 2 * MARGIN) / width)) {
      left = 0;
      right = 0;
      this.insetIgnored = this.insets.left > 0;
    }
    const availW = this.w - left - right - 2 * MARGIN;
    this.scale = Math.max(1, Math.min(availW / width, byHeight));
    this.ox = left + MARGIN + (availW - width * this.scale) / 2;
    this.oy = (this.h - height * this.scale) / 2;
    this.fitted = true;
    this.userMoved = false;
    this.syncTerrainScale();
  }

  syncTerrainScale() {
    this.terrain.setScale(this.scale * this.dpr);
  }

  zoomAt(px, py, factor) {
    const s = Math.max(2, Math.min(120, this.scale * factor));
    const k = s / this.scale;
    this.ox = px - (px - this.ox) * k;
    this.oy = py - (py - this.oy) * k;
    this.scale = s;
    this.userMoved = true;
    this.syncTerrainScale();
  }

  panBy(dx, dy) {
    this.ox += dx;
    this.oy += dy;
    this.userMoved = true;
  }

  /** Screen position of grid point (x, y): cell (i, j) spans [i, i+1] × [j, j+1]; north is up. */
  toScreen(x, y) {
    return [this.ox + x * this.scale, this.oy + (this.site.height - y) * this.scale];
  }

  /** Screen position of the centre of the cell a robot at (x, y) occupies. */
  cellCentre(x, y) {
    return this.toScreen(x + 0.5, y + 0.5);
  }

  toCell(px, py) {
    return { x: (px - this.ox) / this.scale, y: this.site.height - (py - this.oy) / this.scale };
  }

  /** Robot index under a screen point, or null. */
  hit(px, py) {
    let best = null;
    let bestD = Infinity;
    for (const h of this.hitboxes) {
      const d = Math.hypot(px - h.x, py - h.y);
      if (d <= h.r + 3 && d < bestD) {
        best = h.index;
        bestD = d;
      }
    }
    return best;
  }

  /**
   * Draw one frame. `f` carries everything for the current time:
   * `{t, robots, selected, hover, comms, weather, clock, relays, selectedPath}`.
   */
  draw(f) {
    const ctx = this.ctx;
    const { width, height } = this.site;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.fillStyle = '#070a0e';
    ctx.fillRect(0, 0, this.w, this.h);

    const [x0, y0] = this.toScreen(0, height);
    const sw = width * this.scale;
    const sh = height * this.scale;
    ctx.imageSmoothingEnabled = this.terrain.px < this.scale * this.dpr * 0.75;
    ctx.drawImage(this.terrain.canvas, x0, y0, sw, sh);
    ctx.strokeStyle = 'rgba(255,255,255,0.18)';
    ctx.lineWidth = 1;
    ctx.strokeRect(x0 - 0.5, y0 - 0.5, sw + 1, sh + 1);

    this.drawGrid(ctx);
    this.drawGoal(ctx, f);
    this.drawCoverage(ctx, f);
    this.drawFixtures(ctx);
    this.drawEnvironment(ctx, f, x0, y0, sw, sh);
    this.drawPath(ctx, f);
    this.drawLinks(ctx, f);
    this.drawRobots(ctx, f);
    this.drawScale(ctx);
  }

  drawGrid(ctx) {
    const { width, height } = this.site;
    const step = this.scale >= 7 ? 10 : this.scale >= 3 ? 20 : 50;
    ctx.save();
    ctx.strokeStyle = 'rgba(255,255,255,0.07)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = step; x < width; x += step) {
      const [sx] = this.toScreen(x, 0);
      const [, top] = this.toScreen(0, height);
      const [, bottom] = this.toScreen(0, 0);
      ctx.moveTo(Math.round(sx) + 0.5, top);
      ctx.lineTo(Math.round(sx) + 0.5, bottom);
    }
    for (let y = step; y < height; y += step) {
      const [, sy] = this.toScreen(0, y);
      const [left] = this.toScreen(0, 0);
      const [right] = this.toScreen(width, 0);
      ctx.moveTo(left, Math.round(sy) + 0.5);
      ctx.lineTo(right, Math.round(sy) + 0.5);
    }
    ctx.stroke();
    ctx.fillStyle = 'rgba(200,215,230,0.45)';
    ctx.font = '10px ui-monospace, Menlo, monospace';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    for (let x = 0; x <= width; x += step) {
      const [sx, sy] = this.toScreen(x, 0);
      ctx.fillText(String(x), sx, sy + 4);
    }
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    for (let y = 0; y <= height; y += step) {
      const [sx, sy] = this.toScreen(0, y);
      ctx.fillText(String(y), sx - 5, sy);
    }
    ctx.restore();
  }

  drawGoal(ctx, f) {
    const pad = this.model.pad;
    if (!pad) return;
    const a = pad.spec.area;
    const [x0, y0] = this.toScreen(a.min.x, a.max.y + 1);
    const [x1, y1] = this.toScreen(a.max.x + 1, a.min.y);
    const counts = f.padCounts;
    const done = counts && counts.inSpec === counts.total;
    ctx.save();
    ctx.setLineDash(done ? [] : [7, 4]);
    ctx.lineWidth = 2;
    ctx.strokeStyle = done ? 'rgba(130, 240, 170, 0.95)' : 'rgba(235, 245, 255, 0.9)';
    ctx.shadowColor = 'rgba(0,0,0,0.6)';
    ctx.shadowBlur = 4;
    ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
    ctx.setLineDash([]);
    ctx.shadowBlur = 0;
    ctx.font = '600 11px system-ui, sans-serif';
    ctx.textBaseline = 'bottom';
    ctx.textAlign = 'left';
    const label = `PAD ${pad.goalId ?? ''}`.trim();
    const pct = counts ? ` ${Math.round((100 * counts.inSpec) / Math.max(1, counts.total))}%` : '';
    this.labelText(ctx, label + pct, x0 + 1, y0 - 4, done ? '#9ff5c0' : '#eef5ff');

    const berm = pad.spec.berm;
    if (berm?.cells) {
      ctx.strokeStyle = 'rgba(255, 196, 92, 0.9)';
      ctx.lineWidth = 1.5;
      ctx.setLineDash([3, 2]);
      for (const c of berm.cells) {
        const [bx, by] = this.toScreen(c.x, c.y + 1);
        ctx.strokeRect(bx + 1, by + 1, this.scale - 2, this.scale - 2);
      }
      ctx.setLineDash([]);
      const xs = berm.cells.map((c) => c.x);
      const ys = berm.cells.map((c) => c.y);
      const [lx, ly] = this.toScreen(Math.min(...xs), Math.max(...ys) + 1);
      ctx.font = '600 10px system-ui, sans-serif';
      this.labelText(ctx, `BERM ${berm.height_m} m`, lx, ly - 3, '#ffd28a');
    }
    ctx.restore();
  }

  labelText(ctx, text, x, y, color) {
    ctx.lineJoin = 'round';
    ctx.lineWidth = 3;
    ctx.strokeStyle = 'rgba(5, 8, 12, 0.85)';
    ctx.strokeText(text, x, y);
    ctx.fillStyle = color;
    ctx.fillText(text, x, y);
  }

  drawCoverage(ctx, f) {
    const bs = this.model.baseStation;
    const cs = this.site.cellSize;
    const circle = (x, y, rangeM, color) => {
      if (!rangeM) return;
      const [sx, sy] = this.cellCentre(x, y);
      const r = (rangeM / cs) * this.scale;
      ctx.beginPath();
      ctx.arc(sx, sy, r, 0, Math.PI * 2);
      ctx.fillStyle = color.replace('A', '0.06');
      ctx.fill();
      ctx.setLineDash([6, 5]);
      ctx.strokeStyle = color.replace('A', '0.65');
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.setLineDash([]);
    };
    ctx.save();
    if (bs) circle(bs.x, bs.y, bs.rangeM, 'rgba(86, 184, 255, A)');
    for (const r of f.relays ?? []) circle(r.x, r.y, r.rangeM, 'rgba(120, 230, 170, A)');
    ctx.restore();
  }

  drawFixtures(ctx) {
    const size = Math.max(12, Math.min(22, this.scale * 1.15));
    ctx.save();
    ctx.font = '600 10px system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    for (const s of this.model.stations) {
      const [x, y] = this.cellCentre(s.x, s.y);
      drawStation(ctx, x, y, size);
      this.labelText(ctx, s.id, x, y + size / 2 + 2, '#fde68a');
    }
    for (const d of this.model.depots) {
      const [x, y] = this.cellCentre(d.x, d.y);
      drawDepot(ctx, x, y, size);
      this.labelText(ctx, d.id, x, y + size / 2 + 2, '#c8d8ea');
    }
    const bs = this.model.baseStation;
    if (bs) {
      const [x, y] = this.cellCentre(bs.x, bs.y);
      drawBaseStation(ctx, x, y, size + 2);
      this.labelText(ctx, 'base station', x, y + size / 2 + 3, '#9fd0ff');
    }
    ctx.restore();
  }

  drawEnvironment(ctx, f, x0, y0, sw, sh) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(x0, y0, sw, sh);
    ctx.clip();
    const night = nightFactor(f.clock.phase);
    if (night > 0) {
      ctx.fillStyle = `rgba(6, 14, 34, ${0.36 * night})`;
      ctx.fillRect(x0, y0, sw, sh);
    }
    const tau = f.weather?.tau;
    if (tau != null && tau > 0.6) {
      // Suspended dust: a pale haze that washes out contrast.
      const a = Math.min(0.42, ((tau - 0.5) / 3.5) * 0.42);
      ctx.fillStyle = `rgba(214, 170, 128, ${a})`;
      ctx.fillRect(x0, y0, sw, sh);
    }
    if (f.comms?.link === 'down') {
      ctx.fillStyle = 'rgba(8, 10, 18, 0.5)';
      ctx.fillRect(x0, y0, sw, sh);
      ctx.strokeStyle = 'rgba(200, 60, 80, 0.10)';
      ctx.lineWidth = 6;
      ctx.beginPath();
      for (let d = -sh; d < sw; d += 26) {
        ctx.moveTo(x0 + d, y0 + sh);
        ctx.lineTo(x0 + d + sh, y0);
      }
      ctx.stroke();
    }
    ctx.restore();
  }

  drawPath(ctx, f) {
    const path = f.selectedPath;
    if (!path || path.length < 2) return;
    ctx.save();
    ctx.setLineDash([5, 4]);
    ctx.lineWidth = 2;
    ctx.strokeStyle = 'rgba(255,255,255,0.8)';
    ctx.shadowColor = 'rgba(0,0,0,0.7)';
    ctx.shadowBlur = 3;
    ctx.beginPath();
    path.forEach((p, i) => {
      const [x, y] = this.cellCentre(p.x, p.y);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();
    ctx.setLineDash([]);
    const end = path[path.length - 1];
    const [ex, ey] = this.cellCentre(end.x, end.y);
    ctx.fillStyle = 'rgba(255,255,255,0.9)';
    ctx.beginPath();
    ctx.arc(ex, ey, 3.5, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  /** Dashed links from robots repairing, towing or stripping other robots. */
  drawLinks(ctx, f) {
    if (!f.links?.length) return;
    const colors = { repair: '#7dd3fc', tow: '#c084fc', cannibalize: '#f87171' };
    ctx.save();
    ctx.lineWidth = 2;
    ctx.setLineDash([4, 3]);
    for (const l of f.links) {
      const a = f.robots[l.from];
      const b = f.robots[l.to];
      if (!a || !b) continue;
      // Only once they are together: before that the path shows the approach.
      if (Math.hypot(a.x - b.x, a.y - b.y) > 2.5) continue;
      const [ax, ay] = this.cellCentre(a.x, a.y);
      const [bx, by] = this.cellCentre(b.x, b.y);
      ctx.strokeStyle = colors[l.kind] ?? '#e2e8f0';
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
      ctx.stroke();
    }
    ctx.restore();
  }

  drawRobots(ctx, f) {
    const R = Math.max(8, Math.min(17, this.scale * 0.9));
    // Spread robots that share a spot so each stays legible.
    const placed = spread(
      f.robots.filter(Boolean).map((r) => ({ r, x: r.x, y: r.y })),
      (2.1 * R) / this.scale,
    );
    placed.sort((a, b) => (a.r.lost ? -1 : 0) - (b.r.lost ? -1 : 0) || (a.r.index === f.selected) - (b.r.index === f.selected));
    this.hitboxes = [];
    for (const p of placed) {
      const [sx, sy] = this.cellCentre(p.x, p.y);
      p.sx = sx;
      p.sy = sy;
      this.drawRobot(ctx, p.r, sx, sy, R, p.r.index === f.selected, p.r.index === f.hover);
      this.hitboxes.push({ index: p.r.index, x: sx, y: sy, r: R });
    }
    this.drawLabels(ctx, placed, R, f.selected, f.hover);
  }

  /** Robot labels, drawn after every robot so none is hidden (see layout.placeLabels). */
  drawLabels(ctx, placed, R, selected, hover) {
    ctx.save();
    ctx.font = '600 11px system-ui, sans-serif';
    const items = placed.map((p) => ({
      sx: p.sx,
      sy: p.sy,
      w: ctx.measureText(p.r.id).width + 4,
      priority: p.r.index === selected || p.r.index === hover,
    }));
    const spots = placeLabels(items, { R });
    placed.forEach((p, i) => {
      const spot = spots[i];
      if (!spot) return;
      ctx.textAlign = spot.align;
      ctx.textBaseline = spot.base;
      ctx.globalAlpha = p.r.lost ? 0.75 : 1;
      this.labelText(ctx, p.r.id, spot.x, spot.y, p.r.index === selected ? '#ffffff' : '#eef3f8');
    });
    ctx.restore();
  }

  drawRobot(ctx, r, x, y, R, selected, hover) {
    ctx.save();
    const color = statusColor(r.status);
    if (r.lost) ctx.globalAlpha = 0.75;

    if (selected || hover) {
      ctx.beginPath();
      ctx.arc(x, y, R + 5, 0, Math.PI * 2);
      ctx.strokeStyle = selected ? 'rgba(255,255,255,0.95)' : 'rgba(255,255,255,0.5)';
      ctx.lineWidth = 2;
      ctx.stroke();
      if (selected) {
        ctx.fillStyle = 'rgba(255,255,255,0.12)';
        ctx.fill();
      }
    }

    if (r.heading != null && !r.lost) {
      // Direction of travel: y grows north, screen y grows down.
      const a = -r.heading;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.moveTo(x + Math.cos(a) * (R + 6), y + Math.sin(a) * (R + 6));
      ctx.lineTo(x + Math.cos(a + 0.5) * (R + 1), y + Math.sin(a + 0.5) * (R + 1));
      ctx.lineTo(x + Math.cos(a - 0.5) * (R + 1), y + Math.sin(a - 0.5) * (R + 1));
      ctx.closePath();
      ctx.fill();
    }

    ctx.beginPath();
    ctx.arc(x, y, R, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.shadowColor = 'rgba(0,0,0,0.55)';
    ctx.shadowBlur = 5;
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.lineWidth = 2;
    ctx.strokeStyle = '#0b1118';
    if (!r.inCoverage) ctx.setLineDash([3, 2]);
    ctx.stroke();
    ctx.setLineDash([]);

    ctx.fillStyle = '#0b1118';
    drawGlyph(ctx, r.spec.main, x, y, R * 0.58);

    if (r.lost) {
      ctx.strokeStyle = '#f2f4f7';
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(x - R * 0.7, y - R * 0.7);
      ctx.lineTo(x + R * 0.7, y + R * 0.7);
      ctx.moveTo(x + R * 0.7, y - R * 0.7);
      ctx.lineTo(x - R * 0.7, y + R * 0.7);
      ctx.stroke();
    }

    if (!r.lost) {
      // Battery bar, then payload bar for robots that can carry.
      const bw = Math.max(18, R * 2.3);
      const bx = x - bw / 2;
      let by = y + R + 3;
      ctx.fillStyle = 'rgba(5,8,12,0.85)';
      ctx.fillRect(bx - 1, by - 1, bw + 2, 6);
      if (r.batteryFrac != null) {
        ctx.fillStyle = batteryColor(r.batteryFrac);
        ctx.fillRect(bx, by, bw * r.batteryFrac, 4);
      }
      by += 6;
      if (r.spec.haulKg > 0) {
        ctx.fillStyle = 'rgba(5,8,12,0.85)';
        ctx.fillRect(bx - 1, by - 1, bw + 2, 5);
        const frac = Math.min(1, (r.payload || 0) / r.spec.haulKg);
        ctx.fillStyle = '#d29a63';
        ctx.fillRect(bx, by, bw * frac, 3);
      }
    }

    // Fault badge.
    const faults = r.faults ?? [];
    if (faults.length && !r.lost) {
      const disabled = faults.some((fl) => fl.severity === 'disabled');
      const bxp = x + R * 0.78;
      const byp = y - R * 0.78;
      const br = Math.max(5.5, R * 0.42);
      ctx.beginPath();
      ctx.arc(bxp, byp, br, 0, Math.PI * 2);
      ctx.fillStyle = disabled ? '#f25c5c' : '#f5b942';
      ctx.fill();
      ctx.lineWidth = 1.5;
      ctx.strokeStyle = '#0b1118';
      ctx.stroke();
      ctx.fillStyle = '#0b1118';
      ctx.font = `800 ${Math.round(br * 1.45)}px system-ui, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(faults.length > 1 ? String(faults.length) : '!', bxp, byp + 0.5);
    }

    // Under repair or depot service: a wrench badge.
    if (r.servicing && !r.lost) {
      const wx = x + R * 0.8;
      const wy = y + R * 0.75;
      const wr = Math.max(5.5, R * 0.42);
      ctx.beginPath();
      ctx.arc(wx, wy, wr, 0, Math.PI * 2);
      ctx.fillStyle = '#0f2236';
      ctx.fill();
      ctx.strokeStyle = '#7dd3fc';
      ctx.lineWidth = 1.2;
      ctx.stroke();
      ctx.fillStyle = '#7dd3fc';
      drawGlyph(ctx, 'repair', wx, wy, wr * 0.75);
    }

    // Out of coverage: a crossed-out antenna.
    if (!r.inCoverage && !r.lost) {
      const cx = x - R * 0.8;
      const cy = y - R * 0.8;
      const cr = Math.max(5.5, R * 0.42);
      ctx.beginPath();
      ctx.arc(cx, cy, cr, 0, Math.PI * 2);
      ctx.fillStyle = '#1b2530';
      ctx.fill();
      ctx.strokeStyle = '#c9d6e6';
      ctx.lineWidth = 1.2;
      ctx.stroke();
      ctx.fillStyle = '#c9d6e6';
      drawGlyph(ctx, 'relay', cx, cy, cr * 0.7);
      ctx.strokeStyle = '#f25c5c';
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.moveTo(cx - cr * 0.75, cy + cr * 0.75);
      ctx.lineTo(cx + cr * 0.75, cy - cr * 0.75);
      ctx.stroke();
    }
    ctx.restore();
  }

  drawScale(ctx) {
    const cs = this.site.cellSize;
    const metresPerPx = cs / this.scale;
    const target = 110 * metresPerPx;
    const nice = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000].find((v) => v >= target) ?? 1000;
    const len = nice / metresPerPx;
    const x = 16;
    const y = 36;
    ctx.save();
    ctx.fillStyle = 'rgba(10, 14, 19, 0.78)';
    ctx.beginPath();
    ctx.roundRect(x - 8, 8, len + 54, 38, 6);
    ctx.fill();
    ctx.strokeStyle = '#dbe4ee';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(x, y - 4);
    ctx.lineTo(x, y);
    ctx.lineTo(x + len, y);
    ctx.lineTo(x + len, y - 4);
    ctx.stroke();
    ctx.font = '11px ui-monospace, Menlo, monospace';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'bottom';
    this.labelText(ctx, `${nice} m`, x + len / 2, y - 4, '#dbe4ee');
    ctx.textAlign = 'left';
    ctx.font = '600 12px system-ui, sans-serif';
    this.labelText(ctx, 'N ↑', x + len + 12, y + 1, '#dbe4ee');
    ctx.restore();
  }
}
