// Draws the menu bar glyph and writes it as a macOS template image.
//
// A template image is pure black artwork whose alpha channel carries the shape;
// macOS discards the colour and recolours the coverage for the current menu bar,
// which is the only way one asset can read on both a light and a dark strip. So
// every pixel here is rgb(0,0,0) and only the alpha varies.
//
// The glyph is a dashed selection marquee with a solid captured frame lifted out
// of its lower right: the two halves of what Snapdeck does, the region you draw
// and the image you get. The obvious drawing for a screenshot app is instead the
// four corner brackets of a viewfinder, and that is exactly why this is not
// that: the bracket glyph is what almost every capture utility on macOS already
// puts in the menu bar, so it identifies the category and not the application.
// The dash pattern also does work no silhouette could: nothing else on a normal
// menu bar is drawn with a broken stroke, while rounded rectangles are
// everywhere (Screen Mirroring's display, Control Center's pill), so the dashes
// are what let this be picked out of a crowded strip at a glance.
//
// Run with `node generate-tray-icon.mjs` from this directory. No dependencies:
// the PNG is encoded here, because pulling an image library into the build for
// two small assets would cost more than the fifty lines it saves.

import { deflateSync } from 'node:zlib';
import { writeFileSync } from 'node:fs';

/** The design grid, in points. macOS renders the tray image 18 points tall. */
const GRID = 18;
/** Subsamples per axis, per pixel. Cheap, and the edges need to be smooth. */
const SAMPLES = 8;

/** Half the marquee stroke, in points. The full stroke is 1.7pt. */
const HALF_STROKE = 0.85;

/**
 * The selection marquee: left, top, right, bottom, corner radius.
 *
 * The two shapes together span 1.4 to 16.6 across and 1.85 to 16.15 down, so
 * the drawing is centred on the 18pt grid and keeps the ~1.5pt of padding the
 * menu bar's own glyphs leave around themselves.
 */
const MARQUEE = { x0: 1.4, y0: 1.85, x1: 13.2, y1: 12.45, radius: 1.8 };

/** The captured frame lifted out of the marquee. */
const FRAME = { x0: 8.6, y0: 8.45, x1: 16.6, y1: 16.15, radius: 1.6 };

/**
 * Clear space cut around the captured frame, in points.
 *
 * Without it the marquee's dashes run into the frame's edge and the two shapes
 * fuse into one blob at menu bar size. A gap the width of the stroke keeps them
 * legible as two objects, which is the whole idea of the drawing.
 */
const FRAME_GAP = 1.0;

/** Roughly how long one dash plus its gap should be, in points. */
const DASH_PERIOD = 4.2;
/** Share of that period the ink covers, round caps included. */
const DASH_DUTY = 0.62;

/** Points sampled along each 90 degree corner arc when tracing the outline. */
const ARC_STEPS = 8;

/**
 * Traces a rounded rectangle as a closed polyline.
 *
 * Walking the outline as points, rather than solving it analytically, is what
 * makes evenly spaced dashes cheap: arc length along a polyline is a running
 * sum, and the dashes fall out of it directly.
 *
 * The walk starts at the middle of the top edge and runs clockwise.
 */
function traceRoundedRect({ x0, y0, x1, y1, radius }) {
  const midX = (x0 + x1) / 2;
  const points = [[midX, y0]];
  // Corner centres in clockwise order from the top right, each with the angle
  // its arc starts at. Angles grow clockwise because y grows downwards.
  const corners = [
    [x1 - radius, y0 + radius, -Math.PI / 2],
    [x1 - radius, y1 - radius, 0],
    [x0 + radius, y1 - radius, Math.PI / 2],
    [x0 + radius, y0 + radius, Math.PI],
  ];
  for (const [cx, cy, start] of corners) {
    for (let step = 0; step <= ARC_STEPS; step += 1) {
      const angle = start + (Math.PI / 2) * (step / ARC_STEPS);
      points.push([cx + radius * Math.cos(angle), cy + radius * Math.sin(angle)]);
    }
  }
  points.push([midX, y0]);
  return points;
}

/**
 * Cuts a traced outline into the segments a dashed stroke actually inks.
 *
 * The period is rounded to a whole number of dashes so the pattern closes on
 * itself: an outline that ended mid-dash would put a seam at the top of the
 * marquee, and at this size one wrong dash is a visible defect.
 *
 * Each dash is shortened by the stroke's half width at both ends, because the
 * segments are drawn with round caps that reach that far past their endpoints.
 * Skipping this is what turns a dashed rectangle back into a solid one: at this
 * size the caps are wider than the gaps and close every one of them.
 */
function dashOutline(points) {
  const spans = [];
  let perimeter = 0;
  for (let i = 1; i < points.length; i += 1) {
    const [ax, ay] = points[i - 1];
    const [bx, by] = points[i];
    const length = Math.hypot(bx - ax, by - ay);
    if (length === 0) continue;
    spans.push({ ax, ay, bx, by, length, start: perimeter });
    perimeter += length;
  }

  /** The point at `distance` along the outline. */
  const pointAt = (distance) => {
    const clamped = Math.min(Math.max(distance, 0), perimeter);
    const span = spans.findLast((candidate) => candidate.start <= clamped) ?? spans[0];
    const t = (clamped - span.start) / span.length;
    return [span.ax + (span.bx - span.ax) * t, span.ay + (span.by - span.ay) * t];
  };

  const count = Math.max(1, Math.round(perimeter / DASH_PERIOD));
  const period = perimeter / count;
  const segments = [];
  for (let i = 0; i < count; i += 1) {
    let from = i * period + HALF_STROKE;
    let to = i * period + period * DASH_DUTY - HALF_STROKE;
    if (to <= from) {
      // Too short to be a line once the caps are paid for: draw it as the dot
      // the round caps make of it anyway.
      from = to = (i * period + i * period + period * DASH_DUTY) / 2;
    }
    // A dash can run over one or more corners, so follow the outline's own
    // vertices between its ends rather than cutting the corner with a chord.
    const cuts = [from];
    for (const span of spans) {
      if (span.start > from && span.start < to) cuts.push(span.start);
    }
    cuts.push(to);
    for (let cut = 1; cut < cuts.length; cut += 1) {
      const [ax, ay] = pointAt(cuts[cut - 1]);
      const [bx, by] = pointAt(cuts[cut]);
      segments.push([ax, ay, bx, by]);
    }
  }
  return segments;
}

/** Distance from a point to a segment, which gives the stroke its round caps. */
function distanceToSegment(px, py, [ax, ay, bx, by]) {
  const dx = bx - ax;
  const dy = by - ay;
  const lengthSquared = dx * dx + dy * dy;
  const t =
    lengthSquared === 0
      ? 0
      : Math.min(1, Math.max(0, ((px - ax) * dx + (py - ay) * dy) / lengthSquared));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

/** Signed distance to a rounded rectangle: negative inside, zero on the edge. */
function distanceToRoundedRect(px, py, { x0, y0, x1, y1, radius }, grow = 0) {
  const r = radius + grow;
  const halfWidth = (x1 - x0) / 2 + grow - r;
  const halfHeight = (y1 - y0) / 2 + grow - r;
  const dx = Math.abs(px - (x0 + x1) / 2) - halfWidth;
  const dy = Math.abs(py - (y0 + y1) / 2) - halfHeight;
  return (
    Math.hypot(Math.max(dx, 0), Math.max(dy, 0)) + Math.min(Math.max(dx, dy), 0) - r
  );
}

const DASHES = dashOutline(traceRoundedRect(MARQUEE));

/** Whether one sample point falls on ink. */
function isInked(px, py) {
  if (distanceToRoundedRect(px, py, FRAME) <= 0) return true;
  // The gap wins over the marquee, so the frame reads as sitting in front.
  if (distanceToRoundedRect(px, py, FRAME, FRAME_GAP) <= 0) return false;
  return DASHES.some((segment) => distanceToSegment(px, py, segment) <= HALF_STROKE);
}

/** Renders the glyph at `size` pixels square as straight RGBA bytes. */
function render(size) {
  const scale = GRID / size;
  const pixels = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      let covered = 0;
      for (let sy = 0; sy < SAMPLES; sy += 1) {
        for (let sx = 0; sx < SAMPLES; sx += 1) {
          const px = (x + (sx + 0.5) / SAMPLES) * scale;
          const py = (y + (sy + 0.5) / SAMPLES) * scale;
          if (isInked(px, py)) covered += 1;
        }
      }
      // Black artwork; the alpha channel is the whole image as far as macOS
      // is concerned.
      pixels[(y * size + x) * 4 + 3] = Math.round((covered / (SAMPLES * SAMPLES)) * 255);
    }
  }
  return pixels;
}

function chunk(type, body) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(body.length);
  const typed = Buffer.concat([Buffer.from(type, 'latin1'), body]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(typed));
  return Buffer.concat([length, typed, crc]);
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/** Encodes straight RGBA bytes as an 8-bit non-interlaced PNG. */
function encodePng(size, pixels) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8; // Bit depth.
  header[9] = 6; // Colour type: RGBA.
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y += 1) {
    raw[y * (size * 4 + 1)] = 0; // Filter: none.
    pixels.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

for (const [name, size] of [
  ['tray-icon.png', GRID],
  ['tray-icon@2x.png', GRID * 2],
]) {
  writeFileSync(new URL(name, import.meta.url), encodePng(size, render(size)));
  console.log(`wrote ${name} (${size}x${size})`);
}
