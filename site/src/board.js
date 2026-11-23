// Draws the board into the page. Everything an entry says about itself is
// written as text, never as markup.

import { barWidth, crewCounts, finished, fmt3, metaLine, presetName, rankLabel, safeHref, tonnes, variantName } from './format.js';

/** What the public seeds are called in a column head and a link. */
const PUBLIC = 'Seeds 1–50';

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text != null) node.textContent = text;
  return node;
}

/** A glyph from the icon font, hidden from assistive technology. */
function icon(name, cls = '') {
  const span = el('span', `g-icon ${cls}`.trim(), name);
  span.setAttribute('aria-hidden', 'true');
  return span;
}

function labelled(label, value, cls = 'num') {
  const span = el('span', cls);
  span.append(el('small', null, label), document.createTextNode(value));
  return span;
}

function columns(track, round) {
  const cols = el('div', 'cols');
  cols.setAttribute('aria-hidden', 'true');
  cols.append(el('span'), el('span', null, 'Coordinator'), el('span', null, round ? `Round ${round}` : 'Headline'));
  if (round) cols.append(el('span', 'num pub', PUBLIC));
  if (track === 'fleet') cols.append(el('span', 'crew-col', 'Crew'));
  else cols.append(el('span', 'num p', 'Small'), el('span', 'num p', 'Medium'), el('span', 'num p', 'Large'));
  cols.append(el('span', 'num', 'Resilience'), el('span', 'num', 'Finished'), el('span'));
  return cols;
}

function summary(row, track, best, round) {
  const s = el('summary');
  s.append(el('span', 'rank', rankLabel(row)));

  const who = el('div', 'who');
  const name = el('div', 'name', row.name);
  if (row.baseline) name.append(el('span', 'g-badge g-badge--sm', 'Baseline'));
  who.append(name, el('div', 'meta', metaLine(row)));
  s.append(who);

  const score = el('div', 'score');
  const bar = el('div', 'g-progress');
  const fill = el('span');
  fill.style.width = `${barWidth(row.headline, best)}%`;
  bar.append(fill);
  score.append(el('span', 'score-n', fmt3(row.headline)), bar);
  s.append(score);

  const mob = el('div', 'mob');
  // On a round's board, the same entry's headline on the public seeds.
  if (round) {
    s.append(el('span', 'num pub', fmt3(row.public?.headline)));
    mob.append(labelled(PUBLIC, fmt3(row.public?.headline)));
  }
  if (track === 'fleet') {
    const crew = el('span', 'crew crew-col', `${crewCounts(row.crew)} · `);
    crew.append(el('small', null, tonnes(row.mass_kg)));
    s.append(crew);
    mob.append(el('span', 'num', `${crewCounts(row.crew)} · ${tonnes(row.mass_kg)}`));
  } else {
    for (const p of ['small', 'medium', 'large']) {
      s.append(el('span', 'num p', fmt3(row.presets[p])));
      mob.append(labelled(presetName(p), fmt3(row.presets[p])));
    }
  }
  s.append(el('span', 'num', fmt3(row.resilience)), el('span', 'num', finished(row)));
  mob.append(labelled('Resilience', fmt3(row.resilience)), labelled('Finished', finished(row)));
  s.append(icon('expand_more', 'chev'), mob);
  return s;
}

function scenarios(row) {
  const table = el('table', 'g-table');
  const head = el('tr');
  head.append(el('th'));
  for (const c of row.grid[0].cells) head.append(el('th', null, variantName(c.variant)));
  const thead = el('thead');
  thead.append(head);
  const body = el('tbody');
  for (const line of row.grid) {
    const tr = el('tr');
    const th = el('th', null, presetName(line.preset));
    th.scope = 'row';
    tr.append(th);
    for (const c of line.cells) {
      const td = el('td', null, fmt3(c.headline));
      if (c.success_rate < 1) td.append(el('small', null, ` ${Math.round(c.success_rate * 100)}%`));
      tr.append(td);
    }
    body.append(tr);
  }
  table.append(thead, body);
  const wrap = el('div', 'g-table-wrap');
  wrap.append(table);
  return wrap;
}

/** A text link with its chevron, or nothing if the address is not one the
 * page follows. */
function link(label, href) {
  const safe = safeHref(href);
  if (!safe) return null;
  const a = el('a', 'g-link', label);
  a.href = safe;
  a.append(icon('chevron_right'));
  return a;
}

function detail(row) {
  const d = el('div', 'detail');
  if (row.summary) d.append(el('p', null, row.summary));
  // On the fleet design track a crew may be another entry's design.
  if (row.designed_by && row.designed_by !== row.id) {
    d.append(el('p', 'g-small', `It runs the crew that ${row.designed_by} designed.`));
  }
  d.append(scenarios(row));
  const links = el('div', 'links');
  for (const a of [link('Scorecards', row.links.scorecards), link(PUBLIC, row.public?.scorecards), link('Watch a run', row.links.replay), link('Source', row.links.source)]) {
    if (a) links.append(a);
  }
  d.append(links);
  return d;
}

/** Draw one track of `board` into `container`. On a round's board the
 * entries accepted since the round wait under the ranks, with their figures
 * on the public seeds. */
export function renderBoard(container, board, track) {
  const rows = board.tracks[track]?.rows ?? [];
  const pending = board.tracks[track]?.pending ?? [];
  const round = board.basis?.round ?? null;
  container.dataset.track = track;
  container.dataset.basis = round ? 'round' : 'public';
  if (rows.length + pending.length === 0) {
    container.replaceChildren(el('p', 'board-note', 'No entries on this track yet.'));
    return;
  }
  const best = (rows[0] ?? pending[0]).headline;
  const nodes = [columns(track, round)];
  const add = (row, cls, onRound) => {
    const item = el('details', cls);
    item.append(summary(row, track, best, onRound), detail(row));
    nodes.push(item);
  };
  for (const row of rows) add(row, 'row', round);
  if (pending.length > 0) {
    nodes.push(el('p', 'board-group', `Accepted since round ${round}: on seeds 1–50, and ranked in the next round`));
    // Their headline is on the public seeds, so it goes in that column.
    for (const row of pending) add({ ...row, public: { headline: row.headline, scorecards: null }, headline: NaN }, 'row row--pending', round);
  }
  container.replaceChildren(...nodes);
}
