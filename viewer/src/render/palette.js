// Colours shared by the map, the legend and the panels.

/** Surface-state colours, RGB. Chosen to stay apart under hill shading. */
export const SURFACE_RGB = {
  natural: [176, 108, 68],
  cleared: [120, 62, 42],
  graded: [218, 168, 118],
  compacted: [150, 140, 128],
  stabilized: [78, 90, 106],
  berm: [238, 178, 70],
  stockpile: [96, 70, 56],
};
export const OTHER_SURFACE_RGB = [128, 128, 128];

export const SURFACE_LABELS = {
  natural: 'Natural',
  cleared: 'Cleared',
  graded: 'Graded',
  compacted: 'Compacted',
  stabilized: 'Stabilized',
  berm: 'Berm',
  stockpile: 'Stockpile',
};

export const STATUS_COLORS = {
  idle: '#c3cedb',
  moving: '#38bdf8',
  working: '#4ade80',
  charging: '#facc15',
  stuck: '#fb923c',
  failed: '#f43f5e',
  being_towed: '#c084fc',
  offline: '#7b8696',
  lost: '#454d58',
};
export const UNKNOWN_STATUS_COLOR = '#e2e8f0';

export const STATUS_LABELS = {
  idle: 'Idle',
  moving: 'Moving',
  working: 'Working',
  charging: 'Charging',
  stuck: 'Stuck',
  failed: 'Failed',
  being_towed: 'Being towed',
  offline: 'Offline',
  lost: 'Lost',
};

export function statusColor(status) {
  return STATUS_COLORS[status] ?? UNKNOWN_STATUS_COLOR;
}

export function rgb(c, f = 1) {
  const k = (v) => Math.max(0, Math.min(255, Math.round(v * f)));
  return `rgb(${k(c[0])},${k(c[1])},${k(c[2])})`;
}

/** Battery fill colour by state of charge. */
export function batteryColor(frac) {
  if (frac == null) return '#5d6d82';
  if (frac > 0.5) return '#4ade80';
  if (frac > 0.2) return '#f5b942';
  return '#f25c5c';
}
