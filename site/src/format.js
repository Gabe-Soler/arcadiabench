// How the front page writes the board's numbers and names. Pure functions.

/** A headline or a resilience: three decimals, or a dash when there is none. */
export function fmt3(x) {
  return Number.isFinite(x) ? x.toFixed(3) : '–';
}

/** A rank as shown: `2`, or `2=` when the rank is shared. */
export function rankLabel(row) {
  return `${row.rank}${row.tied ? '=' : ''}`;
}

/** Runs finished of runs made. */
export function finished(row) {
  return `${row.completed} / ${row.runs}`;
}

/** A crew as its four counts, in the order scout, excavator, hauler,
 * compactor-sinterer. */
export function crewCounts(crew) {
  return [crew.scout, crew.excavator, crew.hauler, crew.compactor_sinterer].join(' · ');
}

/** A mass in tonnes, to one decimal. */
export function tonnes(kg) {
  return `${(kg / 1000).toFixed(1)} t`;
}

const VARIANT_NAMES = {
  nominal: 'Nominal',
  dust_storm: 'Dust storm',
  conjunction: 'Conjunction',
  cascading_faults: 'Cascading faults',
  solar_only: 'Solar only',
  nuclear_heavy: 'Nuclear heavy',
};

/** A variant's name in words. An unknown one is shown as it is written. */
export function variantName(v) {
  return VARIANT_NAMES[v] ?? String(v).replaceAll('_', ' ');
}

/** An outpost's name, capitalised; the fleet track's one row is the crew's. */
export function presetName(p) {
  return p ? p[0].toUpperCase() + p.slice(1) : 'Crew';
}

/** The width of a headline's bar against the leader's, as a percentage. */
export function barWidth(headline, best) {
  if (!(best > 0) || !Number.isFinite(headline)) return 0;
  return Math.max(0, Math.min(100, (headline / best) * 100));
}

/** The line under an entry's name. */
export function metaLine(row) {
  return [row.version, row.authors.join(', '), row.licence].filter(Boolean).join(' · ');
}

/** Only links to this site or to https are followed; anything else an entry
 * file might say is dropped. */
export function safeHref(href) {
  if (typeof href !== 'string') return null;
  if (/^https:\/\//.test(href)) return href;
  if (/^(leaderboard|viewer|results)\//.test(href) && !href.includes('//')) return href;
  return null;
}
