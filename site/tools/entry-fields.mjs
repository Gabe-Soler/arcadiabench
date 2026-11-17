#!/usr/bin/env node
// What the leaderboard's workflow needs to know of an entry, checked before
// any of it is used.
//
//   node site/tools/entry-fields.mjs ID/VERSION     # prints key=value lines
//
// An entry file is written by a stranger, and the workflow passes what it
// says to git and docker. So every value is matched against a pattern here,
// and the workflow reads them as environment variables, never as text pasted
// into a command. The patterns are the entry schema's
// (schemas/leaderboard-entry.schema.json).

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

const PATTERNS = {
  id: /^[a-z0-9][a-z0-9-]{1,39}$/,
  version: /^[0-9A-Za-z][0-9A-Za-z.+-]{0,31}$/,
  repository: /^https:\/\/github\.com\/[A-Za-z0-9_.-]{1,60}\/[A-Za-z0-9_.-]{1,100}$/,
  commit: /^[0-9a-f]{40}$/,
  dockerfile: /^[A-Za-z0-9_-][A-Za-z0-9_.-]*(\/[A-Za-z0-9_-][A-Za-z0-9_.-]*)*$/,
  image_digest: /^sha256:[0-9a-f]{64}$/,
};

function checked(name, value) {
  if (typeof value !== 'string' || !PATTERNS[name].test(value)) throw new Error(`the entry's ${name} is not acceptable`);
  return value;
}

/** Split `id/version` as given to the workflow. */
export function parseRef(ref) {
  const parts = String(ref).split('/');
  if (parts.length !== 2) throw new Error('expected ID/VERSION');
  return { id: checked('id', parts[0]), version: checked('version', parts[1]) };
}

/** The fields of an entry the workflow uses, each checked. */
export function entryFields(entry, where) {
  const fields = {
    id: checked('id', entry.id),
    version: checked('version', entry.version),
    repository: checked('repository', entry?.source?.repository),
    commit: checked('commit', entry?.source?.commit),
    dockerfile: checked('dockerfile', entry?.source?.dockerfile ?? 'Dockerfile'),
  };
  if (where && (fields.id !== where.id || fields.version !== where.version)) {
    throw new Error(`the entry file says ${fields.id}/${fields.version}`);
  }
  if (!Array.isArray(entry.tracks) || entry.tracks.length === 0 || entry.tracks.some((t) => t !== 'standard' && t !== 'fleet')) {
    throw new Error("the entry's tracks are not acceptable");
  }
  fields.tracks = entry.tracks.join(',');
  // The image's name in the project's registry: lower case throughout.
  fields.image_name = `arcadiabench-entry-${fields.id}`;
  fields.image_tag = fields.version.toLowerCase().replaceAll('+', '-');
  fields.image_digest = entry.image_digest === undefined ? '' : checked('image_digest', entry.image_digest);
  fields.dir = `leaderboard/entries/${fields.id}/${fields.version}`;
  return fields;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const where = parseRef(process.argv[2] ?? '');
    const entry = JSON.parse(readFileSync(join(root, 'leaderboard/entries', where.id, where.version, 'entry.json'), 'utf8'));
    for (const [key, value] of Object.entries(entryFields(entry, where))) console.log(`${key}=${value}`);
  } catch (e) {
    console.error(`entry-fields: ${e.message}`);
    process.exit(1);
  }
}
