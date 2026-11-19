// The front page: loads leaderboard/board.json and draws the track chosen.

import { renderBoard } from './board.js';
import { basisNote } from './format.js';

const board = document.getElementById('board');
const tabs = [...document.querySelectorAll('.board-tracks [role="tab"]')];
let data = null;

function show(track) {
  for (const tab of tabs) {
    const on = tab.dataset.track === track;
    tab.setAttribute('aria-selected', String(on));
    tab.tabIndex = on ? 0 : -1;
    if (on) board.setAttribute('aria-labelledby', tab.id);
  }
  if (data) renderBoard(board, data, track);
}

for (const tab of tabs) {
  tab.addEventListener('click', () => show(tab.dataset.track));
  tab.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    const next = tabs[(tabs.indexOf(tab) + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length];
    next.focus();
    show(next.dataset.track);
  });
}

try {
  const response = await fetch('leaderboard/board.json');
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  data = await response.json();
  document.getElementById('board-tag').textContent = `Catalog ${data.catalog} · ${data.basis.label}`;
  document.getElementById('board-basis-note').textContent = basisNote(data.basis);
  document.getElementById('stat-entries').textContent = String(data.tracks.standard.rows.length + (data.tracks.standard.pending?.length ?? 0));
  show('standard');
} catch {
  const note = document.createElement('p');
  note.className = 'board-note';
  note.textContent = 'The board could not be loaded. The same numbers are in the published v1.1 results, linked below.';
  board.replaceChildren(note);
}
