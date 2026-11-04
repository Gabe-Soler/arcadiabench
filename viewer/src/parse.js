// Reading event logs: bytes from a URL or a local file, gzip or not, then one
// JSON record per line. Uses only web-standard APIs (fetch, streams,
// DecompressionStream, TextDecoder), so it runs unchanged in Node tests.

/** A problem with the input, phrased for the person who opened it. */
export class LogError extends Error {
  constructor(message, { hint, cause } = {}) {
    super(message, cause ? { cause } : undefined);
    this.name = 'LogError';
    this.hint = hint;
  }
}

/** Open a log by URL (relative URLs resolve against `base`). */
export async function openUrl(url, { base, fetchImpl = globalThis.fetch, signal } = {}) {
  let href = url;
  try {
    href = base ? new URL(url, base).href : url;
  } catch {
    throw new LogError(`"${url}" is not a valid URL.`);
  }
  let res;
  try {
    res = await fetchImpl(href, { signal });
  } catch (e) {
    const fileHint =
      typeof location !== 'undefined' && location.protocol === 'file:'
        ? 'Pages opened from file:// cannot fetch logs. Serve the viewer with a static server, for example `python3 -m http.server`, or drop the file onto the page.'
        : 'Check the address, and that the server allows cross-origin requests if the log is on another site.';
    throw new LogError(`Could not fetch ${url}: ${e.message}`, { hint: fileHint, cause: e });
  }
  if (!res.ok) {
    throw new LogError(`Could not fetch ${url}: HTTP ${res.status}${res.statusText ? ` ${res.statusText}` : ''}.`, {
      hint: res.status === 404 ? 'The file does not exist at that address. Paths are relative to the viewer page.' : undefined,
    });
  }
  if (!res.body) throw new LogError(`The response for ${url} has no body.`);
  const len = Number(res.headers.get('content-length'));
  return { stream: res.body, total: Number.isFinite(len) && len > 0 ? len : null, name: url };
}

/** Open a local File (from a picker or a drop). */
export function openFile(file) {
  return { stream: file.stream(), total: file.size || null, name: file.name };
}

/** Open an in-memory string or byte array (tests, pasted text). */
export function openBytes(data, name = 'input') {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
  const stream = new ReadableStream({
    start(controller) {
      // Feed in modest chunks so chunk-boundary handling is exercised.
      const step = 64 * 1024;
      for (let i = 0; i < bytes.length; i += step) controller.enqueue(bytes.subarray(i, i + step));
      controller.close();
    },
  });
  return { stream, total: bytes.length, name };
}

/** True when the bytes start with the gzip magic number. */
export function isGzip(bytes) {
  return bytes.length >= 2 && bytes[0] === 0x1f && bytes[1] === 0x8b;
}

function concatBytes(a, b) {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}

/**
 * Sniff the first bytes of a raw stream and decompress it if it is gzip.
 * Detection is by content, not by file name, because some servers decode
 * `.gz` files on the fly and others do not. `onBytes` sees raw byte counts.
 */
export async function decodedStream(raw, onBytes) {
  const reader = raw.getReader();
  let head = new Uint8Array(0);
  let ended = false;
  while (head.length < 2) {
    const r = await reader.read();
    if (r.done) {
      ended = true;
      break;
    }
    onBytes?.(r.value.byteLength);
    head = concatBytes(head, r.value);
  }
  const replay = new ReadableStream({
    start(controller) {
      if (head.length) controller.enqueue(head);
      if (ended) controller.close();
    },
    async pull(controller) {
      const r = await reader.read();
      if (r.done) {
        controller.close();
      } else {
        onBytes?.(r.value.byteLength);
        controller.enqueue(r.value);
      }
    },
    cancel(reason) {
      return reader.cancel(reason);
    },
  });
  if (!isGzip(head)) return { stream: replay, gzip: false };
  if (typeof DecompressionStream === 'undefined') {
    throw new LogError('This log is gzip-compressed, but this browser cannot decompress gzip.', {
      hint: 'Use a current browser, or open the uncompressed .jsonl file.',
    });
  }
  return { stream: replay.pipeThrough(new DecompressionStream('gzip')), gzip: true };
}

const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now());
const yieldToEventLoop = () => new Promise((resolve) => setTimeout(resolve, 0));

/**
 * Parse a JSONL event log. Calls `onRecord(record, lineNo)` for every record
 * in file order, yields to the event loop every `yieldMs` so a page stays
 * responsive on big files, and reports `onProgress({bytes, total, records})`.
 *
 * Blank lines are skipped. Lines that are not JSON objects with a string
 * `kind` are counted and reported, not fatal, unless nothing usable is found
 * or the first record is not `run_start`.
 */
export async function parseLog(source, { onRecord = () => {}, onProgress, yieldMs = 30, maxErrors = 20 } = {}) {
  let rawBytes = 0;
  const { stream, gzip } = await decodedStream(source.stream, (n) => {
    rawBytes += n;
  });
  const reader = stream.getReader();
  const decoder = new TextDecoder('utf-8');
  const errors = [];
  let lineNo = 0;
  let records = 0;
  let badLines = 0;
  let firstSeen = false;
  let truncatedTail = false;
  let lastYield = now();
  const pending = [];

  const bad = (message) => {
    badLines++;
    if (errors.length < maxErrors) errors.push({ line: lineNo, message });
  };

  const handleLine = (line, isTail = false) => {
    lineNo++;
    if (line.endsWith('\r')) line = line.slice(0, -1);
    if (lineNo === 1 && line.charCodeAt(0) === 0xfeff) line = line.slice(1);
    if (line.trim() === '') return;
    let rec;
    try {
      rec = JSON.parse(line);
    } catch (e) {
      if (isTail) truncatedTail = true;
      bad(isTail ? 'the last line is incomplete (truncated log?)' : `not valid JSON (${e.message})`);
      return;
    }
    if (!rec || typeof rec !== 'object' || Array.isArray(rec) || typeof rec.kind !== 'string') {
      bad('not a log record (expected a JSON object with a "kind")');
      return;
    }
    if (!firstSeen) {
      firstSeen = true;
      if (rec.kind !== 'run_start') {
        throw new LogError(
          `This does not look like an ArcadiaBench run log: the first record is "${rec.kind}", not "run_start".`,
          { hint: 'Event logs are written by `arcadiabench run --log FILE`.' },
        );
      }
    }
    records++;
    onRecord(rec, lineNo);
  };

  try {
    for (;;) {
      let r;
      try {
        r = await reader.read();
      } catch (e) {
        if (e instanceof LogError) throw e;
        throw new LogError(
          gzip ? `Could not decompress ${source.name}: ${e.message}` : `Could not read ${source.name}: ${e.message}`,
          { hint: gzip ? 'The gzip data is corrupt or incomplete.' : undefined, cause: e },
        );
      }
      if (r.done) break;
      const text = decoder.decode(r.value, { stream: true });
      let start = 0;
      for (;;) {
        const nl = text.indexOf('\n', start);
        if (nl < 0) {
          if (start < text.length) pending.push(start === 0 ? text : text.slice(start));
          break;
        }
        let line = text.slice(start, nl);
        if (pending.length) {
          pending.push(line);
          line = pending.join('');
          pending.length = 0;
        }
        handleLine(line);
        start = nl + 1;
      }
      if (now() - lastYield > yieldMs) {
        onProgress?.({ bytes: rawBytes, total: source.total, records });
        await yieldToEventLoop();
        lastYield = now();
      }
    }
    pending.push(decoder.decode());
    const tail = pending.join('');
    if (tail.trim() !== '') handleLine(tail, true);
  } finally {
    try {
      reader.releaseLock();
    } catch {
      // Already released.
    }
  }

  if (records === 0) {
    if (badLines === 0) throw new LogError(`${source.name} is empty.`);
    const first = errors[0];
    throw new LogError(
      `No log records found in ${source.name}${first ? `: line ${first.line} is ${first.message}` : ''}.`,
      { hint: 'Expected one JSON record per line (JSONL), as written by `arcadiabench run --log FILE`.' },
    );
  }
  onProgress?.({ bytes: rawBytes, total: source.total, records });
  return { records, lines: lineNo, badLines, errors, truncatedTail, gzip, bytes: rawBytes };
}
