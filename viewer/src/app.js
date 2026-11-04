// The replay viewer page: loading, URL parameters, the render loop, and the
// wiring between the map, the timeline, the panels and the controls.

import { parseLog, openUrl, openFile, LogError } from './parse.js';
import { ModelBuilder } from './model.js';
import { TerrainLayer } from './render/terrain.js';
import { MapView } from './render/map.js';
import { Timeline } from './render/timeline.js';
import { PowerChart } from './render/chart.js';
import { Panels } from './ui/panels.js';
import { solClock, fmtNumber, fmtDuration, fmtPct, fmtMass, describeCommand, humanize, parseTimeParam } from './format.js';
import { statusColor } from './render/palette.js';

window.__viewerBooted = true;

const $ = (id) => document.getElementById(id);

/** Playback speeds in sim seconds per real second; the last is one sol. */
const SPEEDS = [
  [1, '1 s'],
  [10, '10 s'],
  [60, '1 min'],
  [600, '10 min'],
  [3600, '1 h'],
  [21600, '6 h'],
  [null, '1 sol'],
];

const state = {
  model: null,
  logParam: null,
  t: 0,
  playing: false,
  speed: 600,
  selected: null,
  hover: null,
  dirty: true,
  lastNow: 0,
  lastPanels: 0,
  frame: null,
  map: null,
  terrain: null,
  chart: null,
};

const params = new URLSearchParams(location.search);

// ---------- errors ----------

function showFatal(message) {
  const el = $('fatal');
  el.hidden = false;
  el.textContent = `Viewer error: ${message}`;
}

window.addEventListener('error', (e) => showFatal(e.message || String(e.error)));
window.addEventListener('unhandledrejection', (e) => showFatal(e.reason?.message ?? String(e.reason)));

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function toast(text) {
  const el = $('toast');
  el.textContent = text;
  el.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (el.hidden = true), 2600);
}

// ---------- loading ----------

function showWelcome(show) {
  $('welcome').hidden = !show;
}

function showLoadError(e) {
  showWelcome(true);
  $('load-progress').hidden = true;
  const box = $('load-error');
  box.hidden = false;
  const hint = e instanceof LogError && e.hint ? `<div class="hint">${esc(e.hint)}</div>` : '';
  box.innerHTML = `<b>Could not open the log</b>${esc(e.message)}${hint}`;
  if (!(e instanceof LogError)) console.error(e);
}

async function load(open, { logParam = null, label = '' } = {}) {
  showWelcome(true);
  $('load-error').hidden = true;
  $('load-progress').hidden = false;
  $('load-bar').style.width = '0%';
  $('load-text').textContent = `Opening ${label}…`;
  try {
    const started = performance.now();
    const source = await open();
    const builder = new ModelBuilder();
    const result = await parseLog(source, {
      onRecord: (rec, line) => builder.add(rec, line),
      onProgress: ({ bytes, total, records }) => {
        if (total) $('load-bar').style.width = `${Math.min(100, (100 * bytes) / total).toFixed(1)}%`;
        $('load-text').textContent = `Reading ${label}: ${(bytes / 1e6).toFixed(1)} MB${total ? ` of ${(total / 1e6).toFixed(1)} MB` : ''}, ${fmtNumber(records)} records`;
      },
    });
    $('load-bar').style.width = '100%';
    $('load-text').textContent = `Indexing ${fmtNumber(result.records)} records…`;
    await new Promise((r) => setTimeout(r, 0));
    const model = builder.finish();
    model.loadStats = {
      name: label,
      bytes: result.bytes,
      gzip: result.gzip,
      records: result.records,
      ms: performance.now() - started,
    };
    if (result.badLines) {
      const sample = result.errors
        .slice(0, 3)
        .map((e) => `line ${e.line}: ${e.message}`)
        .join('; ');
      model.warnings.unshift(`${result.badLines} line${result.badLines === 1 ? '' : 's'} could not be read and ${result.badLines === 1 ? 'was' : 'were'} skipped (${sample}).`);
    }
    start(model, logParam);
  } catch (e) {
    showLoadError(e);
  }
}

function loadFile(file) {
  if (!file) return;
  load(() => Promise.resolve(openFile(file)), { label: file.name });
}

// ---------- start a replay ----------

function start(model, logParam) {
  state.model = model;
  state.logParam = logParam;
  state.selected = null;
  state.hover = null;
  showWelcome(false);
  $('copy-link').disabled = false;

  state.terrain = new TerrainLayer(model.site);
  state.map = new MapView($('map'), model, state.terrain);
  state.chart = new PowerChart($('power-chart'), model);
  timeline.setModel(model);
  panels.setModel(model);
  buildSpeeds(model);
  renderRunInfo(model);

  const tParam = parseTimeParam(params.get('t'), model.info.solLength);
  state.t = tParam != null && logParam != null ? clampT(tParam) : model.t0;
  const robotParam = logParam != null ? params.get('robot') : null;
  if (robotParam && model.robotIndex.has(robotParam)) state.selected = model.robotIndex.get(robotParam);
  const speedParam = Number(params.get('speed'));
  if (logParam != null && speedParam > 0) setSpeed(speedParam);
  const tab = logParam != null ? params.get('tab') : null;
  selectTab(tab && ['events', 'fleet', 'run'].includes(tab) ? tab : state.selected != null ? 'fleet' : 'events');

  if (model.warnings.length) {
    const banner = $('warn-banner');
    banner.hidden = false;
    banner.textContent = `${model.warnings[0]}${model.warnings.length > 1 ? ` (+${model.warnings.length - 1} more in the Run tab)` : ''}  ✕`;
    banner.onclick = () => (banner.hidden = true);
  } else {
    $('warn-banner').hidden = true;
  }

  resize();
  setPlaying(logParam != null && params.get('play') === '1');
  render(true);
}

function clampT(t) {
  const m = state.model;
  return Math.min(m.tEnd, Math.max(m.t0, t));
}

function renderRunInfo(m) {
  const i = m.info;
  const end = i.endReason;
  const cls = end === 'goals_complete' ? 'end-ok' : end === 'time_limit' ? 'end-warn' : 'end-err';
  const parts = [
    `<b>${esc(i.scenarioId ?? 'scenario')}</b>`,
    `seed <b>${esc(i.seed ?? '?')}</b>`,
    `<b>${esc(i.coordinator ? `${i.coordinator.name} ${i.coordinator.version}` : 'unknown coordinator')}</b>`,
    end ? `<span class="${cls}">${esc(humanize(end))}</span> at sol ${fmtNumber((m.tEnd - m.t0) / i.solLength, 2)}` : '<span class="end-err">no run_end</span>',
    `protocol ${esc(i.protocol)}`,
  ];
  $('runinfo').innerHTML = parts.join('<span class="sep">·</span>');
  document.title = `${i.scenarioId ?? 'Replay'} seed ${i.seed ?? '?'} · ArcadiaBench replay`;
}

// ---------- playback ----------

function buildSpeeds(model) {
  const box = $('speeds');
  box.innerHTML = '';
  const options = SPEEDS.map(([v, label]) => [v ?? model.info.solLength, label]);
  for (const [v, label] of options) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'btn';
    b.textContent = label;
    b.title = `${label} of mission time per second`;
    b.dataset.speed = String(v);
    b.addEventListener('click', () => setSpeed(v));
    box.appendChild(b);
  }
  // Default: the whole run plays in about four minutes, rounded to a preset.
  const want = Math.max(1, model.duration / 240);
  let best = options[0][0];
  for (const [v] of options) if (Math.abs(Math.log(v / want)) < Math.abs(Math.log(best / want))) best = v;
  setSpeed(best);
}

function setSpeed(v) {
  state.speed = v;
  for (const b of $('speeds').children) b.classList.toggle('active', Number(b.dataset.speed) === v);
}

function stepSpeed(dir) {
  const values = [...$('speeds').children].map((b) => Number(b.dataset.speed));
  let i = values.findIndex((v) => v >= state.speed);
  if (i < 0) i = values.length - 1;
  setSpeed(values[Math.max(0, Math.min(values.length - 1, i + dir))]);
}

function setPlaying(on) {
  const m = state.model;
  if (!m) return;
  if (on && state.t >= m.tEnd) state.t = m.t0;
  state.playing = on;
  const b = $('play');
  b.textContent = on ? '⏸' : '▶';
  b.setAttribute('aria-label', on ? 'Pause' : 'Play');
  state.lastNow = performance.now();
  if (!on) scheduleUrlUpdate();
  state.dirty = true;
}

function seek(t) {
  if (!state.model) return;
  state.t = clampT(t);
  state.dirty = true;
  scheduleUrlUpdate();
}

function scheduleUrlUpdate() {
  clearTimeout(scheduleUrlUpdate.timer);
  scheduleUrlUpdate.timer = setTimeout(() => {
    if (state.logParam == null || state.playing) return;
    history.replaceState(null, '', shareUrl());
  }, 500);
}

function shareUrl() {
  const url = new URL(location.href);
  url.search = '';
  if (state.logParam != null) url.searchParams.set('log', state.logParam);
  url.searchParams.set('t', String(Math.round(state.t * 10) / 10));
  if (state.selected != null) url.searchParams.set('robot', state.model.robots[state.selected].id);
  return url.toString();
}

// ---------- rendering ----------

function resize() {
  if (!state.model) return;
  const legend = $('legend');
  state.map.insets = { left: legend.offsetWidth ? legend.offsetWidth + 14 : 0, right: 44 };
  state.map.resize();
  // When the map cannot keep clear of the legend, fold the legend away (once).
  if (state.map.insetIgnored && !state.legendFolded) {
    state.legendFolded = true;
    legend.classList.add('collapsed');
    state.map.insets = { left: legend.offsetWidth + 14, right: 44 };
    state.map.fit();
  }
  timeline.resize();
  state.chart.resize();
  state.dirty = true;
}

function computeFrame(t) {
  const m = state.model;
  const robots = m.robots.map((_, i) => m.robotAt(i, t));
  const frame = {
    t,
    robots,
    clock: solClock(t, m.info.solLength),
    weather: m.weatherAt(t),
    comms: m.commsAt(t),
    padCounts: m.padAt(t),
    relays: m.activeRelays(t),
    selected: state.selected,
    hover: state.hover,
    selectedPath: null,
  };
  if (state.selected != null) {
    const r = robots[state.selected];
    if (r?.command) frame.selectedPath = m.robots[state.selected].track.between(t, r.command.end);
  }
  return frame;
}

function padSurfaceCounts(t) {
  const m = state.model;
  const counts = new Array(8).fill(0);
  if (!m.pad) return counts;
  for (const idx of m.pad.progress.cells) counts[m.site.cellAt(idx, t).surf]++;
  return counts;
}

function render(forcePanels = false) {
  const m = state.model;
  if (!m) return;
  const t = state.t;
  const f = computeFrame(t);
  state.frame = f;
  state.terrain.update(t);
  state.map.draw(f);
  timeline.draw(t);
  state.chart.draw(t);

  $('clock').textContent = f.clock.text;
  timeline.canvas.setAttribute('aria-valuetext', f.clock.text);
  $('clock-sub').textContent = `t = ${fmtNumber(Math.round(t - m.t0))} s · ${fmtNumber((t - m.t0) / m.info.solLength, 2)} sols`;
  $('sun').className = `sun${f.clock.daylight ? '' : ' night'}`;

  const banner = $('link-banner');
  if (f.comms.link === 'down') {
    banner.hidden = false;
    const back = f.comms.restore ?? (f.comms.until <= m.tEnd ? f.comms.until : null);
    banner.innerHTML = `EARTH LINK DOWN<small>${esc(humanize(f.comms.cause ?? 'unknown cause'))}${back != null ? ` · back in ${esc(fmtDuration(back - t, m.info.solLength))}` : ''}</small>`;
  } else {
    banner.hidden = true;
  }
  const storm = $('storm-banner');
  if (f.weather.storm) {
    storm.hidden = false;
    const pw = m.powerAt(t);
    const solar = pw && Number(m.info.power?.solar_peak_w) > 0 ? ` · solar ${fmtPct(pw.solar / m.info.power.solar_peak_w)} of peak` : '';
    storm.innerHTML = `DUST STORM<small>optical depth ${esc(fmtNumber(f.weather.tau ?? f.weather.storm.peakTau, 2))}${solar}</small>`;
  } else {
    storm.hidden = true;
  }

  const now = performance.now();
  if (forcePanels || !state.playing || now - state.lastPanels > 120) {
    state.lastPanels = now;
    f.padSurfaces = padSurfaceCounts(t);
    panels.update(t, f);
  }
  if (state.hoverPos) updateTooltip();
}

function loop(now) {
  requestAnimationFrame(loop);
  const m = state.model;
  if (!m) return;
  if (state.playing) {
    const dt = Math.min(0.25, Math.max(0, (now - state.lastNow) / 1000));
    state.t = Math.min(m.tEnd, state.t + dt * state.speed);
    if (state.t >= m.tEnd) setPlaying(false);
    state.dirty = true;
  }
  state.lastNow = now;
  if (state.dirty) {
    state.dirty = false;
    render();
  }
}

// ---------- map interaction ----------

function updateTooltip() {
  const tip = $('tooltip');
  const f = state.frame;
  if (state.hover == null || !f || !state.hoverPos) {
    tip.hidden = true;
    return;
  }
  const r = f.robots[state.hover];
  if (!r) {
    tip.hidden = true;
    return;
  }
  const cmd = r.command ? `<br><span class="muted">${esc(r.command.id)}</span> ${esc(describeCommand(r.command.cmd))}` : '';
  const faults = r.faults.length ? `<br><span style="color:#f5b942">${r.faults.length} fault${r.faults.length > 1 ? 's' : ''}: ${esc(r.faults.map((x) => `${humanize(x.component)} ${x.severity}`).join(', '))}</span>` : '';
  tip.innerHTML = `<b style="color:${statusColor(r.status)}">${esc(r.id)}</b> <span class="muted">${esc(r.spec.model ?? '')}</span> · ${esc(humanize(r.status))}<br>battery ${r.batteryFrac != null ? fmtPct(r.batteryFrac) : '–'}${r.spec.haulKg ? ` · payload ${fmtMass(r.payload)}` : ''}${r.inCoverage ? '' : ' · <span style="color:#ff9aa8">out of coverage</span>'}${cmd}${faults}`;
  tip.hidden = false;
  const pane = $('map-pane').getBoundingClientRect();
  const [x, y] = state.hoverPos;
  const w = tip.offsetWidth;
  tip.style.left = `${Math.min(pane.width - w - 8, x + 14)}px`;
  tip.style.top = `${Math.max(8, y - tip.offsetHeight - 10)}px`;
}

function wireMap() {
  const canvas = $('map');
  let drag = null;
  canvas.addEventListener('wheel', (e) => {
    if (!state.map) return;
    e.preventDefault();
    state.map.zoomAt(e.offsetX, e.offsetY, Math.exp(-e.deltaY * 0.0015));
    state.dirty = true;
  }, { passive: false });
  canvas.addEventListener('pointerdown', (e) => {
    if (!state.map) return;
    drag = { x: e.offsetX, y: e.offsetY, moved: false };
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener('pointermove', (e) => {
    if (!state.map) return;
    if (drag) {
      const dx = e.offsetX - drag.x;
      const dy = e.offsetY - drag.y;
      if (drag.moved || Math.hypot(dx, dy) > 3) {
        drag.moved = true;
        canvas.classList.add('dragging');
        state.map.panBy(dx, dy);
        drag.x = e.offsetX;
        drag.y = e.offsetY;
        state.dirty = true;
      }
      return;
    }
    const hit = state.map.hit(e.offsetX, e.offsetY);
    state.hoverPos = [e.offsetX, e.offsetY];
    if (hit !== state.hover) {
      state.hover = hit;
      canvas.classList.toggle('hovering', hit != null);
      state.dirty = true;
    }
    updateTooltip();
  });
  canvas.addEventListener('pointerup', (e) => {
    if (!state.map) return;
    canvas.releasePointerCapture(e.pointerId);
    canvas.classList.remove('dragging');
    if (drag && !drag.moved) select(state.map.hit(e.offsetX, e.offsetY), true);
    drag = null;
  });
  canvas.addEventListener('pointerleave', () => {
    state.hover = null;
    state.hoverPos = null;
    $('tooltip').hidden = true;
    state.dirty = true;
  });
  canvas.addEventListener('dblclick', () => {
    state.map?.fit();
    state.dirty = true;
  });
  $('zoom-in').addEventListener('click', () => zoomButton(1.4));
  $('zoom-out').addEventListener('click', () => zoomButton(1 / 1.4));
  $('zoom-fit').addEventListener('click', () => {
    state.map?.fit();
    state.dirty = true;
  });
}

function zoomButton(factor) {
  if (!state.map) return;
  state.map.zoomAt(state.map.w / 2, state.map.h / 2, factor);
  state.dirty = true;
}

function select(index, fromMap) {
  state.selected = index;
  if (index != null && fromMap) selectTab('fleet');
  state.dirty = true;
  render(true);
  scheduleUrlUpdate();
}

// ---------- tabs, keys, files ----------

function selectTab(name) {
  for (const b of document.querySelectorAll('.tab')) b.setAttribute('aria-selected', String(b.dataset.tab === name));
  for (const body of document.querySelectorAll('.tab-body')) body.hidden = body.dataset.body !== name;
}

function wireControls() {
  for (const b of document.querySelectorAll('.tab')) b.addEventListener('click', () => selectTab(b.dataset.tab));
  $('play').addEventListener('click', () => setPlaying(!state.playing));
  $('to-start').addEventListener('click', () => seek(state.model?.t0 ?? 0));
  $('to-end').addEventListener('click', () => seek(state.model?.tEnd ?? 0));
  $('step-back').addEventListener('click', () => state.model && seek(state.model.prevDecision(state.t)));
  $('step-fwd').addEventListener('click', () => state.model && seek(state.model.nextDecision(state.t)));
  $('open-btn').addEventListener('click', () => $('file-input').click());
  $('welcome-open').addEventListener('click', () => $('file-input').click());
  $('file-input').addEventListener('change', (e) => {
    loadFile(e.target.files?.[0]);
    e.target.value = '';
  });
  $('copy-link').addEventListener('click', async () => {
    if (!state.model) return;
    const url = shareUrl();
    try {
      await navigator.clipboard.writeText(url);
      toast(state.logParam == null ? 'Copied. Local files cannot be linked, so the link carries only the time.' : 'Link to this moment copied.');
    } catch {
      window.prompt('Copy this link:', url);
    }
  });
  $('help-btn').addEventListener('click', () => ($('help').hidden = false));
  $('help-close').addEventListener('click', () => ($('help').hidden = true));
  $('help').addEventListener('click', (e) => {
    if (e.target === $('help')) $('help').hidden = true;
  });

  window.addEventListener('keydown', (e) => {
    if (e.target.closest?.('input, textarea, select')) return;
    // A focused button already acts on Space and Enter.
    if ((e.key === ' ' || e.key === 'Enter') && e.target.closest?.('button')) return;
    const m = state.model;
    if (e.key === 'Escape') {
      if (!$('help').hidden) $('help').hidden = true;
      else if (m) select(null, false);
      return;
    }
    if (e.key === '?') {
      $('help').hidden = !$('help').hidden;
      return;
    }
    if (!m) return;
    const span = m.duration / 100;
    switch (e.key) {
      case ' ':
        e.preventDefault();
        setPlaying(!state.playing);
        break;
      case 'ArrowLeft':
        e.preventDefault();
        seek(e.shiftKey ? state.t - span : m.prevDecision(state.t));
        break;
      case 'ArrowRight':
        e.preventDefault();
        seek(e.shiftKey ? state.t + span : m.nextDecision(state.t));
        break;
      case 'Home':
        seek(m.t0);
        break;
      case 'End':
        seek(m.tEnd);
        break;
      case '[':
        stepSpeed(-1);
        break;
      case ']':
        stepSpeed(1);
        break;
      case 'f':
      case 'F':
        state.map.fit();
        state.dirty = true;
        break;
      default:
        break;
    }
  });

  // Drag and drop anywhere on the page.
  let depth = 0;
  const hasFiles = (e) => [...(e.dataTransfer?.types ?? [])].includes('Files');
  window.addEventListener('dragenter', (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    depth++;
    $('drag-veil').hidden = false;
    $('drop-zone').classList.add('hot');
  });
  window.addEventListener('dragover', (e) => {
    if (hasFiles(e)) e.preventDefault();
  });
  window.addEventListener('dragleave', () => {
    depth = Math.max(0, depth - 1);
    if (!depth) {
      $('drag-veil').hidden = true;
      $('drop-zone').classList.remove('hot');
    }
  });
  window.addEventListener('drop', (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    depth = 0;
    $('drag-veil').hidden = true;
    $('drop-zone').classList.remove('hot');
    loadFile(e.dataTransfer.files[0]);
  });

  window.addEventListener('resize', () => resize());
}

// ---------- boot ----------

const timeline = new Timeline($('timeline'), {
  onSeek: (t) => seek(t),
  onHover: (info, x) => {
    const tip = $('timeline-tip');
    if (!info) {
      tip.hidden = true;
      return;
    }
    tip.hidden = false;
    tip.innerHTML = `<b>${esc(info.clock)}</b>${info.lines.map((l) => `<br>${esc(l)}`).join('')}`;
    const wrap = $('timeline').getBoundingClientRect();
    tip.style.left = `${Math.max(0, Math.min(wrap.width - tip.offsetWidth, x - tip.offsetWidth / 2))}px`;
    state.dirty = true;
  },
});
const panels = new Panels({ onSeek: (t) => seek(t), onSelect: (i, fromMap) => select(i, fromMap) });

wireMap();
wireControls();
requestAnimationFrame(loop);

const logParam = params.get('log');
if (logParam) {
  load(() => openUrl(logParam, { base: location.href }), { logParam, label: logParam.split('/').pop() });
} else {
  showWelcome(true);
}
