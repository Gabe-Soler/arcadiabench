import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseLog, openBytes, isGzip, LogError } from '../src/parse.js';

const start = { t: 0, kind: 'run_start', protocol_version: '0.1.0' };
const lines = (...recs) => recs.map((r) => (typeof r === 'string' ? r : JSON.stringify(r))).join('\n');

async function parse(text, name = 'test.jsonl') {
  const records = [];
  const result = await parseLog(openBytes(text, name), { onRecord: (r, line) => records.push([line, r]) });
  return { records, result };
}

async function gzip(text) {
  const stream = new Blob([text]).stream().pipeThrough(new CompressionStream('gzip'));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

test('records arrive in order with their line numbers; blank lines and CRLF are fine', async () => {
  const text = `﻿${JSON.stringify(start)}\r\n\r\n${JSON.stringify({ t: 1, kind: 'truth' })}\r\n${JSON.stringify({ t: 2, kind: 'run_end', reason: 'time_limit' })}`;
  const { records, result } = await parse(text);
  assert.deepEqual(
    records.map(([line, r]) => [line, r.kind]),
    [
      [1, 'run_start'],
      [3, 'truth'],
      [4, 'run_end'],
    ],
  );
  assert.equal(result.records, 3);
  assert.equal(result.badLines, 0);
  assert.equal(result.gzip, false);
});

test('lines longer than a read chunk are reassembled', async () => {
  const big = { t: 0, kind: 'run_start', padding: 'x'.repeat(300_000) };
  const { records } = await parse(lines(big, { t: 1, kind: 'truth' }));
  assert.equal(records.length, 2);
  assert.equal(records[0][1].padding.length, 300_000);
});

test('bad lines are skipped and reported with line numbers; a cut-off last line is flagged', async () => {
  const text = `${lines(start, '{not json', '[1, 2]', { t: 3, kind: 'truth' })}\n{"t": 4, "kind": "tru`;
  const { records, result } = await parse(text);
  assert.equal(records.length, 2);
  assert.equal(result.badLines, 3);
  assert.equal(result.errors[0].line, 2);
  assert.match(result.errors[0].message, /not valid JSON/);
  assert.match(result.errors[1].message, /not a log record/);
  assert.equal(result.truncatedTail, true);
  assert.match(result.errors[2].message, /incomplete/);
});

test('unknown record kinds are passed through for the model to ignore', async () => {
  const { records } = await parse(lines(start, { t: 1, kind: 'from_the_future', extra: true }));
  assert.equal(records[1][1].kind, 'from_the_future');
});

test('gzip input is detected by content and decompressed', async () => {
  const text = lines(start, { t: 5, kind: 'truth' });
  const bytes = await gzip(text);
  assert.ok(isGzip(bytes));
  const records = [];
  const result = await parseLog(openBytes(bytes, 'log.jsonl.gz'), { onRecord: (r) => records.push(r) });
  assert.equal(result.gzip, true);
  assert.deepEqual(
    records.map((r) => r.kind),
    ['run_start', 'truth'],
  );
});

test('clear errors for empty, foreign and corrupt input', async () => {
  await assert.rejects(parse('', 'empty.jsonl'), (e) => e instanceof LogError && /empty/.test(e.message));
  await assert.rejects(parse('\n\n  \n', 'blank.jsonl'), (e) => e instanceof LogError && /empty/.test(e.message));
  await assert.rejects(
    parse(lines({ t: 0, kind: 'truth' })),
    (e) => e instanceof LogError && /not "run_start"/.test(e.message) && /arcadiabench run/.test(e.hint),
  );
  await assert.rejects(parse('hello world\nsecond line'), (e) => e instanceof LogError && /No log records/.test(e.message) && /line 1/.test(e.message));
  const bytes = await gzip(lines(start, { t: 1, kind: 'truth' }));
  const corrupt = bytes.slice(0, 12);
  await assert.rejects(
    parseLog(openBytes(corrupt, 'cut.jsonl.gz')),
    (e) => e instanceof LogError && /decompress/.test(e.message),
  );
});

test('progress is reported in raw bytes', async () => {
  const seen = [];
  const text = lines(start, { t: 1, kind: 'truth' });
  await parseLog(openBytes(text), { onProgress: (p) => seen.push(p), yieldMs: 0 });
  const last = seen[seen.length - 1];
  assert.equal(last.bytes, new TextEncoder().encode(text).length);
  assert.equal(last.records, 2);
});
