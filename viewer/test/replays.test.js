// The demo replays in results/v1.0/replays/ and results/v1.1/replays/
// (docs/demos.md), which the Pages workflow publishes beside the viewer:
// every one that is present loads through the same gzip path as the browser,
// with no bad lines and no warnings, and replays sensibly from start to end.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { parseLog, openBytes } from '../src/parse.js';
import { ModelBuilder } from '../src/model.js';
import { sweep } from './sweep.js';

const replays = [];
for (const version of ['v1.0', 'v1.1']) {
  const dir = new URL(`../../results/${version}/replays/`, import.meta.url);
  if (!existsSync(dir)) continue;
  for (const name of readdirSync(dir).filter((f) => f.endsWith('.jsonl.gz')).sort()) {
    replays.push({ version, name, url: new URL(name, dir) });
  }
}

for (const { version, name, url } of replays) {
  test(`${version} replay ${name} loads cleanly and replays from start to end`, async () => {
    const bytes = readFileSync(url);
    const b = new ModelBuilder();
    const result = await parseLog(openBytes(bytes, name), { onRecord: (r, l) => b.add(r, l) });
    const m = b.finish();
    assert.equal(result.gzip, true);
    assert.equal(result.badLines, 0);
    assert.deepEqual(m.warnings, []);
    assert.equal(m.info.coordinator.name, 'arcadiaos');
    assert.equal(m.info.endReason, 'goals_complete');
    assert.equal(m.finalInstant, true);
    assert.equal(m.decisionCount, m.info.metrics.decisions);
    const end = m.padAt(m.tEnd);
    assert.equal(end.inSpec, end.total);
    assert.equal(end.verified, end.total);
    sweep(m, 200);
  });
}
