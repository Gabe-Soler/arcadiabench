// The replay model: everything the viewer shows, indexed by time.
//
// Feed log records to a ModelBuilder in file order, then call finish(). Every
// per-frame query is a binary search, so scrubbing stays cheap however long
// the run. No DOM dependencies.
//
// Version handling: protocol 0.1 logs lack the 0.2 additions (truth.site,
// run_end.metrics, robot faults and telemetry_t, scenario power and base
// station); everything that depends on them is optional. Protocol 1.0 has
// 0.2's messages unchanged. Unknown record kinds, event types and fields are
// ignored.

import { Series, Intervals, lastAtOrBefore, firstAtOrAfter } from './series.js';
import { buildTrack, Track } from './motion.js';
import { SiteIndex, padProgress } from './site.js';
import { itemFromEvent, itemFromCommand, itemFromLink, itemFromRunEnd } from './feed.js';
import { DEFAULT_SOL_S, fmtNumber } from './format.js';
import { LogError } from './parse.js';

/** Protocol MAJOR.MINOR versions this viewer was written against. */
export const KNOWN_PROTOCOLS = ['0.1', '0.2', '1.0', '1.1'];

/** Statuses in which a robot cannot act. */
export const DISABLED_STATUSES = new Set(['stuck', 'failed', 'offline', 'being_towed', 'lost']);
/** Disabled statuses that hold until the next truth record says otherwise. */
const HELD_STATUSES = new Set(['stuck', 'failed', 'offline', 'lost']);

const isPoint = (p) => p != null && Number.isFinite(p.x) && Number.isFinite(p.y);
const num = (v) => (Number.isFinite(v) ? v : null);

/** Collects records in file order; `finish()` builds the indexed model. */
export class ModelBuilder {
  constructor() {
    this.runStart = null;
    this.runEnd = null;
    this.truths = [];
    this.stream = [];
    this.uplinks = [];
    this.downlinks = [];
    this.unknownKinds = new Map();
    this.warnings = [];
    this.records = 0;
    this.lastT = -Infinity;
    this.outOfOrder = 0;
    this.seq = 0;
  }

  /** Add one parsed record. `line` is used in warnings. */
  add(rec, line = 0) {
    this.records++;
    const t = Number(rec.t);
    if (!Number.isFinite(t)) {
      this.warn(`Line ${line}: "${rec.kind}" record without a numeric t was skipped.`);
      return;
    }
    if (t < this.lastT - 1e-6) this.outOfOrder++;
    else this.lastT = Math.max(this.lastT, t);
    const seq = this.seq++;
    switch (rec.kind) {
      case 'run_start':
        if (this.runStart) this.warn(`Line ${line}: a second run_start was ignored.`);
        else this.runStart = rec;
        break;
      case 'truth':
        this.truths.push({
          t,
          robots: Array.isArray(rec.robots) ? rec.robots : [],
          changed: Array.isArray(rec.changed_cells) ? rec.changed_cells : [],
          site: rec.site && typeof rec.site === 'object' ? rec.site : null,
        });
        break;
      case 'observed':
        if (rec.event && typeof rec.event === 'object') this.stream.push({ seq, t, kind: 'observed', e: rec.event });
        break;
      case 'commanded':
        if (rec.command && typeof rec.command === 'object') this.stream.push({ seq, t, kind: 'commanded', c: rec.command });
        break;
      case 'earth_uplink':
        this.uplinks.push({ seq, t, deliver: num(rec.deliver_t), message: rec.message ?? {} });
        break;
      case 'earth_downlink':
        this.downlinks.push({ seq, t, deliver: num(rec.deliver_t), message: rec.message ?? {} });
        break;
      case 'run_end':
        this.runEnd = rec;
        break;
      default:
        this.unknownKinds.set(rec.kind, (this.unknownKinds.get(rec.kind) ?? 0) + 1);
    }
  }

  warn(message) {
    if (this.warnings.length < 50) this.warnings.push(message);
  }

  finish() {
    return new ReplayModel(this);
  }
}

/** Build a model from an array of records (tests, small inputs). */
export function modelFromRecords(records) {
  const b = new ModelBuilder();
  records.forEach((r, i) => b.add(r, i + 1));
  return b.finish();
}

function robotSpec(spec, cellSize) {
  const caps = Array.isArray(spec?.capabilities) ? spec.capabilities.filter((c) => c && typeof c.name === 'string') : [];
  const cap = (name) => caps.find((c) => c.name === name);
  const speed = Number(spec?.max_speed_m_s) > 0 ? Number(spec.max_speed_m_s) : 0.2;
  return {
    id: spec.robot_id,
    model: spec.model ?? null,
    massKg: num(spec.mass_kg),
    batteryWh: Number(spec.battery_capacity_wh) > 0 ? Number(spec.battery_capacity_wh) : null,
    speed,
    cellsPerSecond: speed / cellSize,
    idleW: num(spec.idle_power_w),
    caps: caps.map((c) => c.name),
    capSpecs: caps,
    main: caps[0]?.name ?? null,
    haulKg: num(cap('haul')?.capacity) ?? 0,
    tankWh: num(cap('charge')?.capacity),
    relayRange: num(cap('relay')?.range_m),
    known: true,
  };
}

function newSnaps() {
  return { t: [], x: [], y: [], status: [], battery: [], payload: [], cmd: [], faults: [], cov: [], telemetry: [], health: [], tank: [] };
}

function pushSnap(s, t, r) {
  const n = s.t.length;
  const row = [
    t,
    r.position.x,
    r.position.y,
    typeof r.status === 'string' ? r.status : 'idle',
    num(r.battery_wh),
    num(r.payload_kg) ?? 0,
    typeof r.current_command === 'string' ? r.current_command : null,
    Array.isArray(r.faults) && r.faults.length ? r.faults : null,
    r.in_relay_coverage !== false,
    num(r.telemetry_t),
    r.health && typeof r.health === 'object' ? r.health : null,
    // A mobile charger's tank, from protocol 1.1.
    num(r.tank_wh),
  ];
  const keys = ['t', 'x', 'y', 'status', 'battery', 'payload', 'cmd', 'faults', 'cov', 'telemetry', 'health', 'tank'];
  if (n > 0 && s.t[n - 1] === t) {
    keys.forEach((k, i) => (s[k][n - 1] = row[i]));
  } else if (n === 0 || t > s.t[n - 1]) {
    keys.forEach((k, i) => s[k].push(row[i]));
  }
}

function snapPosition(s, t) {
  const n = s.t.length;
  if (n === 0) return null;
  const i = lastAtOrBefore(s.t, t);
  if (i < 0) return { x: s.x[0], y: s.y[0] };
  if (i >= n - 1) return { x: s.x[n - 1], y: s.y[n - 1] };
  const f = s.t[i + 1] > s.t[i] ? (t - s.t[i]) / (s.t[i + 1] - s.t[i]) : 0;
  return { x: s.x[i] + (s.x[i + 1] - s.x[i]) * f, y: s.y[i] + (s.y[i + 1] - s.y[i]) * f };
}

export class ReplayModel {
  constructor(b) {
    if (!b.runStart) throw new LogError('The log has no run_start record.');
    const rs = b.runStart;
    const sc = rs.scenario && typeof rs.scenario === 'object' ? rs.scenario : {};
    this.warnings = [...b.warnings];
    this.info = {
      protocol: typeof rs.protocol_version === 'string' ? rs.protocol_version : '?',
      simulator: rs.simulator ?? null,
      coordinator: rs.coordinator ?? null,
      runId: rs.run_id ?? null,
      seed: num(rs.seed),
      scenarioId: sc.scenario_id ?? null,
      startT: num(sc.start_t) ?? num(Number(rs.t)) ?? 0,
      timeLimitT: num(sc.time_limit_t),
      solLength: Number(sc.sol_length_s) > 0 ? Number(sc.sol_length_s) : DEFAULT_SOL_S,
      policy: sc.policy ?? null,
      power: sc.power && typeof sc.power === 'object' ? sc.power : null,
      endReason: b.runEnd?.reason ?? null,
      metrics: b.runEnd?.metrics && typeof b.runEnd.metrics === 'object' ? b.runEnd.metrics : null,
    };
    this.checkVersion();
    if (!b.runEnd) this.warnings.push('The log has no run_end record: the run may have been cut short. Showing what is there.');
    if (b.outOfOrder) this.warnings.push(`${b.outOfOrder} records were out of time order; the replay may be inaccurate around them.`);
    for (const [kind, count] of b.unknownKinds) {
      this.warnings.push(`Ignored ${count} record${count === 1 ? '' : 's'} of unknown kind "${kind}" (written by a newer simulator?).`);
    }

    if (!rs.true_site || typeof rs.true_site !== 'object') throw new LogError('run_start has no true_site map, so there is no site to draw.');
    this.site = new SiteIndex(rs.true_site);
    const cellSize = this.site.cellSize;

    this.t0 = this.info.startT;
    const lastTruth = b.truths.length ? b.truths[b.truths.length - 1].t : this.t0;
    this.lastTruthT = b.truths.length ? lastTruth : null;
    // Since protocol 0.2 the simulator also writes the run's final instant: the
    // events still pending and a last truth record at run_end's time, after the
    // last decision. Older logs stop at the last decision, so the end of the
    // run has to be inferred (see buildCommands and buildGoals).
    let lastCommandT = -Infinity;
    for (const it of b.stream) if (it.kind === 'commanded') lastCommandT = Math.max(lastCommandT, it.t);
    this.finalInstant =
      b.runEnd != null && b.truths.length > 0 && lastTruth >= Number(b.runEnd.t) - 1e-6 && lastCommandT < lastTruth - 1e-9;
    /** Decision points: the metrics' count, else truth records less the final instant. */
    this.decisionCount = num(this.info.metrics?.decisions) ?? b.truths.length - (this.finalInstant ? 1 : 0);
    this.tEnd = Math.max(num(Number(b.runEnd?.t)) ?? -Infinity, Number.isFinite(b.lastT) ? b.lastT : this.t0, lastTruth, this.t0);
    this.info.endT = b.runEnd ? Number(b.runEnd.t) : null;
    this.duration = Math.max(0, this.tEnd - this.t0);

    this.buildFixtures(sc);
    this.buildRobots(sc, b.truths, cellSize);
    this.buildCommands(b.stream);
    this.buildTracks();
    this.buildCells(b.truths);
    this.buildGoals(b.stream, b.uplinks);
    this.buildPower(b.truths, b.stream);
    this.buildWeather(b.truths, b.stream);
    this.buildComms(b.truths, b.stream);
    this.buildLinks(b.uplinks, b.downlinks);
    this.buildFeed(b);
    this.buildMarkers(b.stream);
    this.decisionTimes = [...new Set(b.truths.map((tr) => tr.t))].sort((x, y) => x - y);
    this.hasV2 = {
      siteTruth: b.truths.some((tr) => tr.site),
      faults: b.truths.some((tr) => tr.robots.some((r) => Array.isArray(r?.faults))),
      metrics: this.info.metrics != null,
      power: this.info.power != null,
      baseStation: this.baseStation != null,
    };
  }

  checkVersion() {
    const m = /^(\d+)\.(\d+)\./.exec(this.info.protocol);
    const mm = m ? `${m[1]}.${m[2]}` : null;
    if (!mm) {
      this.warnings.push(`The log does not state a valid protocol version ("${this.info.protocol}"). Showing what this viewer understands.`);
    } else if (!KNOWN_PROTOCOLS.includes(mm)) {
      const known = `${KNOWN_PROTOCOLS.slice(0, -1).join(', ')} and ${KNOWN_PROTOCOLS.at(-1)}`;
      this.warnings.push(
        `This log uses protocol ${this.info.protocol}; the viewer was written for ${known}. Unknown records and fields are ignored.`,
      );
    }
  }

  buildFixtures(sc) {
    const pos = (p) => (isPoint(p) ? { x: p.x, y: p.y } : null);
    this.stations = (Array.isArray(sc.stations) ? sc.stations : [])
      .filter((s) => s && isPoint(s.position))
      .map((s) => ({ id: s.station_id, ...pos(s.position), slots: num(s.slots), maxPowerW: num(s.max_power_w) }));
    this.depots = (Array.isArray(sc.depots) ? sc.depots : [])
      .filter((d) => d && isPoint(d.position))
      .map((d) => ({ id: d.depot_id, ...pos(d.position) }));
    const bs = sc.base_station;
    this.baseStation = bs && isPoint(bs.position) ? { ...pos(bs.position), rangeM: num(bs.range_m) } : null;
    this.stationPos = new Map(this.stations.map((s) => [s.id, { x: s.x, y: s.y }]));
    this.depotPos = new Map(this.depots.map((d) => [d.id, { x: d.x, y: d.y }]));
  }

  buildRobots(sc, truths, cellSize) {
    this.robots = [];
    this.robotIndex = new Map();
    const addRobot = (spec) => {
      const r = { id: spec.id, spec, snaps: newSnaps(), commands: [], starts: [], track: new Track(), lostAt: null };
      this.robotIndex.set(spec.id, this.robots.length);
      this.robots.push(r);
      return r;
    };
    for (const spec of Array.isArray(sc.robots) ? sc.robots : []) {
      if (spec && typeof spec.robot_id === 'string' && !this.robotIndex.has(spec.robot_id)) addRobot(robotSpec(spec, cellSize));
    }
    for (const tr of truths) {
      for (const r of tr.robots) {
        if (!r || typeof r.robot_id !== 'string' || !isPoint(r.position)) continue;
        let rb = this.robots[this.robotIndex.get(r.robot_id)];
        if (!rb) {
          rb = addRobot({ ...robotSpec({ robot_id: r.robot_id }, cellSize), known: false });
          this.warnings.push(`Robot ${r.robot_id} appears in truth records but not in the scenario.`);
        }
        pushSnap(rb.snaps, tr.t, r);
        if (rb.lostAt == null && r.status === 'lost') rb.lostAt = tr.t;
      }
    }
  }

  buildCommands(stream) {
    this.commandEntries = [];
    const byId = new Map();
    const latest = (id, pred) => {
      const list = byId.get(id);
      if (!list) return null;
      for (let i = list.length - 1; i >= 0; i--) if (pred(list[i])) return list[i];
      return null;
    };
    const create = (id, robot, cmd, t) => {
      const entry = { id, robot, cmd, issued: t, start: null, est: null, end: null, outcome: null, reason: null, detail: null, energy: null };
      if (!byId.has(id)) byId.set(id, []);
      byId.get(id).push(entry);
      this.commandEntries.push(entry);
      return entry;
    };
    const stats = { commands: 0, completed: 0, failed: 0, rejected: 0, faults: 0, cancelled: 0 };
    for (const it of stream) {
      if (it.kind === 'commanded') {
        stats.commands++;
        create(it.c.command_id, it.c.robot_id, it.c, it.t);
        continue;
      }
      const e = it.e;
      const id = e.command_id;
      switch (e.type) {
        case 'command_accepted': {
          const entry = latest(id, (x) => x.start == null && x.outcome == null) ?? create(id, e.robot_id, { type: '?' }, e.t);
          entry.start = num(e.t) ?? it.t;
          entry.est = num(e.estimated_duration_s);
          break;
        }
        case 'command_rejected': {
          const entry = latest(id, (x) => x.start == null && x.outcome == null) ?? create(id, e.robot_id, { type: '?' }, e.t);
          Object.assign(entry, { outcome: 'rejected', end: num(e.t) ?? it.t, reason: e.reason ?? null, detail: e.detail ?? null });
          stats.rejected++;
          break;
        }
        case 'command_completed':
        case 'command_failed': {
          const entry = latest(id, (x) => x.start != null && x.outcome == null);
          if (!entry) break;
          entry.outcome = e.type === 'command_completed' ? 'completed' : 'failed';
          entry.end = num(e.t) ?? it.t;
          entry.reason = e.reason ?? null;
          entry.detail = e.detail ?? null;
          entry.energy = num(e.energy_used_wh);
          if (entry.outcome === 'completed') stats.completed++;
          else if (e.reason === 'cancelled') stats.cancelled++;
          else stats.failed++;
          break;
        }
        case 'robot_fault':
          stats.faults++;
          break;
        default:
          break;
      }
    }
    // Logs without the final instant end before the simulator's last
    // observation, so commands issued at the final decision point never get
    // an acceptance in the log. Treat them as running until the end.
    const lastDecision = this.lastTruthT ?? -Infinity;
    for (const entry of this.commandEntries) {
      if (this.finalInstant) break;
      if (entry.start == null && entry.outcome == null && entry.issued >= lastDecision) {
        entry.start = entry.issued;
        entry.assumed = true;
      }
    }
    for (const entry of this.commandEntries) {
      if (entry.start == null) continue;
      const r = this.robots[this.robotIndex.get(entry.robot)];
      if (!r) continue;
      if (entry.end == null) entry.end = this.tEnd;
      r.commands.push(entry);
    }
    for (const r of this.robots) {
      r.commands.sort((a, b) => a.start - b.start);
      r.starts = r.commands.map((c) => c.start);
    }
    this.stats = stats;
  }

  buildTracks() {
    const bounds = { width: this.site.width, height: this.site.height };
    this.scanWindows = new Map();
    const work = [];
    const pickups = [];
    for (const r of this.robots) {
      const result = buildTrack(
        r.snaps,
        r.commands.map((c) => ({ id: c.id, cmd: c.cmd, start: c.start, end: c.end, completed: c.outcome === 'completed', est: c.est })),
        {
          speed: r.spec.cellsPerSecond,
          capacity: r.spec.haulKg,
          ctxFor: (c, startAnchor) => ({
            start: startAnchor,
            stations: this.stationPos,
            depots: this.depotPos,
            bounds,
            robotPos: (id) => {
              const other = this.robots[this.robotIndex.get(id)];
              return other ? snapPosition(other.snaps, c.start) : null;
            },
          }),
        },
      );
      r.track = result.track;
      work.push(...result.work);
      for (const s of result.scans) this.scanWindows.set(s.id, s);
      for (const p of result.pickups) pickups.push({ ...p, tower: r });
    }
    this.workHints = work;
    this.applyTows(pickups);
  }

  /** A towed robot rides along with its tower between pickup and drop-off. */
  applyTows(pickups) {
    for (const p of pickups) {
      const target = this.robots[this.robotIndex.get(p.target)];
      const cmd = p.tower.commands.find((c) => c.id === p.id);
      if (!target || !cmd || !(cmd.end > p.t)) continue;
      const s = target.snaps;
      const from = lastAtOrBefore(s.t, p.t) + 1;
      const to = lastAtOrBefore(s.t, cmd.end);
      let towed = false;
      for (let i = Math.max(0, from); i <= to; i++) if (s.status[i] === 'being_towed') towed = true;
      // A short tow may fall between snapshots: then the target simply turns
      // up somewhere else when it ends.
      const pre = Math.max(0, from - 1);
      const post = firstAtOrAfter(s.t, cmd.end);
      const moved = post < s.t.length && Math.hypot(s.x[post] - s.x[pre], s.y[post] - s.y[pre]) > 0.5;
      if (!towed && !moved) continue;
      const old = target.track;
      const tower = p.tower.track;
      // A disabled robot stays put until it is picked up.
      const before = Math.max(0, lastAtOrBefore(s.t, p.t));
      const a0 = { x: s.x[before], y: s.y[before], payload: s.payload[before] };
      const b0 = tower.at(p.t);
      const a1 = old.at(cmd.end);
      const b1 = tower.at(cmd.end);
      if (!a0 || !b0 || !a1 || !b1) continue;
      const d0 = { x: a0.x - b0.x, y: a0.y - b0.y };
      const d1 = { x: a1.x - b1.x, y: a1.y - b1.y };
      const nt = new Track();
      for (let i = 0; i < old.length && old.t[i] < p.t; i++) nt.push(old.t[i], old.x[i], old.y[i], old.s[i], old.p[i]);
      for (const q of tower.between(p.t, cmd.end)) {
        if (q.t >= cmd.end) break;
        const f = (q.t - p.t) / (cmd.end - p.t);
        nt.push(q.t, q.x + d0.x + (d1.x - d0.x) * f, q.y + d0.y + (d1.y - d0.y) * f, 'being_towed', a0.payload);
      }
      for (let i = 0; i < old.length; i++) if (old.t[i] >= cmd.end) nt.push(old.t[i], old.x[i], old.y[i], old.s[i], old.p[i]);
      target.track = nt;
    }
  }

  buildCells(truths) {
    const hints = new Map();
    for (const w of this.workHints) {
      const idx = this.site.index(w);
      if (idx < 0) continue;
      if (!hints.has(idx)) hints.set(idx, []);
      hints.get(idx).push(w.t);
    }
    for (const list of hints.values()) list.sort((x, y) => x - y);
    // A changed cell is reported at the next truth record; when a robot's
    // modelled work on that cell finished inside the interval, use that time
    // so the pad takes shape behind the robot rather than in jumps.
    let prevT = -Infinity;
    for (const tr of truths) {
      for (const obs of tr.changed) {
        const idx = this.site.index(obs?.at);
        if (idx < 0) continue;
        let tc = tr.t;
        const h = hints.get(idx);
        if (h) {
          const j = lastAtOrBefore(h, tr.t);
          if (j >= 0 && h[j] > prevT) tc = h[j];
        }
        this.site.addChange(idx, tc, obs.cell);
      }
      prevT = tr.t;
    }
    this.site.seal();
  }

  buildGoals(stream, uplinks) {
    this.goals = [];
    const seen = new Set();
    const addGoal = (t, m) => {
      if (!m || m.type !== 'goal' || seen.has(m.goal_id)) return;
      seen.add(m.goal_id);
      this.goals.push({ t, goal: m });
    };
    for (const it of stream) if (it.kind === 'observed' && it.e.type === 'earth_message') addGoal(num(it.e.t) ?? it.t, it.e.message);
    for (const u of uplinks) addGoal(u.deliver ?? u.t, u.message);
    const g = this.goals.find((x) => x.goal.objective?.kind === 'landing_pad' && x.goal.objective.pad?.area);
    if (!g) {
      this.pad = null;
      return;
    }
    const spec = g.goal.objective.pad;
    const scans = [];
    for (const it of stream) {
      if (it.kind !== 'observed' || it.e.type !== 'scan_result' || !Array.isArray(it.e.cells)) continue;
      const end = num(it.e.t) ?? it.t;
      const window = this.scanWindows.get(it.e.command_id);
      const entry = this.commandEntries.find((c) => c.id === it.e.command_id && c.start != null);
      const start = window ? window.start : entry ? entry.start : end;
      const cells = [];
      for (const c of it.e.cells) {
        const idx = this.site.index(c?.at);
        if (idx >= 0) cells.push(idx);
      }
      scans.push({ start, end, cells, robot: it.e.robot_id });
    }
    const complete = this.info.endReason === 'goals_complete';
    if (complete && !this.finalInstant) {
      // The scan that completed the goal ends the run, and its result is never
      // logged: count scans still running at the end as finishing then.
      for (const entry of this.commandEntries) {
        if (entry.cmd?.type !== 'scan' || entry.start == null || entry.outcome != null) continue;
        const area = entry.cmd.area;
        if (!area?.min || !area?.max) continue;
        const cells = [];
        for (let y = Math.max(0, area.min.y); y <= Math.min(this.site.height - 1, area.max.y); y++) {
          for (let x = Math.max(0, area.min.x); x <= Math.min(this.site.width - 1, area.max.x); x++) cells.push(y * this.site.width + x);
        }
        const start = this.scanWindows.get(entry.id)?.start ?? entry.start;
        scans.push({ start, end: this.tEnd, cells, robot: entry.robot, synthetic: true });
      }
    }
    this.scans = scans;
    const progress = padProgress(this.site, spec, scans);
    // goals_complete means the simulator judged every pad cell verified; logs
    // without the final instant lack the scan that proved it.
    if (complete && !this.finalInstant && (progress.verified.step(this.tEnd) ?? 0) < progress.total) {
      progress.verified.push(this.tEnd, progress.total);
    }
    this.pad = { goalId: g.goal.goal_id, arrivedT: g.t, spec, progress };
  }

  buildPower(truths, stream) {
    const p = { storage: new Series(), solar: new Series(), fission: new Series(), demand: new Series(), capacity: null, source: null };
    let lastCapacity = null;
    if (truths.some((tr) => tr.site)) {
      p.source = 'truth';
      for (const tr of truths) {
        if (!tr.site) continue;
        p.storage.push(tr.t, tr.site.storage_wh);
        p.solar.push(tr.t, tr.site.solar_w);
        p.fission.push(tr.t, tr.site.fission_w);
        p.demand.push(tr.t, tr.site.demand_w);
      }
    }
    for (const it of stream) {
      if (it.kind !== 'observed' || it.e.type !== 'power') continue;
      if (Number.isFinite(it.e.storage_capacity_wh)) lastCapacity = it.e.storage_capacity_wh;
      if (p.source === 'truth') continue;
      const t = num(it.e.t) ?? it.t;
      p.storage.push(t, it.e.storage_wh);
      p.solar.push(t, it.e.solar_w);
      p.fission.push(t, it.e.fission_w);
      p.demand.push(t, it.e.demand_w);
    }
    if (!p.source && p.storage.length) p.source = 'events';
    p.capacity = num(this.info.power?.storage_capacity_wh) ?? lastCapacity ?? (p.storage.length ? p.storage.max() : null);
    this.power = p;
  }

  buildWeather(truths, stream) {
    const w = { tau: new Series(), temp: new Series(), tempStep: true, storms: null, source: null };
    const events = stream.filter((it) => it.kind === 'observed' && it.e.type === 'weather');
    if (truths.some((tr) => tr.site)) {
      w.source = 'truth';
      w.tempStep = false;
      for (const tr of truths) {
        if (!tr.site) continue;
        w.tau.push(tr.t, tr.site.optical_depth);
        w.temp.push(tr.t, tr.site.temperature_c);
      }
    } else if (events.length) {
      // The simulator ramps optical depth linearly between the reports it
      // sends (storm start, peak, end), so linear interpolation is faithful.
      w.source = 'events';
      for (const it of events) {
        const t = num(it.e.t) ?? it.t;
        w.tau.push(t, it.e.optical_depth);
        w.temp.push(t, it.e.temperature_c);
      }
    }
    if (!w.source) {
      // Protocol 0.1 logs report weather only when it changes; robot health
      // temperature is ambient there, so it gives a far denser curve.
      for (const tr of truths) {
        const temps = tr.robots.map((r) => r?.health?.temperature_c).filter(Number.isFinite);
        if (temps.length) w.temp.push(tr.t, temps.reduce((a, v) => a + v, 0) / temps.length);
      }
      if (w.temp.length) w.tempStep = false;
    }
    if (w.source === 'events') {
      const temp = new Series();
      for (const tr of truths) {
        const temps = tr.robots.map((r) => r?.health?.temperature_c).filter(Number.isFinite);
        if (temps.length) temp.push(tr.t, temps.reduce((a, v) => a + v, 0) / temps.length);
      }
      if (temp.length > w.temp.length) {
        w.temp = temp;
        w.tempStep = false;
      }
    }
    const storms = [];
    let open = null;
    const openEnd = this.tEnd + 1e-3;
    for (const it of events) {
      const t = num(it.e.t) ?? it.t;
      if (it.e.condition === 'dust_storm') {
        if (!open) open = { start: t, end: openEnd, open: true, peakTau: 0 };
        open.peakTau = Math.max(open.peakTau, Number(it.e.optical_depth) || 0);
      } else if (open) {
        open.end = t;
        open.open = false;
        storms.push(open);
        open = null;
      }
    }
    if (open) storms.push(open);
    for (const s of storms) {
      // Peak from the optical depth series when it is denser than the reports.
      for (let i = 0; i < w.tau.length; i++) if (w.tau.t[i] >= s.start && w.tau.t[i] < s.end) s.peakTau = Math.max(s.peakTau, w.tau.v[i]);
    }
    w.storms = new Intervals(storms);
    this.weather = w;
  }

  buildComms(truths, stream) {
    const events = [];
    for (const it of stream) {
      if (it.kind === 'observed' && it.e.type === 'comms') {
        events.push({
          t: num(it.e.t) ?? it.t,
          link: it.e.link === 'down' ? 'down' : 'up',
          cause: it.e.cause ?? null,
          delay: num(it.e.one_way_delay_s),
          restore: num(it.e.expected_restore_t),
          // When the pass window in progress closes, from protocol 1.1.
          windowEnd: num(it.e.window_end_t),
        });
      }
    }
    events.sort((a, b) => a.t - b.t);
    const delay = new Series();
    const siteLink = new Series();
    const hasSite = truths.some((tr) => tr.site);
    if (hasSite) {
      for (const tr of truths) {
        if (!tr.site) continue;
        delay.push(tr.t, tr.site.one_way_delay_s);
        siteLink.push(tr.t, tr.site.link === 'down' ? 1 : 0);
      }
    } else {
      for (const e of events) if (e.delay != null) delay.push(e.t, e.delay);
    }
    // One interval per stretch of time the link is down for one cause. A
    // stretch can change cause while it lasts (an outage that gives way to
    // the blackout, or that ends in a gap between passes): each part keeps
    // its own cause, and all of them know when the whole stretch began
    // (`since`) and when the link came back (`until`).
    const down = [];
    let open = null;
    let stretch = [];
    const openEnd = this.tEnd + 1e-3;
    const close = (t) => {
      open.end = t;
      open.open = false;
      down.push(open);
      stretch.push(open);
      open = null;
    };
    if (events.length) {
      for (const e of events) {
        if (e.link === 'down') {
          if (open && e.cause !== open.cause) close(e.t);
          if (!open) {
            open = { start: e.t, end: openEnd, open: true, cause: e.cause, restore: e.restore, since: stretch.length ? stretch[0].since : e.t, until: openEnd };
          } else if (e.restore != null) open.restore = e.restore;
        } else if (open) {
          close(e.t);
          for (const part of stretch) part.until = e.t;
          stretch = [];
        }
      }
    } else {
      for (let i = 0; i < siteLink.length; i++) {
        const isDown = siteLink.v[i] === 1;
        const t = siteLink.t[i];
        if (isDown && !open) open = { start: t, end: openEnd, open: true, cause: null, restore: null, since: t, until: openEnd };
        else if (!isDown && open) {
          close(t);
          for (const part of stretch) part.until = t;
          stretch = [];
        }
      }
    }
    if (open) down.push(open);
    this.comms = { events, times: events.map((e) => e.t), delay, siteLink, down: new Intervals(down), delayStep: !hasSite };
  }

  buildLinks(uplinks, downlinks) {
    const norm = (list) => {
      const items = list.map((m) => ({ t: m.t, deliver: m.deliver, message: m.message })).sort((a, b) => a.t - b.t);
      let maxTransit = 0;
      for (const m of items) if (m.deliver != null) maxTransit = Math.max(maxTransit, m.deliver - m.t);
      return { items, times: items.map((m) => m.t), maxTransit, undelivered: items.filter((m) => m.deliver == null) };
    };
    this.links = { up: norm(uplinks), down: norm(downlinks) };
  }

  buildFeed(b) {
    const items = [];
    // Policy updates are described by what they change: against the policy
    // Earth last sent for the uplink, and the one in force at the site for
    // its arrival.
    let sitePolicy = this.info.policy;
    for (const it of b.stream) {
      let item;
      if (it.kind === 'commanded') item = itemFromCommand(it.c, it.t);
      else {
        item = itemFromEvent(it.e, it.t, { prevPolicy: sitePolicy });
        if (it.e.type === 'earth_message' && it.e.message?.type === 'policy_update') sitePolicy = it.e.message.policy;
      }
      if (item) items.push({ ...item, seq: it.seq });
    }
    let sentPolicy = this.info.policy;
    for (const u of b.uplinks) {
      items.push({ ...itemFromLink('up', u, { prevPolicy: sentPolicy }), seq: u.seq });
      if (u.message?.type === 'policy_update') sentPolicy = u.message.policy;
    }
    for (const d of b.downlinks) items.push({ ...itemFromLink('down', d), seq: d.seq });
    if (b.runEnd) items.push({ ...itemFromRunEnd({ ...b.runEnd, t: this.tEnd }), seq: b.seq });
    items.sort((x, y) => x.t - y.t || x.seq - y.seq);
    this.feed = { items, times: items.map((i) => i.t) };
  }

  buildMarkers(stream) {
    const markers = [];
    for (const it of stream) {
      if (it.kind !== 'observed') continue;
      const e = it.e;
      const t = num(e.t) ?? it.t;
      if (e.type === 'robot_fault') {
        markers.push({ t, type: 'fault', sev: e.severity === 'disabled' ? 'disabled' : 'degraded', robot: e.robot_id, label: `${e.robot_id}: ${String(e.component ?? '').replaceAll('_', ' ')} ${e.severity}` });
      } else if (e.type === 'command_failed' && e.reason !== 'cancelled') {
        markers.push({ t, type: 'failure', robot: e.robot_id, label: `${e.robot_id}: ${e.command_id} failed (${String(e.reason).replaceAll('_', ' ')})` });
      }
    }
    for (const r of this.robots) if (r.lostAt != null) markers.push({ t: r.lostAt, type: 'lost', robot: r.id, label: `${r.id} lost` });
    if (this.info.endReason) markers.push({ t: this.tEnd, type: 'end', label: `run ended: ${this.info.endReason.replaceAll('_', ' ')}` });
    markers.sort((a, b) => a.t - b.t);
    this.markers = markers;
  }

  // ----- queries -----

  /** Robot i's command running at time t, or null. */
  activeCommand(i, t) {
    const r = this.robots[i];
    const k = lastAtOrBefore(r.starts, t);
    if (k < 0) return null;
    const c = r.commands[k];
    return t < c.end || (t === c.start && c.end === c.start) ? c : null;
  }

  /** Everything about robot i at time t. */
  robotAt(i, t) {
    const r = this.robots[i];
    const s = r.snaps;
    const st = r.track.at(t);
    if (!st) return null;
    const n = s.t.length;
    let j = lastAtOrBefore(s.t, t);
    const jj = Math.max(0, j);
    let battery = s.battery[jj];
    if (j >= 0 && j + 1 < n && s.battery[j] != null && s.battery[j + 1] != null && s.t[j + 1] > s.t[j]) {
      battery = s.battery[j] + ((s.battery[j + 1] - s.battery[j]) * (t - s.t[j])) / (s.t[j + 1] - s.t[j]);
    }
    if (j < 0) j = 0;
    const cap = r.spec.batteryWh;
    // A robot the truth shows as disabled stays so in between, even while a
    // command of its own (such as depot service) runs.
    let status = st.status;
    if (status !== 'being_towed' && HELD_STATUSES.has(s.status[j]) && s.t[j] <= t) status = s.status[j];
    return {
      index: i,
      id: r.id,
      spec: r.spec,
      x: st.x,
      y: st.y,
      heading: status === st.status ? st.heading : null,
      status,
      battery,
      batteryFrac: battery != null && cap ? Math.min(1, Math.max(0, battery / cap)) : null,
      payload: st.payload ?? 0,
      tank: s.tank[j] ?? null,
      faults: s.faults[j] ?? [],
      inCoverage: s.cov[j] !== false,
      telemetryT: s.telemetry[j],
      health: s.health[j],
      sampleT: s.t[j],
      command: this.activeCommand(i, t),
      lost: status === 'lost',
    };
  }

  /** Pad tallies at time t, or null without a landing-pad goal. */
  padAt(t) {
    if (!this.pad) return null;
    const p = this.pad.progress;
    return {
      inSpec: p.inSpec.step(t) ?? 0,
      verified: p.verified.step(t) ?? 0,
      berm: p.berm.step(t) ?? 0,
      total: p.total,
      bermTotal: p.bermTotal,
    };
  }

  powerAt(t) {
    const p = this.power;
    if (!p.storage.length) return null;
    const storage = p.storage.linear(t);
    return {
      storage,
      capacity: p.capacity,
      frac: p.capacity ? Math.min(1, Math.max(0, storage / p.capacity)) : null,
      solar: p.solar.linear(t),
      fission: p.fission.linear(t),
      demand: p.demand.linear(t),
      source: p.source,
    };
  }

  weatherAt(t) {
    const w = this.weather;
    const storm = w.storms.at(t);
    return {
      tau: w.tau.length ? w.tau.linear(t) : null,
      temp: w.temp.length ? (w.tempStep ? w.temp.step(t) ?? w.temp.v[0] : w.temp.linear(t)) : null,
      storm,
      source: w.source,
    };
  }

  commsAt(t) {
    const c = this.comms;
    const k = lastAtOrBefore(c.times, t);
    const ev = k >= 0 ? c.events[k] : null;
    const down = c.down.at(t);
    let link = down ? 'down' : 'up';
    if (!c.events.length && c.siteLink.length) link = c.siteLink.step(t) === 1 ? 'down' : 'up';
    const delay = c.delay.length ? (c.delayStep ? c.delay.step(t) ?? c.delay.v[0] : c.delay.linear(t)) : null;
    return {
      link,
      known: c.events.length > 0 || c.siteLink.length > 0,
      cause: down ? down.cause : ev?.cause ?? null,
      delay,
      since: down ? down.since : null,
      restore: down ? down.restore ?? ev?.restore ?? null : null,
      until: down ? down.until : null,
      // While up in a pass window: when it is due to close.
      windowEnd: !down && ev?.link === 'up' && ev.windowEnd != null && ev.windowEnd > t ? ev.windowEnd : null,
    };
  }

  /** Earth messages travelling at time t, with the fraction of the trip done. */
  inFlight(t) {
    const out = [];
    for (const [dir, list] of [['up', this.links.up], ['down', this.links.down]]) {
      const end = lastAtOrBefore(list.times, t);
      const begin = Math.max(0, lastAtOrBefore(list.times, t - list.maxTransit - 1));
      const visit = (m) => {
        if (m.t > t || (m.deliver != null && t >= m.deliver)) return;
        // Store and forward: a message waits for the link, then takes one light-time.
        const delay = m.deliver != null ? this.commsAt(m.deliver).delay ?? 0 : 0;
        const depart = m.deliver != null ? Math.max(m.t, m.deliver - delay) : Infinity;
        const frac = t < depart ? 0 : Math.min(1, (t - depart) / Math.max(1, m.deliver - depart));
        out.push({ dir, message: m.message, sent: m.t, deliver: m.deliver, queued: t < depart, frac });
      };
      for (let i = begin; i <= end; i++) if (list.items[i].deliver != null) visit(list.items[i]);
      for (const m of list.undelivered) visit(m);
    }
    return out;
  }

  /** Index of the last feed item at or before t (-1 if none). */
  feedIndexAt(t) {
    return lastAtOrBefore(this.feed.times, t);
  }

  /** Relays holding position at time t: `{robot, x, y, rangeM}`. */
  activeRelays(t) {
    const out = [];
    this.robots.forEach((r, i) => {
      const c = this.activeCommand(i, t);
      if (!c || c.cmd?.type !== 'relay' || !isPoint(c.cmd.position)) return;
      const st = r.track.at(t);
      if (st && Math.hypot(st.x - c.cmd.position.x, st.y - c.cmd.position.y) < 0.75) {
        out.push({ robot: r.id, x: st.x, y: st.y, rangeM: r.spec.relayRange ?? this.baseStation?.rangeM ?? null });
      }
    });
    return out;
  }

  prevDecision(t) {
    const i = lastAtOrBefore(this.decisionTimes, t - 1e-6);
    return i >= 0 ? this.decisionTimes[i] : this.t0;
  }

  nextDecision(t) {
    const i = lastAtOrBefore(this.decisionTimes, t + 1e-6) + 1;
    return i < this.decisionTimes.length ? this.decisionTimes[i] : this.tEnd;
  }

  /** Commands of each outcome so far, for the summary. */
  describeEnd() {
    if (!this.info.endReason) return 'no run_end';
    return `${this.info.endReason.replaceAll('_', ' ')} at ${fmtNumber((this.tEnd - this.t0) / this.info.solLength, 2)} sols`;
  }
}
