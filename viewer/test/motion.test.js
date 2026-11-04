import { test } from 'node:test';
import assert from 'node:assert/strict';
import { serpentine, commandWaypoints, schedule, buildTrack } from '../src/motion.js';

const snaps = (rows) => ({
  t: rows.map((r) => r[0]),
  x: rows.map((r) => r[1]),
  y: rows.map((r) => r[2]),
  status: rows.map((r) => r[3] ?? 'idle'),
  payload: rows.map((r) => r[4] ?? 0),
});
const ctx = (extra = {}) => (cmd, start) => ({
  start,
  stations: new Map([['st-1', { x: 0, y: 0 }]]),
  depots: new Map([['dp-1', { x: 0, y: 9 }]]),
  bounds: { width: 60, height: 60 },
  robotPos: (id) => extra.robots?.[id] ?? null,
});
const near = (p, x, y, tol = 1e-6) => Math.abs(p.x - x) <= tol && Math.abs(p.y - y) <= tol;

test('serpentine sweeps rows in alternating directions', () => {
  assert.deepEqual(
    serpentine({ min: { x: 1, y: 1 }, max: { x: 3, y: 2 } }),
    [
      { x: 1, y: 1 },
      { x: 2, y: 1 },
      { x: 3, y: 1 },
      { x: 3, y: 2 },
      { x: 2, y: 2 },
      { x: 1, y: 2 },
    ],
  );
  assert.deepEqual(serpentine({ min: { x: 3, y: 0 }, max: { x: 1, y: 0 } }), []);
});

test('each command type yields the waypoints it implies', () => {
  const c = ctx({ robots: { 'cp-1': { x: 7, y: 7 } } })(null, { x: 5, y: 5 });
  const pts = (cmd) => commandWaypoints(cmd, c).points;
  assert.deepEqual(pts({ type: 'move_to', target: { x: 9, y: 1 } }).map((p) => [p.x, p.y]), [[9, 1]]);
  const haul = pts({ type: 'haul', from: { x: 45, y: 30 }, to: { x: 20, y: 20 }, mass_kg: 400 });
  assert.deepEqual(haul.map((p) => [p.x, p.y, p.load ?? null, p.dump ?? false]), [
    [45, 30, 400, false],
    [20, 20, null, true],
  ]);
  const scan = pts({ type: 'scan', area: { min: { x: 20, y: 20 }, max: { x: 39, y: 39 } } });
  assert.deepEqual(scan.map((p) => [p.x, p.y, p.scan]), [[20, 20, true]], 'scans from the nearest cell of the area');
  assert.equal(pts({ type: 'dig', area: { min: { x: 0, y: 0 }, max: { x: 3, y: 1 } }, target_elevation_m: 0 }).length, 8);
  assert.deepEqual(pts({ type: 'charge', source: 'st-1' }).map((p) => [p.x, p.y, p.status]), [[0, 0, 'charging']]);
  assert.deepEqual(pts({ type: 'service', depot: 'dp-1' }).map((p) => [p.x, p.y]), [[0, 9]]);
  const tow = pts({ type: 'tow', target_robot: 'cp-1', to: { x: 2, y: 2 } });
  assert.deepEqual(tow.map((p) => [p.x, p.y, p.pickup ?? null]), [
    [7, 7, 'cp-1'],
    [2, 2, null],
  ]);
  assert.equal(pts({ type: 'repair', target_robot: 'cp-1' })[0].near, true);
  assert.deepEqual(pts({ type: 'wait', duration_s: 10 }), []);
  assert.equal(commandWaypoints({ type: 'wait', duration_s: 10 }, c).holdStatus, 'idle');
  assert.deepEqual(pts({ type: 'teleport', to: { x: 1, y: 1 } }), [], 'unknown commands have no waypoints');
  assert.equal(pts({ type: 'dig', area: { min: { x: 58, y: 58 }, max: { x: 99, y: 99 } } }).length, 4, 'areas are clipped to the site');
});

test('schedule drives at top speed and shares the spare time among dwells by weight', () => {
  const path = [
    { x: 0, y: 0 },
    { x: 10, y: 0, w: 1 },
    { x: 10, y: 10, w: 3 },
  ];
  const s = schedule(path, 0, 100, 1);
  assert.deepEqual(s.arrive, [0, 10, 40]);
  assert.deepEqual(s.leave, [0, 30, 100]);
  const tight = schedule(path, 0, 10, 1);
  assert.deepEqual(tight.arrive, [0, 5, 10], 'without enough time it drives the whole way at the speed that fits');
  const noDwell = schedule([{ x: 0, y: 0 }, { x: 4, y: 0 }], 0, 8, 1);
  assert.deepEqual(noDwell.arrive, [0, 8], 'without dwells the drive fills the interval');
});

test('a haul visits its source between two snapshots, carrying the load back', () => {
  const s = snaps([
    [0, 14, 10],
    [452, 20, 20],
  ]);
  const cmd = { id: 'h', cmd: { type: 'haul', from: { x: 45, y: 30 }, to: { x: 20, y: 20 }, mass_kg: 400 }, start: 0, end: 452, completed: true };
  const { track, work } = buildTrack(s, [cmd], { speed: 0.3, capacity: 400, ctxFor: ctx() });
  let visited = null;
  for (let t = 0; t <= 452; t += 2) {
    const st = track.at(t);
    if (near(st, 45, 30)) {
      visited = st;
      break;
    }
  }
  assert.ok(visited, 'the hauler reaches the stockpile');
  assert.equal(visited.status, 'working', 'and loads there');
  const back = track.at(452 - 100);
  assert.ok(back.x < 45 && back.payload === 400, 'heads back with 400 kg');
  assert.ok(near(track.at(0), 14, 10) && near(track.at(452), 20, 20), 'snapshots are exact');
  assert.equal(track.at(452).payload, 0, 'dumped at the end');
  assert.deepEqual(work.map((w) => [w.x, w.y]), [
    [45, 30],
    [20, 20],
  ]);
  assert.ok(work[0].t < work[1].t && work[1].t <= 452);
});

test('a dig sweeps along its row, finishing cells in order', () => {
  const s = snaps([
    [0, 39, 20],
    [350, 39, 21],
  ]);
  const cmd = { id: 'd', cmd: { type: 'dig', area: { min: { x: 20, y: 21 }, max: { x: 39, y: 21 } }, target_elevation_m: 0 }, start: 0, end: 350, completed: true };
  const { track, work } = buildTrack(s, [cmd], { speed: 0.15, ctxFor: ctx() });
  assert.equal(work.length, 20);
  assert.deepEqual(work.map((w) => w.x), Array.from({ length: 20 }, (_, i) => 20 + i));
  for (let i = 1; i < work.length; i++) assert.ok(work[i].t >= work[i - 1].t);
  const mid = track.at(250);
  assert.equal(mid.y, 21);
  assert.ok(mid.x > 20 && mid.x < 39, 'partway along the row');
  assert.ok(['moving', 'working'].includes(mid.status));
  assert.equal(track.at(350).status, 'idle', 'the end snapshot status takes over');
});

test('a snapshot in mid-haul is placed on the stretch the timing implies', () => {
  // Out and back along nearly the same line: (32, 25) lies on both stretches.
  const s = snaps([
    [0, 20, 20, 'idle'],
    [380, 32, 25, 'moving'],
    [450, 20, 21, 'idle'],
  ]);
  const cmd = { id: 'h', cmd: { type: 'haul', from: { x: 45, y: 30 }, to: { x: 20, y: 21 }, mass_kg: 400 }, start: 0, end: 450, completed: true };
  const { track } = buildTrack(s, [cmd], { speed: 0.3, capacity: 400, ctxFor: ctx() });
  // With the snapshot on the return stretch, the stockpile is reached before 380.
  let reached = false;
  for (let t = 0; t < 380; t += 1) if (near(track.at(t), 45, 30)) reached = true;
  assert.ok(reached);
  assert.ok(near(track.at(380), 32, 25));
});

test('snapshots that do not fit the path are honoured with a straight move', () => {
  const s = snaps([
    [0, 0, 0],
    [50, 0, 30, 'moving'],
    [100, 10, 0],
  ]);
  const cmd = { id: 'm', cmd: { type: 'move_to', target: { x: 10, y: 0 } }, start: 0, end: 100, completed: true };
  const { track } = buildTrack(s, [cmd], { speed: 1, ctxFor: ctx() });
  assert.ok(near(track.at(50), 0, 30));
  assert.ok(near(track.at(25), 0, 15), 'straight towards the off-path snapshot');
  assert.ok(near(track.at(100), 10, 0));
});

test('robots without commands interpolate between snapshots; waits hold still and idle', () => {
  const s = snaps([
    [0, 0, 0, 'moving'],
    [10, 10, 0, 'moving'],
    [20, 10, 0, 'idle'],
    [40, 10, 0, 'idle'],
  ]);
  const wait = { id: 'w', cmd: { type: 'wait', duration_s: 20 }, start: 20, end: 40, completed: true };
  const { track } = buildTrack(s, [wait], { speed: 1, ctxFor: ctx() });
  assert.ok(near(track.at(5), 5, 0));
  assert.equal(track.at(5).status, 'moving');
  assert.ok(near(track.at(30), 10, 0));
  assert.equal(track.at(30).status, 'idle');
});

test('an unfinished command uses its estimate to lay out the path', () => {
  const s = snaps([
    [0, 0, 0],
    [10, 5, 0, 'moving'],
  ]);
  const cmd = { id: 'm', cmd: { type: 'move_to', target: { x: 20, y: 0 } }, start: 0, end: 10, completed: false, est: 40 };
  const { track } = buildTrack(s, [cmd], { speed: 0.5, ctxFor: ctx() });
  assert.ok(near(track.at(10), 5, 0));
  assert.ok(near(track.at(5), 2.5, 0));
});

test('scan windows start when the scanner arrives', () => {
  const s = snaps([
    [0, 12, 10],
    [100, 20, 20],
  ]);
  const cmd = { id: 's', cmd: { type: 'scan', area: { min: { x: 20, y: 20 }, max: { x: 39, y: 39 } } }, start: 0, end: 100, completed: true };
  const { scans } = buildTrack(s, [cmd], { speed: 0.5, ctxFor: ctx() });
  assert.equal(scans.length, 1);
  assert.ok(Math.abs(scans[0].start - Math.hypot(8, 10) / 0.5) < 1e-9);
  assert.equal(scans[0].end, 100);
});
