/**
 * Pixel sampling for the overlay's magnifier and colour picker.
 *
 * Pure, exactly like `./selection` and `./snap`: no DOM, no React, no Tauri.
 * The buffer arrives as a plain `Uint8ClampedArray` rather than an
 * `ImageData`, so every rule about reading a pixel out of it is testable
 * without a canvas.
 *
 * Two coordinate spaces meet here, and they are not the same one.
 * `samplePixel` reads the frozen frame, which is in **device pixels**: the
 * frame is an unresampled, lossless copy of the display, so one entry in the
 * buffer is one physical pixel of the screen. The overlay's pointer events
 * arrive in **display-local CSS points**. The caller multiplies by the ratio
 * between the two before asking, which is the only reason the colour under the
 * cursor is the colour of the pixel the user is pointing at rather than an
 * average of the ones near it. That ratio is `frame.width / bounds.width`,
 * measured from the frame that was actually decoded rather than taken from the
 * display's advertised scale factor: the two agree today, but only the first
 * one is exact by construction, and a disagreement would show up as nothing but
 * a wrong colour.
 *
 * `magnifierSourceRect` takes no side in that: it is plain geometry and works
 * in whichever space its `point` and `bounds` are already in.
 */

import type { Point, Rect } from './selection'

export type Rgba = { r: number; g: number; b: number; a: number }

/** Reads one RGBA pixel from a canvas buffer, or null when out of range. */
export function samplePixel(
  data: Uint8ClampedArray,
  width: number,
  point: Point,
): Rgba | null {
  const x = Math.floor(point.x)
  const y = Math.floor(point.y)
  // `x >= width` on its own, because a row is `width` pixels wide and an x past
  // the right edge otherwise reads the first pixel of the next row: a colour
  // from somewhere else on the screen, which is worse than no colour at all.
  // The buffer has no height to check against, so the bottom edge is caught by
  // the length test below.
  if (x < 0 || y < 0 || x >= width) return null
  const offset = (y * width + x) * 4
  if (offset < 0 || offset + 3 >= data.length) return null
  return { r: data[offset]!, g: data[offset + 1]!, b: data[offset + 2]!, a: data[offset + 3]! }
}

/**
 * Formats a colour the way a user pastes it: `#RRGGBB`, uppercase.
 *
 * Alpha is dropped on purpose. The frozen frame is a screenshot of a composited
 * display, so every pixel in it is opaque, and a fourth channel would only ever
 * read `FF` while making the string harder to paste anywhere that expects a
 * six-digit hex colour.
 *
 * Colour space: these digits are the display's **native** values, not sRGB. No
 * conversion happens anywhere in the chain, which is the point. The frozen PNG
 * is written untagged, the canvas it is decoded into is a default sRGB one, and
 * an untagged source passes through unconverted, so the bytes that come back
 * out of `getImageData` are the bytes ScreenCaptureKit put in. On a P3 panel
 * that means the hex names a P3 colour while looking like any other hex, and
 * pasting it into a tool that assumes sRGB will land somewhere slightly
 * different. This matches what Digital Color Meter reports by default, so it is
 * the number a user comparing the two would expect, but it is a choice and not
 * a colour-managed answer.
 */
export function toHex(color: Rgba): string {
  const channel = (value: number) => value.toString(16).padStart(2, '0').toUpperCase()
  return `#${channel(color.r)}${channel(color.g)}${channel(color.b)}`
}

/**
 * Square source region for the magnifier, kept fully inside `bounds`.
 *
 * Clamped rather than cropped, so the magnifier is always the same size and
 * never shrinks into a sliver in a corner. The cost is that near an edge the
 * pointer is no longer at the centre of the region, which is why the overlay
 * marks the sampled pixel where it actually falls instead of assuming the
 * middle of the view.
 */
export function magnifierSourceRect(point: Point, size: number, bounds: Rect): Rect {
  const half = size / 2
  const x = Math.min(Math.max(point.x - half, bounds.x), bounds.x + bounds.width - size)
  const y = Math.min(Math.max(point.y - half, bounds.y), bounds.y + bounds.height - size)
  return { x, y, width: size, height: size }
}
