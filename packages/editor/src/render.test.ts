/**
 * Renderer tests, run in a real browser engine against a real canvas.
 *
 * Every assertion is a measurement over pixels the renderer actually produced:
 * how many bytes differ from the source, how many distinct colours a region
 * holds, how far neighbouring pixels sit apart, how many original pixels
 * survived. No snapshots, because a snapshot of a 64x48 image is a blob nobody
 * ever looks at again and it passes just as happily when the drawing is wrong.
 */

import { describe, expect, it } from 'vitest'
import type { EditorDocument, Layer, Rect } from './model'
import { renderDocument } from './render'
import {
  blankCanvas,
  context2d,
  differingBytes,
  distinctColors,
  identicalFraction,
  neighbourDelta,
  noiseImage,
  pixelAt,
  readPixels,
} from './__fixtures__/canvas'

/** Render a document into a fresh canvas of the given size and read it back. */
function render(
  image: OffscreenCanvas,
  doc: EditorDocument,
  size: { width: number; height: number },
  transform?: (ctx: OffscreenCanvasRenderingContext2D) => void,
): ImageData {
  const canvas = blankCanvas(size.width, size.height)
  const ctx = context2d(canvas)
  transform?.(ctx)
  renderDocument(ctx, image, doc)
  return readPixels(canvas)
}

function documentOf(width: number, height: number, layers: Layer[], crop: Rect | null = null): EditorDocument {
  return { width, height, crop, layers }
}

function filled(id: string, rect: Rect, color: string): Layer {
  return { id, kind: 'rect', rect, style: { stroke: { color, width: 1 }, fill: color } }
}

function countColor(pixels: ImageData, rgb: [number, number, number]): number {
  let count = 0
  for (let y = 0; y < pixels.height; y += 1) {
    for (let x = 0; x < pixels.width; x += 1) {
      const [r, g, b] = pixelAt(pixels, x, y)
      if (r === rgb[0] && g === rgb[1] && b === rgb[2]) count += 1
    }
  }
  return count
}

describe('renderDocument', () => {
  it('reproduces the source image exactly when there is nothing to draw over it', () => {
    const image = noiseImage(64, 48)
    const doc = documentOf(64, 48, [])

    const output = render(image, doc, { width: 64, height: 48 })

    expect(differingBytes(output, readPixels(image))).toBe(0)
  })

  it('draws layers in array order, last on top', () => {
    const image = noiseImage(64, 48)
    const doc = documentOf(64, 48, [
      filled('under', { x: 10, y: 10, width: 20, height: 20 }, '#ff0000'),
      filled('over', { x: 20, y: 20, width: 20, height: 20 }, '#0000ff'),
    ])

    const output = render(image, doc, { width: 64, height: 48 })

    expect(pixelAt(output, 15, 15)).toEqual([255, 0, 0, 255])
    expect(pixelAt(output, 35, 35)).toEqual([0, 0, 255, 255])
    // The overlap is the only pixel that can tell the two orders apart.
    expect(pixelAt(output, 25, 25)).toEqual([0, 0, 255, 255])
  })

  it('shifts layers by the crop origin and leaves everything outside the crop off the canvas', () => {
    const image = noiseImage(64, 48)
    const crop = { x: 16, y: 12, width: 32, height: 24 }
    const doc = documentOf(
      64,
      48,
      [
        filled('outside', { x: 0, y: 0, width: 8, height: 8 }, '#ff0000'),
        filled('inside', { x: 16, y: 12, width: 4, height: 4 }, '#00ff00'),
      ],
      crop,
    )

    const output = render(image, doc, { width: crop.width, height: crop.height })

    // The layer at the crop origin lands at the origin of the output.
    expect(pixelAt(output, 1, 1)).toEqual([0, 255, 0, 255])
    expect(countColor(output, [255, 0, 0])).toBe(0)
    // Untouched background is the source pixel at the crop offset, not at 0,0.
    expect(pixelAt(output, 20, 18)).toEqual(pixelAt(readPixels(image), 36, 30))
  })

  it('blacks out every pixel of an obscure region and keeps none of the source values', () => {
    const image = noiseImage(64, 48)
    const region = { x: 10, y: 8, width: 24, height: 16 }
    const doc = documentOf(64, 48, [
      { id: 'secret', kind: 'obscure', rect: region, mode: 'blackout', intensity: 0 },
    ])

    const output = render(image, doc, { width: 64, height: 48 })
    const source = readPixels(image)

    expect(distinctColors(output, region)).toBe(1)
    expect(pixelAt(output, region.x, region.y)).toEqual([0, 0, 0, 255])
    expect(pixelAt(output, region.x + region.width - 1, region.y + region.height - 1)).toEqual([0, 0, 0, 255])
    expect(identicalFraction(output, source, region)).toBe(0)
    // The pixel one step outside is untouched, so the fill has not spread.
    expect(pixelAt(output, region.x - 1, region.y)).toEqual(pixelAt(source, region.x - 1, region.y))
  })

  it('quantises a pixelated region to no more colours than it has blocks', () => {
    const image = noiseImage(64, 48)
    const region = { x: 8, y: 8, width: 32, height: 32 }
    const blockSize = 8
    const doc = documentOf(64, 48, [
      { id: 'secret', kind: 'obscure', rect: region, mode: 'pixelate', intensity: blockSize },
    ])

    const output = render(image, doc, { width: 64, height: 48 })

    const blocks = (region.width / blockSize) * (region.height / blockSize)
    expect(distinctColors(output, region)).toBeLessThanOrEqual(blocks)
    // And it did quantise rather than paint one flat colour over the lot.
    expect(distinctColors(output, region)).toBeGreaterThan(1)
  })

  it('smooths a blurred region measurably below the source it replaced', () => {
    const image = noiseImage(64, 48)
    const region = { x: 8, y: 8, width: 32, height: 32 }
    const doc = documentOf(64, 48, [
      { id: 'secret', kind: 'obscure', rect: region, mode: 'blur', intensity: 4 },
    ])

    const output = render(image, doc, { width: 64, height: 48 })
    const before = neighbourDelta(readPixels(image), region)
    const after = neighbourDelta(output, region)

    expect(before).toBeGreaterThan(50)
    expect(after).toBeLessThan(before * 0.2)
  })

  it('destroys the source pixels under every obscure mode', () => {
    const image = noiseImage(96, 96)
    const blackout = { x: 4, y: 4, width: 24, height: 24 }
    const pixelate = { x: 36, y: 4, width: 24, height: 24 }
    const blur = { x: 68, y: 4, width: 24, height: 24 }
    const doc = documentOf(96, 96, [
      { id: 'a', kind: 'obscure', rect: blackout, mode: 'blackout', intensity: 0 },
      { id: 'b', kind: 'obscure', rect: pixelate, mode: 'pixelate', intensity: 8 },
      { id: 'c', kind: 'obscure', rect: blur, mode: 'blur', intensity: 5 },
    ])

    const output = render(image, doc, { width: 96, height: 96 })
    const source = readPixels(image)

    // Not one original pixel value survives anywhere in any of the regions.
    expect(identicalFraction(output, source, blackout)).toBe(0)
    expect(identicalFraction(output, source, pixelate)).toBe(0)
    expect(identicalFraction(output, source, blur)).toBe(0)

    // And the detail that carried the secret is gone, not merely displaced.
    for (const region of [blackout, pixelate, blur]) {
      expect(neighbourDelta(output, region)).toBeLessThan(neighbourDelta(source, region) * 0.2)
    }
  })

  it('places an obscure region correctly under a scale the caller applied', () => {
    const image = noiseImage(64, 48)
    const region = { x: 10, y: 8, width: 20, height: 16 }
    const doc = documentOf(64, 48, [
      { id: 'secret', kind: 'obscure', rect: region, mode: 'blackout', intensity: 0 },
    ])

    // The renderer is told nothing about the scale; it only sees the transform.
    const output = render(image, doc, { width: 128, height: 96 }, (ctx) => ctx.scale(2, 2))

    expect(pixelAt(output, 20, 16)).toEqual([0, 0, 0, 255])
    expect(pixelAt(output, 59, 47)).toEqual([0, 0, 0, 255])
    expect(countColor(output, [0, 0, 0])).toBe(40 * 32)
  })

  it('destroys the source even at the weakest intensity a tool can hand it', () => {
    // Zero is what an uninitialised tool, a slider at its left stop or a
    // document from an older version of the app can all produce. The blur and
    // the mosaic have to mean something anyway, because the user reads the
    // layer as "this is redacted" and not as "this is redacted a bit".
    const image = noiseImage(64, 48)
    const pixelate = { x: 4, y: 4, width: 24, height: 24 }
    const blur = { x: 34, y: 4, width: 24, height: 24 }
    const doc = documentOf(64, 48, [
      { id: 'a', kind: 'obscure', rect: pixelate, mode: 'pixelate', intensity: 0 },
      { id: 'b', kind: 'obscure', rect: blur, mode: 'blur', intensity: 0 },
    ])

    const output = render(image, doc, { width: 64, height: 48 })
    const source = readPixels(image)

    expect(identicalFraction(output, source, pixelate)).toBe(0)
    expect(identicalFraction(output, source, blur)).toBe(0)
  })
})
