// Sidebar panels and the map legend: plain DOM, updated a few times a second.

import {
  fmtDuration,
  fmtPower,
  fmtEnergy,
  fmtMass,
  fmtPct,
  fmtNumber,
  fmtCoord,
  describeCommand,
  humanize,
  solClock,
  SUNRISE,
  SUNSET,
  formatMetric,
} from '../format.js';
import { CATEGORIES } from '../feed.js';
import { SURFACES } from '../site.js';
import { SURFACE_RGB, SURFACE_LABELS, STATUS_COLORS, STATUS_LABELS, statusColor, batteryColor, rgb } from '../render/palette.js';
import { drawGlyph, drawStation, drawDepot, drawBaseStation } from '../render/glyphs.js';

const $ = (id) => document.getElementById(id);

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function setText(el, text) {
  if (el && el.textContent !== text) el.textContent = text;
}

function setHtml(el, html) {
  if (el && el._html !== html) {
    el.innerHTML = html;
    el._html = html;
  }
}

const FEED_WINDOW = 160;

export class Panels {
  constructor({ onSeek, onSelect }) {
    this.model = null;
    this.onSeek = onSeek;
    this.onSelect = onSelect;
    this.filters = new Set(CATEGORIES.map(([k]) => k));
    this.feedKey = null;
    this.buildChips();
    this.buildLegend();
    $('feed').addEventListener('click', (e) => {
      const li = e.target.closest('li[data-i]');
      const model = this.model;
      if (!li || !model) return;
      const item = model.feed.items[Number(li.dataset.i)];
      if (item.robot && model.robotIndex.has(item.robot)) this.onSelect?.(model.robotIndex.get(item.robot), false);
      this.onSeek?.(item.t);
    });
    $('fleet').addEventListener('click', (e) => {
      const tr = e.target.closest('tr[data-i]');
      if (tr) this.onSelect?.(Number(tr.dataset.i), false);
    });
  }

  setModel(model) {
    this.model = model;
    this.feedKey = null;
    this.runKey = null;
  }

  buildChips() {
    const box = $('feed-chips');
    box.innerHTML = '';
    for (const [key, label] of CATEGORIES) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'chip on';
      b.textContent = label;
      b.addEventListener('click', () => {
        if (this.filters.has(key)) this.filters.delete(key);
        else this.filters.add(key);
        b.classList.toggle('on', this.filters.has(key));
        this.feedKey = null;
        this.updateFeed(this.lastT ?? 0);
      });
      box.appendChild(b);
    }
  }

  buildLegend() {
    const el = $('legend');
    const surf = SURFACES.map(
      (s) => `<div class="item"><span class="sq" style="background:${rgb(SURFACE_RGB[s])}"></span>${SURFACE_LABELS[s]}</div>`,
    ).join('');
    const statuses = Object.keys(STATUS_COLORS)
      .map((s) => `<div class="item"><span class="dot" style="background:${STATUS_COLORS[s]}"></span>${STATUS_LABELS[s]}</div>`)
      .join('');
    const icons = [
      ['station', 'Charge station'],
      ['depot', 'Depot'],
      ['base', 'Base station'],
      ['fault', 'Fault (amber/red)'],
      ['nocov', 'Out of coverage'],
      ['pad', 'Pad / berm target'],
    ];
    const iconHtml = icons.map(([k, label]) => `<div class="item"><canvas width="28" height="28" style="width:14px;height:14px" data-icon="${k}"></canvas>${label}</div>`).join('');
    el.innerHTML = `<h3>Legend</h3><div class="legend-body"><div class="group">${surf}</div><div class="group">${statuses}</div><div class="group">${iconHtml}</div></div>`;
    el.querySelector('h3').addEventListener('click', () => el.classList.toggle('collapsed'));
    for (const c of el.querySelectorAll('canvas[data-icon]')) {
      const ctx = c.getContext('2d');
      ctx.scale(2, 2);
      const k = c.dataset.icon;
      if (k === 'station') drawStation(ctx, 7, 7, 12);
      else if (k === 'depot') drawDepot(ctx, 7, 7, 12);
      else if (k === 'base') drawBaseStation(ctx, 7, 7, 12);
      else if (k === 'fault') {
        ctx.fillStyle = '#f5b942';
        ctx.beginPath();
        ctx.arc(7, 7, 6, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = '#0b1118';
        ctx.font = '800 9px system-ui';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText('!', 7, 7.5);
      } else if (k === 'nocov') {
        ctx.fillStyle = '#1b2530';
        ctx.beginPath();
        ctx.arc(7, 7, 6, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = '#c9d6e6';
        drawGlyph(ctx, 'relay', 7, 7, 4);
        ctx.strokeStyle = '#f25c5c';
        ctx.lineWidth = 1.4;
        ctx.beginPath();
        ctx.moveTo(3, 11);
        ctx.lineTo(11, 3);
        ctx.stroke();
      } else if (k === 'pad') {
        ctx.setLineDash([3, 2]);
        ctx.strokeStyle = '#eef5ff';
        ctx.lineWidth = 1.5;
        ctx.strokeRect(1.5, 1.5, 11, 11);
        ctx.strokeStyle = '#ffc45c';
        ctx.strokeRect(4.5, 4.5, 5, 5);
      }
    }
  }

  update(t, frame) {
    this.lastT = t;
    this.updateMission(t, frame);
    this.updatePower(t);
    this.updateWeather(t, frame);
    this.updateComms(t, frame);
    this.updateFeed(t);
    this.updateFleet(t, frame);
    this.updateRun(t, frame);
  }

  updateMission(t, frame) {
    const m = this.model;
    const p = frame.padCounts;
    if (!p) {
      setHtml($('pad-numbers'), '<span class="sub">No landing-pad goal in this log.</span>');
      setText($('pad-tag'), '');
      $('pipeline').hidden = true;
      setHtml($('pad-detail'), '');
      return;
    }
    $('pipeline').hidden = false;
    const frac = p.inSpec / Math.max(1, p.total);
    const tag = $('pad-tag');
    const done = m.info.endReason === 'goals_complete' && t >= m.tEnd;
    setText(tag, done ? 'complete' : frac > 0 ? 'building' : t < m.pad.arrivedT ? 'no goal yet' : 'not started');
    tag.className = `tag ${done ? 'ok' : ''}`;
    setHtml(
      $('pad-numbers'),
      `<span class="big">${fmtPct(frac)}</span><span class="sub">${p.inSpec} of ${p.total} cells in spec<br>${p.verified} verified by scan</span>`,
    );
    const counts = frame.padSurfaces;
    const total = Math.max(1, p.total);
    setHtml(
      $('pipeline'),
      SURFACES.map((s, i) => (counts[i] ? `<span style="width:${(100 * counts[i]) / total}%;background:${rgb(SURFACE_RGB[s])}" title="${SURFACE_LABELS[s]}: ${counts[i]}"></span>` : '')).join(''),
    );
    const parts = SURFACES.map((s, i) => (counts[i] ? `<span class="k"><i class="sq" style="background:${rgb(SURFACE_RGB[s])}"></i>${SURFACE_LABELS[s].toLowerCase()} <b>${counts[i]}</b></span>` : '')).filter(Boolean);
    if (p.bermTotal) parts.push(`<span class="k">berm <b>${p.berm}/${p.bermTotal}</b></span>`);
    setHtml($('pad-detail'), parts.join(''));
  }

  updatePower(t) {
    const m = this.model;
    const pw = m.powerAt(t);
    const tag = $('power-tag');
    const mode = m.info.power?.mode;
    setText(tag, mode ? humanize(mode) : pw ? (pw.source === 'truth' ? 'ground truth' : 'site reports') : 'no data');
    if (!pw) {
      setText($('storage-pct'), '–');
      return;
    }
    setText($('storage-pct'), pw.frac != null ? fmtPct(pw.frac) : fmtEnergy(pw.storage));
    const fill = $('storage-fill');
    fill.style.width = `${(pw.frac ?? 0) * 100}%`;
    fill.className = `gauge-fill${pw.frac != null && pw.frac < 0.02 ? ' empty' : pw.frac != null && pw.frac < 0.2 ? ' low' : ''}`;
    setText($('storage-wh'), `${fmtEnergy(pw.storage)}${pw.capacity ? ` of ${fmtEnergy(pw.capacity)} stored` : ''}`);
    setText($('solar-w'), fmtPower(pw.solar));
    setText($('fission-w'), fmtPower(pw.fission));
    setText($('demand-w'), fmtPower(pw.demand));
  }

  updateWeather(t, frame) {
    const w = frame.weather;
    const clock = frame.clock;
    setHtml(
      $('weather-state'),
      w.storm ? '<span class="pill storm">DUST STORM</span>' : w.tau == null ? '<span class="muted">No reports</span>' : '<span class="pill clear">CLEAR</span>',
    );
    setText($('tau'), w.tau != null ? fmtNumber(w.tau, 2) : '–');
    $('tau-fill').style.width = `${Math.min(100, ((w.tau ?? 0) / 4.5) * 100)}%`;
    setText($('temp'), w.temp != null ? `${Math.round(w.temp)} °C` : '–');
    const phase = clock.phase;
    let sky;
    if (clock.daylight) sky = `day, sunset in ${fmtDuration((SUNSET - phase) * this.model.info.solLength)}`;
    else {
      const toRise = (phase < SUNRISE ? SUNRISE - phase : 1 - phase + SUNRISE) * this.model.info.solLength;
      sky = `night, sunrise in ${fmtDuration(toRise)}`;
    }
    setText($('sky'), sky);
  }

  updateComms(t, frame) {
    const c = frame.comms;
    const m = this.model;
    setHtml(
      $('link-state'),
      !c.known ? '<span class="muted">No reports</span>' : c.link === 'down' ? '<span class="pill down">DOWN</span>' : '<span class="pill up">UP</span>',
    );
    setText($('delay'), c.delay != null ? fmtDuration(c.delay) : '–');
    setText($('link-cause'), c.cause ? humanize(c.cause) : '–');
    const restoreRow = $('restore-row');
    if (c.link === 'down') {
      restoreRow.hidden = false;
      const back = c.restore ?? (c.until <= m.tEnd ? c.until : null);
      setText($('restore'), back != null ? `in ${fmtDuration(back - t, m.info.solLength)}` : 'unknown');
    } else {
      restoreRow.hidden = true;
    }
    $('lightpath').classList.toggle('down', c.link === 'down');
    const msgs = m.inFlight(t).slice(0, 24);
    setHtml(
      $('lightpath-track'),
      msgs
        .map((q) => {
          const f = q.dir === 'down' ? q.frac : 1 - q.frac;
          return `<span class="msg ${q.dir}${q.queued ? ' queued' : ''}" style="left:${(f * 100).toFixed(1)}%" title="${esc(q.dir === 'down' ? 'to Earth' : 'to Mars')}: ${esc(q.message?.type ?? 'message')}${q.queued ? ' (waiting for the link)' : ''}"></span>`;
        })
        .join(''),
    );
  }

  updateFeed(t) {
    const m = this.model;
    const last = m.feedIndexAt(t);
    const key = `${last}|${[...this.filters].join(',')}`;
    if (key === this.feedKey) return;
    this.feedKey = key;
    const sol = m.info.solLength;
    const out = [];
    for (let i = last; i >= 0 && out.length < FEED_WINDOW; i--) {
      const it = m.feed.items[i];
      if (!this.filters.has(it.cat)) continue;
      const robot = it.robot ? `<span class="rb" style="color:${this.robotColor(it.robot, t)}">${esc(it.robot)}</span>` : '';
      const id = it.cat === 'command' && it.id ? `<span class="id">${esc(it.id)}</span>` : '';
      const note = it.note ? ` <span class="note">${esc(it.note)}</span>` : '';
      const late = it.late ? ` <span class="late">(reported ${fmtDuration(it.late, sol)} late)</span>` : '';
      const time = solClock(it.et, sol);
      out.push(
        `<li data-i="${i}" class="cat-${it.cat} sev-${it.sev}"><span class="ft" title="Sol ${time.sol}, ${String(time.hh).padStart(2, '0')}:${String(time.mm).padStart(2, '0')} local time">S${time.sol} ${String(time.hh).padStart(2, '0')}:${String(time.mm).padStart(2, '0')}</span><span class="fx">${robot}${id}${esc(it.text)}${note}${late}</span></li>`,
      );
    }
    setHtml($('feed'), out.length ? out.join('') : '<li class="empty">Nothing yet. Press play or drag the timeline.</li>');
  }

  robotColor(id, t) {
    const i = this.model.robotIndex.get(id);
    if (i == null) return '#dbe4ee';
    const st = this.model.robotAt(i, t);
    return st ? statusColor(st.status) : '#dbe4ee';
  }

  updateFleet(t, frame) {
    const m = this.model;
    const rows = frame.robots
      .map((r, i) => {
        if (!r) return '';
        const frac = r.batteryFrac;
        const cmd = r.command ? `${r.command.id} ${describeCommand(r.command.cmd)}` : '';
        const badges = (r.faults.length ? ` <span title="${r.faults.length} fault(s)" style="color:${r.faults.some((f) => f.severity === 'disabled') ? '#f25c5c' : '#f5b942'}">⚠</span>` : '') + (r.inCoverage ? '' : ' <span title="out of coverage" style="color:#8b9bb0">⌀</span>');
        return `<tr data-i="${i}" class="${frame.selected === i ? 'sel' : ''}"><td><span class="status-dot" style="background:${statusColor(r.status)}"></span> <b>${esc(r.id)}</b>${badges}</td><td>${esc(humanize(r.status))}</td><td class="bat"><span class="minibar"><i style="width:${((frac ?? 0) * 100).toFixed(0)}%;background:${batteryColor(frac)}"></i></span>${frac != null ? fmtPct(frac) : '–'}</td><td class="cmd" title="${esc(cmd)}">${esc(cmd || '–')}</td></tr>`;
      })
      .join('');
    setHtml($('fleet'), `<tr><th>Robot</th><th>Status</th><th>Battery</th><th>Command</th></tr>${rows}`);

    const sel = frame.selected != null ? frame.robots[frame.selected] : null;
    if (!sel) {
      setHtml($('robot-detail'), '<div class="hint">Click a robot on the map or in this table for its details.</div>');
      return;
    }
    const spec = sel.spec;
    const cmd = sel.command;
    const faults = sel.faults.length
      ? `<ul class="faults">${sel.faults.map((f) => `<li>${esc(humanize(f.component))} <b style="color:${f.severity === 'disabled' ? '#f25c5c' : '#f5b942'}">${esc(f.severity)}</b> (${esc(humanize(f.cause))}${f.field_repairable === false ? ', depot only' : ''}) since ${esc(solClock(f.since_t, m.info.solLength).text)}</li>`).join('')}</ul>`
      : 'none';
    const contact = sel.telemetryT != null && t - sel.telemetryT > 1 ? `last contact ${fmtDuration(t - sel.telemetryT, m.info.solLength)} ago` : 'current';
    const h = sel.health;
    const health = h
      ? `slip ${fmtPct(h.wheel_slip, 1)}, motor ${fmtNumber(h.motor_current_ratio, 2)}×, dust ${fmtPct(h.dust_coverage)}, ${Math.round(h.temperature_c)} °C`
      : '–';
    setHtml(
      $('robot-detail'),
      `<div class="head"><span class="status-dot" style="background:${statusColor(sel.status)}"></span><h3>${esc(sel.id)}</h3><span class="muted">${esc(spec.model ?? '')}</span></div>
      <dl>
        <dt>Status</dt><dd>${esc(humanize(sel.status))}</dd>
        <dt>Position</dt><dd>${fmtCoord({ x: Math.round(sel.x), y: Math.round(sel.y) })}</dd>
        <dt>Battery</dt><dd>${fmtEnergy(sel.battery)}${spec.batteryWh ? ` of ${fmtEnergy(spec.batteryWh)} (${fmtPct(sel.batteryFrac)})` : ''}</dd>
        <dt>Payload</dt><dd>${spec.haulKg ? `${fmtMass(sel.payload)} of ${fmtMass(spec.haulKg)}` : 'cannot carry'}</dd>
        <dt>Command</dt><dd>${cmd ? `${esc(cmd.id)}: ${esc(describeCommand(cmd.cmd))}${cmd.est ? ` (est. ${fmtDuration(cmd.est)})` : ''}` : 'none'}</dd>
        <dt>Capabilities</dt><dd>${esc(spec.caps.join(', ') || '–')}</dd>
        <dt>Top speed</dt><dd>${fmtNumber(spec.speed, 2)} m/s</dd>
        <dt>Faults</dt><dd>${faults}</dd>
        <dt>Coverage</dt><dd>${sel.inCoverage ? 'in coverage' : 'out of coverage'}, telemetry ${contact}</dd>
        <dt>Health</dt><dd>${health}</dd>
      </dl>`,
    );
  }

  updateRun(t, frame) {
    const m = this.model;
    if (this.runKey === Math.floor(t / 60)) return;
    this.runKey = Math.floor(t / 60);
    const i = m.info;
    const dl = (rows) => `<dl>${rows.filter((r) => r && r[1] != null).map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')}</dl>`;
    const sol = i.solLength;
    const soft = (s) => (s ? `${s.name} ${s.version}` : '–');
    let html = `<h3>Run</h3>${dl([
      ['Coordinator', soft(i.coordinator)],
      ['Simulator', soft(i.simulator)],
      ['Protocol', i.protocol],
      ['Scenario', i.scenarioId ?? '–'],
      ['Run id', i.runId ?? '–'],
      ['Seed', i.seed != null ? String(i.seed) : '–'],
      ['End', m.describeEnd()],
      ['Time limit', i.timeLimitT != null ? `${fmtNumber(i.timeLimitT / sol, 1)} sols` : null],
      ['Power system', i.power ? `${humanize(i.power.mode)}: fission ${fmtPower(i.power.fission_w)}, solar ${fmtPower(i.power.solar_peak_w)}, ${fmtEnergy(i.power.storage_capacity_wh)}` : null],
    ])}`;
    const pad = frame.padCounts;
    if (pad) {
      html += `<h3>Landing pad now</h3>${dl([
        ['Cells in spec', `${pad.inSpec} / ${pad.total}`],
        ['Verified by scan', `${pad.verified} / ${pad.total}`],
        ['Berm cells done', pad.bermTotal ? `${pad.berm} / ${pad.bermTotal}` : 'no berm'],
        ['Steepest slope', `${fmtNumber(m.pad.progress.maxSlopeDeg(t), 2)}° (limit ${m.pad.spec.max_slope_deg}°)`],
        ['Target elevation', `${fmtNumber(m.pad.spec.target_elevation_m, 2)} m ± ${m.pad.spec.surface_tolerance_m} m`],
      ])}`;
    }
    const s = m.stats;
    html += `<h3>Activity (whole run)</h3>${dl([
      ['Decision points', fmtNumber(m.decisionTimes.length)],
      ['Commands', fmtNumber(s.commands)],
      ['Completed', fmtNumber(s.completed)],
      ['Failed', fmtNumber(s.failed)],
      ['Rejected', fmtNumber(s.rejected)],
      ['Cancelled', s.cancelled ? fmtNumber(s.cancelled) : null],
      ['Faults', fmtNumber(s.faults)],
      ['Messages to / from Earth', `${m.links.down.items.length} / ${m.links.up.items.length}`],
      ['Dust storms', `${m.weather.storms.items.length} (${fmtDuration(m.weather.storms.total(m.t0, m.tEnd), sol)})`],
      ['Link down', `${fmtDuration(m.comms.down.total(m.t0, m.tEnd), sol)}`],
    ])}`;
    const ls = m.loadStats;
    if (ls) {
      html += `<h3>Log</h3>${dl([
        ['File', ls.name || '–'],
        ['Size', `${(ls.bytes / 1e6).toFixed(2)} MB${ls.gzip ? ' (gzip)' : ''}`],
        ['Records', fmtNumber(ls.records)],
        ['Loaded in', `${(ls.ms / 1000).toFixed(2)} s`],
      ])}`;
    }
    if (i.metrics) html += `<h3>Run metrics (from run_end)</h3>${renderMetrics(i.metrics, sol)}`;
    else html += '<p class="note">This log has no run metrics (they arrive with protocol 0.2).</p>';
    if (m.warnings.length) html += `<h3>Notes on this log</h3><ul class="warnings">${m.warnings.map((w) => `<li>${esc(w)}</li>`).join('')}</ul>`;
    setHtml($('run-summary'), html);
  }
}

function renderMetrics(metrics, solLength) {
  const rows = [];
  const walk = (obj, prefix) => {
    for (const [k, v] of Object.entries(obj)) {
      if (v && typeof v === 'object' && !Array.isArray(v)) {
        rows.push(`<dt class="grp" style="grid-column:1/-1;color:var(--faint);margin-top:3px">${esc(humanize(k))}</dt>`);
        walk(v, `${prefix}${k}.`);
      } else {
        rows.push(`<dt>${esc(humanize(k))}</dt><dd>${esc(formatMetric(k, v, solLength))}</dd>`);
      }
    }
  };
  walk(metrics, '');
  return `<dl>${rows.join('')}</dl>`;
}
