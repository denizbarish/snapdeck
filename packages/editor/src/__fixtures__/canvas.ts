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

/**
 * A deterministic, text-like test image: dark strokes on a light ground.
 *
 * This is the fixture the privacy floors are calibrated against, and noise
 * cannot stand in for it. Noise has no two equal neighbours, so any averaging
 * at all changes every byte and every smoothing measurement comes out perfect;
 * structure is what a redaction has to destroy and what a weak one leaves
 * legible.
 *
 * The strokes SCALE with the region, because that is what the content in a box
 * a user drew actually does: a box dragged tightly around a line of text holds
 * glyphs about as tall as the box, whatever the box's size. Stem width is 12%
 * of the height and the glyphs stand 72% of it, which is roughly a screenshot's
 * UI text. Stems are spaced irregularly so no block size can be commensurate
 * with the pattern and score well by luck.
 */
export function textImage(width: number, height: number, seed = 0x2545f491): OffscreenCanvas {
  const canvas = new OffscreenCanvas(width, height)
  const ctx = context2d(canvas)
  const pixels = ctx.createImageData(width, height)
  pixels.data.fill(255)
  let state = seed >>> 0
  const next = (): number => {
    state ^= state << 13
    state >>>= 0
    state ^= state >>> 17
    state ^= state << 5
    state >>>= 0
    return state
  }
  const ink = (x: number, y: number): void => {
    if (x < 0 || x >= width || y < 0 || y >= height) return
    const index = (y * width + x) * 4
    pixels.data[index] = 0
    pixels.data[index + 1] = 0
    pixels.data[index + 2] = 0
  }
  const stem = Math.max(1, Math.round(height * 0.12))
  const tall = Math.max(2, Math.round(height * 0.72))
  const top = Math.floor((height - tall) / 2)
  for (let x = stem; x + stem <= width; ) {
    for (let y = top; y < top + tall; y += 1) {
      for (let across = 0; across < stem; across += 1) ink(x + across, y)
    }
    // Half the stems get a crossbar, so the fixture has structure on both axes
    // and a blur along one of them cannot flatter itself.
    if (next() % 2 === 0) {
      const bar = top + Math.floor(tall / 2)
      for (let across = 0; across < stem * 3; across += 1) {
        for (let down = 0; down < stem; down += 1) ink(x + across, bar + down)
      }
    }
    x += stem + Math.max(1, Math.round(stem * (1 + (next() % 3) * 0.5)))
  }
  ctx.putImageData(pixels, 0, 0)
  return canvas
}

/**
 * One channel.
 *
 * Out-of-range coordinates throw rather than wrapping. Without the guard,
 * reading one past the right edge silently returns the first pixel of the next
 * row, so a test that walks off a rect measures the wrong pixels and passes.
 */
function channel(pixels: ImageData, x: number, y: number, offset: number): number {
  if (x < 0 || y < 0 || x >= pixels.width || y >= pixels.height) {
    throw new Error(`pixel ${x},${y} is outside ${pixels.width}x${pixels.height}`)
  }
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

/** Rec. 601 luma, which is what "how dark is this stroke" means to an eye. */
function luminance(pixels: ImageData, x: number, y: number): number {
  const [r, g, b] = pixelAt(pixels, x, y)
  return 0.299 * r + 0.587 * g + 0.114 * b
}

/**
 * Peak-to-trough luminance across a region: how much stroke contrast is left.
 *
 * 255 on the text fixture means the strokes are still at full strength against
 * their ground, which is what legibility is made of. This is the legibility
 * measurement; `sourceCorrelation` below is the recoverability one. Neither is
 * a byte count, so neither can be satisfied by a fixture with no two equal
 * neighbours.
 */
export function strokeContrast(pixels: ImageData, rect: Rect): number {
  let lowest = 255
  let highest = 0
  for (let y = rect.y; y < rect.y + rect.height; y += 1) {
    for (let x = rect.x; x < rect.x + rect.width; x += 1) {
      const luma = luminance(pixels, x, y)
      if (luma < lowest) lowest = luma
      if (luma > highest) highest = luma
    }
  }
  return highest - lowest
}

/**
 * Pearson correlation between a region's luminance and the source's.
 *
 * The metric that survives a change of fixture. 1 means the output still varies
 * exactly as the original did, pixel for pixel, whether or not a single byte
 * matches; 0 means what is left carries none of the original's structure. A
 * mosaic and a blur both bottom out near 0.4 on text, because both preserve
 * local ink density: you can still see that there was writing, which is the
 * honest limit of any mode that averages rather than destroys.
 *
 * `shift` is the source coordinate of the region's top-left corner, so a
 * cropped export can still be compared against the image it came from.
 */
export function sourceCorrelation(
  output: ImageData,
  source: ImageData,
  rect: Rect,
  shift: { x: number; y: number } = { x: 0, y: 0 },
): number {
  const outputs: number[] = []
  const sources: number[] = []
  for (let y = rect.y; y < rect.y + rect.height; y += 1) {
    for (let x = rect.x; x < rect.x + rect.width; x += 1) {
      outputs.push(luminance(output, x, y))
      sources.push(luminance(source, x + shift.x, y + shift.y))
    }
  }
  const count = outputs.length
  if (count === 0) return 0
  const meanOutput = outputs.reduce((total, value) => total + value, 0) / count
  const meanSource = sources.reduce((total, value) => total + value, 0) / count
  let covariance = 0
  let outputVariance = 0
  let sourceVariance = 0
  for (let index = 0; index < count; index += 1) {
    const a = (outputs[index] ?? 0) - meanOutput
    const b = (sources[index] ?? 0) - meanSource
    covariance += a * b
    outputVariance += a * a
    sourceVariance += b * b
  }
  // A region the obscure flattened has no variance left to correlate, which is
  // the strongest possible result, not an undefined one.
  if (outputVariance === 0 || sourceVariance === 0) return 0
  return covariance / Math.sqrt(outputVariance * sourceVariance)
}

/**
 * The share of pixels in a region whose RGB is byte-for-byte the source's.
 *
 * A mutation detector, not a privacy metric, and it is kept for what it is good
 * at: a mosaic of one-pixel blocks scores 1 here and nothing else catches that
 * so cheaply. It cannot carry the privacy claim on its own, because it is
 * trivially 0 for any smoothing over noise and trivially 1 for a blur over a
 * flat region; `strokeContrast` and `sourceCorrelation` carry that.
 *
 * `shift` is the source coordinate of the region's top-left corner, so a
 * cropped export can still be compared against the image it came from.
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
