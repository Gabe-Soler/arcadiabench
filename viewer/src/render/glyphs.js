// Small capability glyphs drawn inside robot markers, in a box of half-size s.

function bolt(ctx, cx, cy, s) {
  ctx.beginPath();
  ctx.moveTo(cx + 0.15 * s, cy - 0.95 * s);
  ctx.lineTo(cx - 0.5 * s, cy + 0.12 * s);
  ctx.lineTo(cx - 0.02 * s, cy + 0.12 * s);
  ctx.lineTo(cx - 0.15 * s, cy + 0.95 * s);
  ctx.lineTo(cx + 0.5 * s, cy - 0.15 * s);
  ctx.lineTo(cx + 0.02 * s, cy - 0.15 * s);
  ctx.closePath();
  ctx.fill();
}

const GLYPHS = {
  scan(ctx, cx, cy, s) {
    ctx.beginPath();
    ctx.arc(cx - 0.45 * s, cy + 0.45 * s, 0.18 * s, 0, Math.PI * 2);
    ctx.fill();
    for (const r of [0.6, 1.05]) {
      ctx.beginPath();
      ctx.arc(cx - 0.45 * s, cy + 0.45 * s, r * s, -Math.PI / 2 + 0.1, -0.1);
      ctx.stroke();
    }
  },
  dig(ctx, cx, cy, s) {
    ctx.beginPath();
    ctx.moveTo(cx - 0.8 * s, cy - 0.45 * s);
    ctx.lineTo(cx + 0.8 * s, cy - 0.45 * s);
    ctx.lineTo(cx + 0.45 * s, cy + 0.35 * s);
    ctx.lineTo(cx - 0.45 * s, cy + 0.35 * s);
    ctx.closePath();
    ctx.fill();
    for (const dx of [-0.35, 0, 0.35]) {
      ctx.beginPath();
      ctx.moveTo(cx + (dx - 0.1) * s, cy + 0.35 * s);
      ctx.lineTo(cx + dx * s, cy + 0.75 * s);
      ctx.lineTo(cx + (dx + 0.1) * s, cy + 0.35 * s);
      ctx.fill();
    }
  },
  grade(ctx, cx, cy, s) {
    ctx.fillRect(cx - 0.85 * s, cy - 0.15 * s, 1.7 * s, 0.38 * s);
    ctx.beginPath();
    ctx.moveTo(cx - 0.85 * s, cy + 0.6 * s);
    ctx.lineTo(cx + 0.85 * s, cy + 0.6 * s);
    ctx.stroke();
    ctx.fillRect(cx - 0.12 * s, cy - 0.75 * s, 0.24 * s, 0.6 * s);
  },
  haul(ctx, cx, cy, s) {
    ctx.beginPath();
    ctx.moveTo(cx - 0.85 * s, cy - 0.4 * s);
    ctx.lineTo(cx + 0.85 * s, cy - 0.4 * s);
    ctx.lineTo(cx + 0.6 * s, cy + 0.3 * s);
    ctx.lineTo(cx - 0.6 * s, cy + 0.3 * s);
    ctx.closePath();
    ctx.fill();
    for (const dx of [-0.42, 0.42]) {
      ctx.beginPath();
      ctx.arc(cx + dx * s, cy + 0.58 * s, 0.2 * s, 0, Math.PI * 2);
      ctx.fill();
    }
  },
  compact(ctx, cx, cy, s) {
    ctx.beginPath();
    ctx.arc(cx, cy + 0.15 * s, 0.6 * s, 0, Math.PI * 2);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(cx, cy + 0.15 * s, 0.16 * s, 0, Math.PI * 2);
    ctx.fill();
    ctx.beginPath();
    ctx.moveTo(cx, cy + 0.15 * s);
    ctx.lineTo(cx + 0.55 * s, cy - 0.8 * s);
    ctx.stroke();
  },
  sinter(ctx, cx, cy, s) {
    ctx.beginPath();
    ctx.arc(cx, cy, 0.32 * s, 0, Math.PI * 2);
    ctx.fill();
    for (let i = 0; i < 8; i++) {
      const a = (i * Math.PI) / 4;
      ctx.beginPath();
      ctx.moveTo(cx + Math.cos(a) * 0.5 * s, cy + Math.sin(a) * 0.5 * s);
      ctx.lineTo(cx + Math.cos(a) * 0.92 * s, cy + Math.sin(a) * 0.92 * s);
      ctx.stroke();
    }
  },
  repair(ctx, cx, cy, s) {
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(-Math.PI / 4);
    ctx.fillRect(-0.14 * s, -0.2 * s, 0.28 * s, 1.05 * s);
    ctx.beginPath();
    ctx.arc(0, -0.45 * s, 0.4 * s, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalCompositeOperation = 'destination-out';
    ctx.fillRect(-0.13 * s, -0.95 * s, 0.26 * s, 0.45 * s);
    ctx.restore();
  },
  charge(ctx, cx, cy, s) {
    bolt(ctx, cx, cy, s);
  },
  relay(ctx, cx, cy, s) {
    ctx.beginPath();
    ctx.moveTo(cx, cy - 0.2 * s);
    ctx.lineTo(cx - 0.4 * s, cy + 0.85 * s);
    ctx.moveTo(cx, cy - 0.2 * s);
    ctx.lineTo(cx + 0.4 * s, cy + 0.85 * s);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(cx, cy - 0.25 * s, 0.17 * s, 0, Math.PI * 2);
    ctx.fill();
    for (const r of [0.48, 0.82]) {
      ctx.beginPath();
      ctx.arc(cx, cy - 0.25 * s, r * s, -Math.PI * 0.85, -Math.PI * 0.15);
      ctx.stroke();
    }
  },
};

/**
 * Draw the glyph for capability `name` centred at (cx, cy). The caller's
 * fillStyle is used; strokes use the same colour.
 */
export function drawGlyph(ctx, name, cx, cy, s) {
  const g = GLYPHS[name];
  ctx.save();
  ctx.strokeStyle = ctx.fillStyle;
  ctx.lineWidth = Math.max(1, s * 0.2);
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  if (g) {
    g(ctx, cx, cy, s);
  } else {
    ctx.beginPath();
    ctx.arc(cx, cy, 0.3 * s, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}

/** A charge-station marker: a square with a bolt. */
export function drawStation(ctx, x, y, size) {
  const h = size / 2;
  ctx.fillStyle = '#2a2610';
  ctx.strokeStyle = '#facc15';
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.roundRect(x - h, y - h, size, size, size * 0.18);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = '#facc15';
  bolt(ctx, x, y, h * 0.72);
}

/** A depot marker: a square with a wrench. */
export function drawDepot(ctx, x, y, size) {
  const h = size / 2;
  ctx.fillStyle = '#18222e';
  ctx.strokeStyle = '#9fb6cf';
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.roundRect(x - h, y - h, size, size, size * 0.18);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = '#9fb6cf';
  drawGlyph(ctx, 'repair', x, y, h * 0.7);
}

/** The base-station marker: a mast with waves. */
export function drawBaseStation(ctx, x, y, size) {
  const h = size / 2;
  ctx.fillStyle = '#0f2236';
  ctx.strokeStyle = '#56b8ff';
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.arc(x, y, h, 0, Math.PI * 2);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = '#56b8ff';
  drawGlyph(ctx, 'relay', x, y + h * 0.05, h * 0.68);
}
