// Colours and the UI font for the canvases, read from the page's CSS custom
// properties (style.css), so the map, the timeline and the power chart follow
// the same light or dark scheme as the rest of the page. The system picks the
// scheme (prefers-color-scheme); watchTheme reports a change so the caller
// can redraw.

const VARS = {
  ink: '--gs-ink',
  grey: '--gs-grey',
  faint: '--gs-placeholder',
  card: '--gs-card',
  raised: '--gs-raised',
  font: '--gs-font-body',
  ok: '--ok',
  warn: '--warn',
  err: '--err',
  solar: '--solar',
  fission: '--fission',
  demand: '--demand',
  storage: '--storage',
  mapFrame: '--cv-map-frame',
  unknown: '--cv-unknown',
  night: '--cv-night',
  day: '--cv-day',
  tick: '--cv-tick',
  elapsed: '--cv-elapsed',
  hover: '--cv-hover',
  bandStorm: '--cv-band-storm',
  bandLink: '--cv-band-link',
  bandPass: '--cv-band-pass',
  failure: '--cv-failure',
  chartNight: '--cv-chart-night',
  chartStorm: '--cv-chart-storm',
  chartGrid: '--cv-chart-grid',
  storageFill: '--cv-storage-fill',
};

const SCHEME = '(prefers-color-scheme: dark)';

/** The current canvas colours; read again on every scheme change. */
export const theme = {};

export function readTheme() {
  const style = getComputedStyle(document.documentElement);
  for (const [key, name] of Object.entries(VARS)) theme[key] = style.getPropertyValue(name).trim();
  theme.font ||= 'system-ui, sans-serif';
  theme.dark = matchMedia(SCHEME).matches;
  return theme;
}

/** Call `onChange(theme)` whenever the system switches between light and dark. */
export function watchTheme(onChange) {
  matchMedia(SCHEME).addEventListener('change', () => onChange(readTheme()));
}

readTheme();
