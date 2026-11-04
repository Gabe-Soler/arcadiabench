// Shared by the tests that load real logs: query everything the viewer draws
// at many times across a run, and check that each answer makes sense.

import assert from 'node:assert/strict';

/** Query everything the viewer draws at `steps + 1` times across the run. */
export function sweep(m, steps = 300) {
  for (let k = 0; k <= steps; k++) {
    const t = m.t0 + ((m.tEnd - m.t0) * k) / steps;
    for (let i = 0; i < m.robots.length; i++) {
      const r = m.robotAt(i, t);
      assert.ok(Number.isFinite(r.x) && Number.isFinite(r.y), `${r.id} at ${t}`);
      assert.ok(r.x >= 0 && r.y >= 0 && r.x < m.site.width && r.y < m.site.height);
    }
    const pad = m.padAt(t);
    assert.ok(pad.verified <= pad.total && pad.inSpec <= pad.total);
    m.powerAt(t);
    m.weatherAt(t);
    m.commsAt(t);
    m.inFlight(t);
    m.activeRelays(t);
    m.feedIndexAt(t);
  }
}
