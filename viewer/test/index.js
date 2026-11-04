// Entry point for `node --test viewer/test/`. Node 22+ resolves a directory
// argument like a module, which lands here; this loads every test file in
// the directory so the whole suite runs. The test files also run on their
// own: `node --test viewer/test/*.test.js`.

import { readdirSync } from 'node:fs';

const here = new URL('./', import.meta.url);
const files = readdirSync(here)
  .filter((name) => name.endsWith('.test.js'))
  .sort();
for (const name of files) await import(new URL(name, here));
