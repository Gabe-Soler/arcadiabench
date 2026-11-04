// Map layout without the DOM: spreading robots that share a spot, and placing
// their labels so that none covers another robot or label. Pure functions, so
// crowded scenes (20-robot crews) can be tested in Node.

/**
 * Push apart points closer than `minDist` (in their own units), in place.
 * A few relaxation passes settle clusters such as a crew parked in a row.
 */
export function spread(points, minDist, passes = 60) {
  for (let pass = 0; pass < passes; pass++) {
    let moved = false;
    for (let i = 0; i < points.length; i++) {
      for (let j = 0; j < i; j++) {
        const a = points[i];
        const b = points[j];
        const d = Math.hypot(a.x - b.x, a.y - b.y);
        if (d >= minDist - 1e-6) continue;
        const ang = d > 1e-6 ? Math.atan2(a.y - b.y, a.x - b.x) : (i * Math.PI * 2) / points.length + j;
        const push = (minDist - d) / 2;
        a.x += Math.cos(ang) * push;
        a.y += Math.sin(ang) * push;
        b.x -= Math.cos(ang) * push;
        b.y -= Math.sin(ang) * push;
        moved = true;
      }
    }
    if (!moved) break;
  }
  return points;
}

const overlaps = (a, b) => a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0;

/**
 * Place labels for robots drawn at screen points. Each item is
 * `{sx, sy, w, priority}`: its centre, its label's width and whether it must
 * be labelled (selected or hovered). `R` is the marker radius; `below` is the
 * height of the bars under a marker.
 *
 * Positions are tried in order (above, below the bars, right, left, then the
 * diagonals) and the first that covers no robot body and no placed label
 * wins. Priority items choose first and always get a label: when no spot is
 * clear, the one covering the least. Others that find no room get no label
 * (null), which keeps a crowded map readable. Returns, per item, null or
 * `{x, y, align, base, box}`.
 */
export function placeLabels(items, { R, below = 13, height = 13 }) {
  const bodies = items.map((p) => ({ x0: p.sx - R - 2, x1: p.sx + R + 2, y0: p.sy - R - 6, y1: p.sy + R + below }));
  const taken = [];
  const result = new Array(items.length).fill(null);
  const order = items.map((_, i) => i).sort((a, b) => (items[b].priority ? 1 : 0) - (items[a].priority ? 1 : 0) || items[a].sy - items[b].sy);
  for (const i of order) {
    const { sx, sy, w } = items[i];
    const h = height;
    const options = [
      { x: sx, y: sy - R - 3, align: 'center', base: 'bottom', box: { x0: sx - w / 2, x1: sx + w / 2, y0: sy - R - 3 - h, y1: sy - R - 3 } },
      { x: sx, y: sy + R + below + h, align: 'center', base: 'bottom', box: { x0: sx - w / 2, x1: sx + w / 2, y0: sy + R + below, y1: sy + R + below + h } },
      { x: sx + R + 7, y: sy, align: 'left', base: 'middle', box: { x0: sx + R + 5, x1: sx + R + 7 + w, y0: sy - h / 2, y1: sy + h / 2 } },
      { x: sx - R - 7, y: sy, align: 'right', base: 'middle', box: { x0: sx - R - 7 - w, x1: sx - R - 5, y0: sy - h / 2, y1: sy + h / 2 } },
      { x: sx + R, y: sy - R, align: 'left', base: 'bottom', box: { x0: sx + R, x1: sx + R + w, y0: sy - R - h, y1: sy - R } },
      { x: sx - R, y: sy - R, align: 'right', base: 'bottom', box: { x0: sx - R - w, x1: sx - R, y0: sy - R - h, y1: sy - R } },
      { x: sx + R, y: sy + R + h, align: 'left', base: 'bottom', box: { x0: sx + R, x1: sx + R + w, y0: sy + R, y1: sy + R + h } },
      { x: sx - R, y: sy + R + h, align: 'right', base: 'bottom', box: { x0: sx - R - w, x1: sx - R, y0: sy + R, y1: sy + R + h } },
    ];
    const covered = (o) => {
      let area = 0;
      const add = (b) => {
        const w = Math.min(o.box.x1, b.x1) - Math.max(o.box.x0, b.x0);
        const h = Math.min(o.box.y1, b.y1) - Math.max(o.box.y0, b.y0);
        if (w > 0 && h > 0) area += w * h;
      };
      taken.forEach(add);
      bodies.forEach((b, j) => j !== i && add(b));
      return area;
    };
    let pick = options.find((o) => covered(o) === 0) ?? null;
    if (!pick && items[i].priority) pick = options.reduce((best, o) => (covered(o) < covered(best) ? o : best));
    if (pick) {
      taken.push(pick.box);
      result[i] = pick;
    }
  }
  return result;
}
