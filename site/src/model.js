// The leaderboard's arithmetic: what an entry's results come to, and how
// entries are ranked. Pure functions, used by the tool that builds
// leaderboard/board.json (site/tools/build-board.mjs) and by its tests. The
// page itself only draws the board it is given.
//
// - The number that ranks an entry is its headline (success rate × speed) on
//   the nominal variant: on the Standard track averaged over the three
//   outposts, on the fleet design track for the one crew it brought.
// - Entries are ranked by that number. Two entries whose difference, taken
//   seed by seed over the same runs, cannot be told from zero share a rank.

export const PRESETS = ['small', 'medium', 'large'];
export const VARIANTS = ['nominal', 'dust_storm', 'conjunction', 'cascading_faults', 'solar_only', 'nuclear_heavy'];

/** How many standard errors a paired difference has to be from zero for two
 * entries to be told apart. */
export const TIE_Z = 2;

export function mean(xs) {
  return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : NaN;
}

/** The mean of `a[i] - b[i]` and its standard error. */
export function pairedGap(a, b) {
  const n = Math.min(a.length, b.length);
  if (n === 0 || a.length !== b.length) return { mean: NaN, se: NaN, n };
  const d = a.map((x, i) => x - b[i]);
  const m = mean(d);
  if (n === 1) return { mean: m, se: NaN, n };
  const variance = d.reduce((s, x) => s + (x - m) ** 2, 0) / (n - 1);
  return { mean: m, se: Math.sqrt(variance / n), n };
}

/** Whether two entries' scores on the same runs can be told apart. Scores
 * that cannot be paired (different runs, or too few) count as apart: a tie
 * has to be shown, not assumed. */
export function separable(a, b, z = TIE_Z) {
  const gap = pairedGap(a, b);
  if (!Number.isFinite(gap.mean) || !Number.isFinite(gap.se)) return true;
  if (gap.se === 0) return gap.mean !== 0;
  return Math.abs(gap.mean) > z * gap.se;
}

/** Rank rows in place: by headline, then resilience, then id. A row shares
 * the rank of the first row of the group above it when its paired scores
 * cannot be told apart from that row's. `scores` and `runs` are what the
 * comparison needs: one score per run, and which runs they are. */
export function rankRows(rows) {
  rows.sort((a, b) => (b.headline - a.headline) || ((b.resilience ?? 0) - (a.resilience ?? 0)) || (a.id < b.id ? -1 : 1));
  let leader = null;
  rows.forEach((row, i) => {
    const sameRuns = leader && leader.runsKey === row.runsKey;
    if (leader && sameRuns && !separable(leader.scores, row.scores)) {
      row.rank = leader.rank;
      row.tied = true;
      leader.tied = true;
    } else {
      row.rank = i + 1;
      row.tied = false;
      leader = row;
    }
  });
  return rows;
}

function scenarioId(catalog, who, variant) {
  const prefix = catalog === '1.0' ? '' : `${catalog}-`;
  return `landing-pad-${prefix}${who}-${variant}`;
}

function crewLabel(crew) {
  return `${crew.scout}.${crew.excavator}.${crew.hauler}.${crew.compactor_sinterer}`;
}

function cell(card) {
  return {
    variant: card.variant,
    headline: card.headline,
    success_rate: card.completion.success_rate,
    mean_sols: card.completion.mean_sols,
  };
}

function links(entry, basis, track) {
  const dir = `leaderboard/entries/${entry.id}/${entry.version}`;
  const replay = entry.replays?.[track];
  return {
    source: `${entry.source.repository}/tree/${entry.source.commit}`,
    scorecards: `${dir}/${basis}/report.json`,
    replay: replay ? `viewer/?log=../${replay}` : null,
  };
}

function common(entry) {
  return {
    id: entry.id,
    version: entry.version,
    name: entry.name,
    authors: entry.authors,
    licence: entry.licence,
    baseline: entry.baseline === true,
    summary: entry.summary ?? '',
  };
}

function paired(runs, ids) {
  const scores = [];
  const keys = [];
  for (const id of ids) {
    const s = runs?.scenarios?.[id];
    if (!s || s.seeds.length !== s.scores.length) return { scores: [], runsKey: null };
    scores.push(...s.scores);
    keys.push(`${id}:${s.seeds.join(',')}`);
  }
  return { scores, runsKey: keys.join('|') };
}

/** An entry's row on the Standard track, or null if its report does not
 * cover the track: all three outposts on all six variants. */
export function standardRow(entry, report, runs, basis = 'public') {
  const cards = report.scorecards.filter((c) => !c.crew && c.preset);
  if (cards.length === 0) return null;
  const catalog = cards[0].catalog ?? '1.0';
  const of = (preset, variant) => cards.find((c) => c.preset === preset && c.variant === variant);
  const grid = [];
  for (const preset of PRESETS) {
    const row = VARIANTS.map((v) => of(preset, v));
    if (row.some((c) => !c)) return null;
    grid.push({ preset, cells: row.map(cell) });
  }
  const nominal = PRESETS.map((p) => of(p, 'nominal').headline);
  const resilience = report.resilience.find((r) => !r.crew)?.resilience ?? null;
  return {
    ...common(entry),
    headline: mean(nominal),
    presets: Object.fromEntries(PRESETS.map((p, i) => [p, nominal[i]])),
    resilience,
    completed: cards.reduce((s, c) => s + c.completion.completed, 0),
    runs: cards.reduce((s, c) => s + c.completion.runs, 0),
    grid,
    links: links(entry, basis, 'standard'),
    ...paired(runs, PRESETS.map((p) => scenarioId(catalog, p, 'nominal'))),
  };
}

/** An entry's row on the fleet design track, or null if it did not enter. */
export function fleetRow(entry, report, runs, basis = 'public') {
  const cards = report.scorecards.filter((c) => c.crew);
  if (cards.length === 0 || !entry.fleet) return null;
  const catalog = cards[0].catalog ?? '1.0';
  const row = VARIANTS.map((v) => cards.find((c) => c.variant === v));
  if (row.some((c) => !c)) return null;
  const crew = cards[0].crew;
  return {
    ...common(entry),
    headline: row[0].headline,
    crew,
    mass_kg: entry.fleet.mass_kg,
    designed_by: entry.fleet.designed_by ?? entry.id,
    resilience: report.resilience.find((r) => r.crew)?.resilience ?? null,
    completed: cards.reduce((s, c) => s + c.completion.completed, 0),
    runs: cards.reduce((s, c) => s + c.completion.runs, 0),
    reference_sols: row[0].reference_sols,
    grid: [{ preset: null, cells: row.map(cell) }],
    links: links(entry, basis, 'fleet'),
    ...paired(runs, [scenarioId(catalog, `crew.${crewLabel(crew)}`, 'nominal')]),
  };
}

function finish(rows) {
  rankRows(rows);
  for (const row of rows) {
    delete row.scores;
    delete row.runsKey;
  }
  return rows;
}

/** The board: every entry's row on each track it entered, ranked.
 * `entries` is a list of `{ entry, report, runs }`. */
export function buildBoard(entries, basis = { kind: 'public', label: 'seeds 1–50' }) {
  const catalogs = new Set(entries.flatMap((e) => e.report.scorecards.map((c) => c.catalog ?? '1.0')));
  if (catalogs.size > 1) throw new Error(`entries on more than one catalog: ${[...catalogs].join(', ')}`);
  const standard = entries.map((e) => standardRow(e.entry, e.report, e.runs, basis.kind)).filter(Boolean);
  const fleet = entries.map((e) => fleetRow(e.entry, e.report, e.runs, basis.kind)).filter(Boolean);
  return {
    catalog: [...catalogs][0] ?? null,
    basis,
    tie_z: TIE_Z,
    tracks: {
      standard: { scenarios: PRESETS.length * VARIANTS.length, rows: finish(standard) },
      fleet: { scenarios: VARIANTS.length, rows: finish(fleet) },
    },
  };
}
