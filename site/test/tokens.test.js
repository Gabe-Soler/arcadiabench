// The front page takes its colours from site/ui/tokens.css; the replay
// viewer writes the same ladder out per scheme in viewer/tokens.css, because
// its canvases read plain colours. This keeps the two files in step.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');
const norm = (value) => value.trim().toLowerCase().replace(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/, '#$1$1$2$2$3$3');

/** The light and dark halves of `--name: light-dark(a, b);`. */
function sitePair(css, name) {
  const m = css.match(new RegExp(`^\\s*${name}:\\s*light-dark\\((.*)\\);`, 'm'));
  assert.ok(m, `${name} is not a light-dark() token`);
  // Split on the comma that is not inside an rgb(...).
  let depth = 0;
  const at = [...m[1]].findIndex((ch) => {
    if (ch === '(') depth += 1;
    if (ch === ')') depth -= 1;
    return ch === ',' && depth === 0;
  });
  return [norm(m[1].slice(0, at)), norm(m[1].slice(at + 1))];
}

/** A token's value in the viewer's light block and in its dark block. */
function viewerPair(css, name) {
  const split = css.indexOf('@media (prefers-color-scheme: dark)');
  assert.ok(split > 0);
  const value = (block) => {
    const m = block.match(new RegExp(`^\\s*${name}:\\s*([^;]*);`, 'm'));
    assert.ok(m, `${name} is missing`);
    return norm(m[1]);
  };
  return [value(css.slice(0, split)), value(css.slice(split))];
}

const SAME = [
  ['--g-surface', '--gs-bg'],
  ['--g-container', '--gs-card'],
  ['--g-container-high', '--gs-raised'],
  ['--g-container-higher', '--gs-track'],
  ['--g-on-surface', '--gs-ink'],
  ['--g-on-surface-variant', '--gs-grey'],
  ['--g-on-surface-variant', '--gs-text'],
  ['--g-outline', '--gs-line'],
  ['--g-outline-variant', '--gs-line-soft'],
  ['--g-error', '--gs-error-fill'],
  ['--g-on-error', '--gs-on-error'],
];

test('the viewer and the front page share one tonal ladder', () => {
  const site = read('site/ui/tokens.css');
  const viewer = read('viewer/tokens.css');
  for (const [g, gs] of SAME) assert.deepEqual(viewerPair(viewer, gs), sitePair(site, g), `${gs} against ${g}`);
});

test('both pages use the one typeface and no weight outside 400 to 500', () => {
  const viewer = read('viewer/tokens.css');
  // The type family is written once, for both schemes.
  for (const name of ['--gs-font-display', '--gs-font-body']) {
    const m = viewer.match(new RegExp(`^\\s*${name}:\\s*([^;]*);`, 'm'));
    assert.ok(m, `${name} is missing`);
    assert.match(m[1], /^"Google Sans Flex"/);
  }
  for (const path of ['site/site.css', 'viewer/style.css']) {
    const weights = [...read(path).matchAll(/font-weight:\s*(\d+)/g)].map((m) => +m[1]);
    assert.ok(weights.length > 0);
    assert.deepEqual(weights.filter((w) => w < 400 || w > 500), [], path);
  }
});
