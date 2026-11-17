#!/usr/bin/env node
// Turn what `arcadiabench suite` wrote for an entry into the two files the
// leaderboard keeps for it: report.json and runs.json.
//
//   node site/tools/entry-results.mjs ENTRY_JSON OUT_DIR SUITE_DIR [SUITE_DIR...]
//
// A suite directory holds suite.json (every scorecard and the resilience) and
// one directory per scenario with its metrics.jsonl. An entry that took both
// tracks has two: its Standard track suite and its fleet design track suite.
//
// The coordinator's name and version in the results are what the coordinator
// said of itself in `welcome`. They are replaced by the entry's id and
// version: an entry is named by its entry file, not by its own program.

import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const json = (path) => JSON.parse(readFileSync(path, 'utf8'));

function check(condition, message) {
  if (!condition) throw new Error(message);
}

/** The report and the per-seed scores of `entry`, from its suite directories. */
export function entryResults(entry, suiteDirs) {
  const who = { name: entry.id, version: entry.version };
  const report = { scorecards: [], resilience: [] };
  const scenarios = {};
  for (const dir of suiteDirs) {
    const suite = json(join(dir, 'suite.json'));
    check(Array.isArray(suite.scorecards) && Array.isArray(suite.resilience), `${dir}/suite.json is not a report`);
    for (const card of suite.scorecards) report.scorecards.push({ ...card, coordinator: who });
    for (const r of suite.resilience) report.resilience.push({ ...r, coordinator: who });
    for (const name of readdirSync(dir).sort()) {
      const metrics = join(dir, name, 'metrics.jsonl');
      if (!statSync(join(dir, name)).isDirectory() || !existsSync(metrics)) continue;
      const records = readFileSync(metrics, 'utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line));
      check(records.length > 0, `${metrics} is empty`);
      if (records[0].variant !== 'nominal') continue;
      const id = records[0].scenario_id;
      for (const r of records) {
        check(r.scenario_id === id, `${metrics} mixes scenarios`);
        check(Number.isSafeInteger(r.seed) && r.seed >= 0, `${metrics}: bad seed`);
        check(typeof r.score === 'number' && r.score >= 0 && r.score <= 1, `${metrics}: bad score`);
      }
      check(!(id in scenarios), `${id} appears in two suites`);
      scenarios[id] = { seeds: records.map((r) => r.seed), scores: records.map((r) => r.score) };
    }
  }
  check(report.scorecards.length > 0, 'no scorecards');
  return { report, runs: { scenarios } };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [entryPath, out, ...suites] = process.argv.slice(2);
  if (!entryPath || !out || suites.length === 0) {
    console.error('usage: entry-results.mjs ENTRY_JSON OUT_DIR SUITE_DIR [SUITE_DIR...]');
    process.exit(2);
  }
  try {
    const { report, runs } = entryResults(json(entryPath), suites);
    mkdirSync(out, { recursive: true });
    writeFileSync(join(out, 'report.json'), `${JSON.stringify(report)}\n`);
    writeFileSync(join(out, 'runs.json'), `${JSON.stringify(runs)}\n`);
    console.log(`entry-results: ${report.scorecards.length} scorecards and ${Object.keys(runs.scenarios).length} nominal scenarios -> ${out}`);
  } catch (e) {
    console.error(`entry-results: ${e.message}`);
    process.exit(1);
  }
}
