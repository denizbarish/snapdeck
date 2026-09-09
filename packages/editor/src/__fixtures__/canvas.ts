/**
 * Test-only canvas fixtures and pixel measurements.
 *
 * Shared by `render.test.ts` and `export.test.ts` so both measure the same
 * quantities the same way. Nothing here is exported from the package; it exists
 * only so the two test files can assert over pixels instead of over snapshots
 * nobody reads.
 *
 * Every measurement returns a number. That is deliberate: "the region is
 * blurred" is an opinion, "the mean neighbour difference fell from 62.6 to 8.1"
 * is a fact a test can fail on.
 */

import type { Rect } from '../model'

/** The 2D context of a canvas, or a thrown error rather than a null deref. */
export function context2d(canvas: OffscreenCanvas): OffscreenCanvasRenderingContext2D {
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('no 2d context')
  return ctx
}

export function blankCanvas(width: number, height: number): OffscreenCanvas {
  return new OffscreenCanvas(width, height)
}

/**
 * A deterministic, high-frequency test image.
 *
 * Noise rather than a gradient or a photo: smoothing and quantisation are the
 * things under test, and noise is the signal that shows them most sharply. Two
 * neighbouring pixels differ a lot, so a blur that does nothing is obvious, and
 * every pixel is its own colour, so a pixelate that does nothing is obvious too.
 *
 * Channels are kept inside 16..239, which leaves pure black and pure white
 * unused by the source. A blackout test can then assert that no source value
 * survived without having to allow for a pixel that happened to be black already.
 */
export function noiseImage(width: number, height: number, seed = 0x9e3779b9): OffscreenCanvas {
  const canvas = new OffscreenCanvas(width, height)
  const ctx = context2d(canvas)
  const pixels = ctx.createImageData(width, height)
  let state = seed >>> 0
  const next = (): number => {
    state ^= state << 13
    state >>>= 0
    state ^= state >>> 17
    state ^= state << 5
    state >>>= 0
    return state
  }
  for (let index = 0; index < pixels.data.length; index += 4) {
    pixels.data[index] = 16 + (next() % 224)
    pixels.data[index + 1] = 16 + (next() % 224)
    pixels.data[index + 2] = 16 + (next() % 224)
    pixels.data[index + 3] = 255
  }
  ctx.putImageData(pixels, 0, 0)
  return canvas
}

export function readPixels(canvas: OffscreenCanvas): ImageData {
  return context2d(canvas).getImageData(0, 0, canvas.width, canvas.height)
}

/** One channel, with the index guard `noUncheckedIndexedAccess` asks for. */
function channel(pixels: ImageData, x: number, y: number, offset: number): number {
  return pixels.data[(y * pixels.width + x) * 4 + offset] ?? 0
}

export function pixelAt(pixels: ImageData, x: number, y: number): [number, number, number, number] {
  return [
    channel(pixels, x, y, 0),
    channel(pixels, x, y, 1),
    channel(pixels, x, y, 2),
    channel(pixels, x, y, 3),
  ]
}

/** How many of two images' bytes differ. Zero means pixel-identical. */
export function differingBytes(a: ImageData, b: ImageData): number {
  if (a.width !== b.width || a.height !== b.height) {
    throw new Error(`size mismatch: ${a.width}x${a.height} vs ${b.width}x${b.height}`)
  }
  let count = 0
  for (let index = 0; index < a.data.length; index += 1) {
    if (a.data[index] !== b.data[index]) count += 1
  }
  return count
}

/** How many distinct RGB colours a region holds. */
export function distinctColors(pixels: ImageData, rect: Rect): number {
  const seen = new Set<number>()
  for (let y = rect.y; y < rect.y + rect.height; y += 1) {
    for (let x = rect.x; x < rect.x + rect.width; x += 1) {
      const [r, g, b] = pixelAt(pixels, x, y)
      seen.add((r << 16) | (g << 8) | b)
    }
  }
  return seen.size
}

/**
 * Mean absolute difference between adjacent pixels in a region.
 *
 * The measure of how much high-frequency detail is left. Blurring drives it
 * down because neighbours are averaged together; pixelating drives it down
 * further still because neighbours inside a block become identical.
 */
export function neighbourDelta(pixels: ImageData, rect: Rect): number {
  let total = 0
  let samples = 0
  for (let y = rect.y; y < rect.y + rect.height - 1; y += 1) {
    for (let x = rect.x; x < rect.x + rect.width - 1; x += 1) {
      const here = pixelAt(pixels, x, y)
      const right = pixelAt(pixels, x + 1, y)
      const below = pixelAt(pixels, x, y + 1)
      for (let offset = 0; offset < 3; offset += 1) {
        total += Math.abs((here[offset] ?? 0) - (right[offset] ?? 0))
        total += Math.abs((here[offset] ?? 0) - (below[offset] ?? 0))
        samples += 2
      }
    }
  }
  return samples === 0 ? 0 : total / samples
}

/**
 * The share of pixels in a region whose RGB is byte-for-byte the source's.
 *
 * This is the privacy measurement. `shift` is the source coordinate of the
 * region's top-left corner, so a cropped export can still be compared against
 * the image it came from. A result of 0 means not one original pixel survived
 * the obscure; anything above chance means some did.
 */
export function identicalFraction(
  output: ImageData,
  source: ImageData,
  rect: Rect,
  shift: { x: number; y: number } = { x: 0, y: 0 },
): number {
  let same = 0
  let total = 0
  for (let y = rect.y; y < rect.y + rect.height; y += 1) {
    for (let x = rect.x; x < rect.x + rect.width; x += 1) {
      const [r, g, b] = pixelAt(output, x, y)
      const [sr, sg, sb] = pixelAt(source, x + shift.x, y + shift.y)
      if (r === sr && g === sg && b === sb) same += 1
      total += 1
    }
  }
  return total === 0 ? 0 : same / total
}
