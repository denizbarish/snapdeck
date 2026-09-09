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
import { obscureStrength, renderDocument } from './render'
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
  sourceCorrelation,
  strokeContrast,
  textImage,
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

function countColor(
  pixels: ImageData,
  rgb: [number, number, number],
  within: Rect = { x: 0, y: 0, width: pixels.width, height: pixels.height },
): number {
  let count = 0
  for (let y = within.y; y < within.y + within.height; y += 1) {
    for (let x = within.x; x < within.x + within.width; x += 1) {
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

  it('destroys structured content even at the weakest intensity a tool can hand it', () => {
    // Zero is what an uninitialised tool, a slider at its left stop or a
    // document from an older version of the app can all produce. The measure
    // here is legibility on text-like strokes, not byte identity on noise:
    // noise has no two equal neighbours, so any averaging changes every byte
    // and a mosaic of two-pixel blocks scores perfectly while leaving the
    // glyph edges at full contrast.
    const image = textImage(64, 16)
    const pixelate = { x: 2, y: 0, width: 28, height: 16 }
    const blur = { x: 34, y: 0, width: 28, height: 16 }
    const doc = documentOf(64, 16, [
      { id: 'a', kind: 'obscure', rect: pixelate, mode: 'pixelate', intensity: 0 },
      { id: 'b', kind: 'obscure', rect: blur, mode: 'blur', intensity: 0 },
    ])

    const output = render(image, doc, { width: 64, height: 16 })
    const source = readPixels(image)

    // The strokes are at full contrast in the source, by construction.
    expect(strokeContrast(source, pixelate)).toBe(255)
    expect(strokeContrast(source, blur)).toBe(255)

    // At the floor a 2px mosaic leaves 255 contrast and a correlation of 0.84,
    // so these bounds are the whole point of the floor.
    expect(strokeContrast(output, pixelate)).toBeLessThan(160)
    expect(sourceCorrelation(output, source, pixelate)).toBeLessThan(0.45)
    expect(strokeContrast(output, blur)).toBeLessThan(130)
    expect(sourceCorrelation(output, source, blur)).toBeLessThan(0.45)
  })

  it('scales the floor with the region, so a big box is not redacted at a small box\'s strength', () => {
    // A box drawn tightly round a line of text holds glyphs about as tall as
    // the box. A fixed floor that suits 16px text is a rounding error on 48px
    // text, which is why the minimum is derived from the region as well.
    const image = textImage(160, 48)
    const region = { x: 0, y: 0, width: 160, height: 48 }
    const doc = documentOf(160, 48, [
      { id: 'a', kind: 'obscure', rect: region, mode: 'pixelate', intensity: 1 },
    ])

    const output = render(image, doc, { width: 160, height: 48 })
    const source = readPixels(image)

    // Blocks of at least ceil(48 / 4) = 12 source pixels: no more distinct
    // colours than that grid has cells. The absolute floor of 6 would allow
    // four times as many.
    const blocks = Math.ceil(160 / 12) * Math.ceil(48 / 12)
    expect(distinctColors(output, region)).toBeLessThanOrEqual(blocks)
    expect(sourceCorrelation(output, source, region)).toBeLessThan(0.5)
  })

  it('redacts rather than freezing or passing through when the intensity is not a usable number', () => {
    // `NaN` used to make the mosaic's loop end before its first step, leaving
    // every source pixel untouched with nothing thrown; `Infinity` used to make
    // the blur's window loop never advance, which is a frozen editor mid-drag.
    // A finite but absurd intensity is the same hazard with a different cause,
    // and it is the one the clamp to the region has to catch: this test only
    // finishes at all if the work stays bounded by the region's own size.
    const image = textImage(160, 24)
    const absurd = 1e9
    const regions = {
      pixelateNaN: { x: 0, y: 0, width: 30, height: 24 },
      blurNaN: { x: 32, y: 0, width: 30, height: 24 },
      pixelateInfinite: { x: 64, y: 0, width: 30, height: 24 },
      blurInfinite: { x: 96, y: 0, width: 30, height: 24 },
      blurAbsurd: { x: 128, y: 0, width: 30, height: 24 },
    }
    const doc = documentOf(160, 24, [
      { id: 'a', kind: 'obscure', rect: regions.pixelateNaN, mode: 'pixelate', intensity: Number.NaN },
      { id: 'b', kind: 'obscure', rect: regions.blurNaN, mode: 'blur', intensity: Number.NaN },
      { id: 'c', kind: 'obscure', rect: regions.pixelateInfinite, mode: 'pixelate', intensity: Number.POSITIVE_INFINITY },
      { id: 'd', kind: 'obscure', rect: regions.blurInfinite, mode: 'blur', intensity: Number.POSITIVE_INFINITY },
      { id: 'e', kind: 'obscure', rect: regions.blurAbsurd, mode: 'blur', intensity: absurd },
    ])

    const output = render(image, doc, { width: 160, height: 24 })
    const source = readPixels(image)

    for (const region of Object.values(regions)) {
      expect(strokeContrast(output, region)).toBeLessThan(160)
      expect(sourceCorrelation(output, source, region)).toBeLessThan(0.5)
    }
  })

  it('resamples nothing when the crop origin is fractional', () => {
    // The canvas is sized from a rounded crop, so translating by the raw origin
    // would put every source pixel half a pixel off its own and resample the
    // whole screenshot bilinearly.
    const image = noiseImage(64, 48)
    const doc = documentOf(64, 48, [], { x: 5.5, y: 7.5, width: 31.2, height: 23.4 })

    const output = render(image, doc, { width: 31, height: 23 })

    expect(identicalFraction(output, readPixels(image), { x: 0, y: 0, width: 31, height: 23 }, { x: 6, y: 8 })).toBe(1)
  })

  it('draws the same text whatever font the context arrived with', () => {
    // An invalid font string is a silent no-op: the context keeps the font it
    // already had. Inherited state would then be a second way for a preview and
    // an export to disagree, which is the one thing this module rules out.
    const image = noiseImage(64, 48)
    const doc = documentOf(64, 48, [
      {
        id: 'a',
        kind: 'text',
        rect: { x: 8, y: 8, width: 48, height: 24 },
        content: 'Hi',
        style: { color: '#ff0000', size: 16, family: 'not a valid; font family' },
      },
    ])

    const large = render(image, doc, { width: 64, height: 48 }, (ctx) => {
      ctx.font = '40px monospace'
    })
    const small = render(image, doc, { width: 64, height: 48 }, (ctx) => {
      ctx.font = '6px monospace'
    })

    expect(differingBytes(large, small)).toBe(0)
  })

  it('draws from a known state whatever compositing the caller left set', () => {
    const image = noiseImage(64, 48)
    const doc = documentOf(64, 48, [filled('a', { x: 8, y: 8, width: 16, height: 16 }, '#ff0000')])

    const clean = render(image, doc, { width: 64, height: 48 })
    const dirty = render(image, doc, { width: 64, height: 48 }, (ctx) => {
      ctx.globalAlpha = 0.25
      ctx.globalCompositeOperation = 'xor'
    })

    expect(differingBytes(clean, dirty)).toBe(0)
  })
})

/**
 * One test per layer kind.
 *
 * The export-versus-preview comparison passes identically if every one of these
 * draws nothing at all, so each kind needs a pin of its own: ink where it
 * belongs, no ink where it does not, and the geometry the model and the hit
 * testing already committed to.
 */
describe('drawing each layer kind', () => {
  const RED: [number, number, number] = [255, 0, 0]

  it('draws an arrow as a shaft that stops at a head wider than itself', () => {
    const image = noiseImage(64, 48)
    const doc = documentOf(64, 48, [
      { id: 'a', kind: 'arrow', from: { x: 8, y: 24 }, to: { x: 56, y: 24 }, style: { color: '#ff0000', width: 4 } },
    ])

    const output = render(image, doc, { width: 64, height: 48 })

    expect(pixelAt(output, 30, 24)).toEqual([255, 0, 0, 255])
    // The head is `width * 4` long, so it starts at x = 40 and flares from
    // there: a column inside it carries more ink than the 4px shaft.
    const shaft = countColor(output, RED, { x: 20, y: 0, width: 1, height: 48 })
    const head = countColor(output, RED, { x: 44, y: 0, width: 1, height: 48 })
    expect(shaft).toBeGreaterThan(0)
    expect(head).toBeGreaterThan(shaft)
    // Nothing past the tip.
    expect(countColor(output, RED, { x: 58, y: 0, width: 6, height: 48 })).toBe(0)
  })

  it('draws a polyline through every point, and a single point as a dot', () => {
    const image = noiseImage(64, 48)
    const doc = documentOf(64, 48, [
      { id: 'a', kind: 'line', points: [{ x: 8, y: 8 }, { x: 8, y: 40 }, { x: 40, y: 40 }], style: { color: '#ff0000', width: 4 } },
      { id: 'b', kind: 'line', points: [{ x: 56, y: 8 }], style: { color: '#ff0000', width: 6 } },
    ])

    const output = render(image, doc, { width: 64, height: 48 })

    expect(pixelAt(output, 8, 24)).toEqual([255, 0, 0, 255])
    expect(pixelAt(output, 24, 40)).toEqual([255, 0, 0, 255])
    // The corner the polyline never turned through stays untouched.
    expect(countColor(output, RED, { x: 20, y: 4, width: 8, height: 8 })).toBe(0)
    // A stroke sampled once is a dot rather than nothing at all.
    expect(pixelAt(output, 56, 8)).toEqual([255, 0, 0, 255])
    expect(countColor(output, RED, { x: 50, y: 2, width: 13, height: 13 })).toBeGreaterThan(8)
  })

  it('inscribes an ellipse in its rect, so the rect corners stay clear', () => {
    const image = noiseImage(64, 48)
    const rect = { x: 8, y: 8, width: 32, height: 32 }
    const doc = documentOf(64, 48, [
      { id: 'a', kind: 'ellipse', rect, style: { stroke: { color: '#ff0000', width: 0 }, fill: '#ff0000' } },
    ])

    const output = render(image, doc, { width: 64, height: 48 })
    const source = readPixels(image)

    expect(pixelAt(output, 24, 24)).toEqual([255, 0, 0, 255])
    expect(pixelAt(output, 24, 10)).toEqual([255, 0, 0, 255])
    // The corner of the bounding rect is what separates an ellipse from a rect.
    expect(pixelAt(output, 9, 9)).toEqual(pixelAt(source, 9, 9))
  })

  it('sets text from the top-left of its rect and puts each line below the last', () => {
    const image = noiseImage(64, 48)
    const rect = { x: 8, y: 8, width: 48, height: 40 }
    const doc = documentOf(64, 48, [
      { id: 'a', kind: 'text', rect, content: 'AB\nCD', style: { color: '#ff0000', size: 16, family: 'monospace' } },
    ])

    const output = render(image, doc, { width: 64, height: 48 })

    // A line height of size * 1.25 puts the second line's box at y = 28.
    expect(countColor(output, RED, { x: 8, y: 8, width: 48, height: 16 })).toBeGreaterThan(0)
    expect(countColor(output, RED, { x: 8, y: 28, width: 48, height: 16 })).toBeGreaterThan(0)
    // Baseline `top`, not `middle` or `alphabetic`: no ink above the rect.
    expect(countColor(output, RED, { x: 0, y: 0, width: 64, height: 7 })).toBe(0)
  })

  it('multiplies a highlight into what is under it instead of veiling it', () => {
    const image = noiseImage(64, 48)
    const rect = { x: 8, y: 8, width: 24, height: 24 }
    const doc = documentOf(64, 48, [{ id: 'a', kind: 'highlight', rect, color: '#ff0000' }])

    const output = render(image, doc, { width: 64, height: 48 })
    const source = readPixels(image)

    // Multiply by pure red keeps the red channel exactly and zeroes the others.
    // A translucent fill would wash all three towards the highlight colour.
    const [red, green, blue] = pixelAt(output, 16, 16)
    expect(red).toBe(pixelAt(source, 16, 16)[0])
    expect([green, blue]).toEqual([0, 0])
    expect(pixelAt(output, 33, 16)).toEqual(pixelAt(source, 33, 16))
  })

  it('draws a step badge whose ink is the diameter the model and the hit test agree on', () => {
    // `size` is the diameter: `boundsOf` lays a square of `size` around the
    // centre and `hit.ts` tests within `size / 2` of it. A badge drawn at
    // `radius = size` would put twice the ink under the same hit target.
    const image = noiseImage(64, 48)
    const size = 20
    const center = { x: 32, y: 24 }
    const doc = documentOf(64, 48, [
      { id: 'a', kind: 'step', center, index: 7, style: { fill: '#ff0000', color: '#ffffff', size } },
    ])

    const output = render(image, doc, { width: 64, height: 48 })
    const source = readPixels(image)

    expect(pixelAt(output, center.x, center.y - 9)).toEqual([255, 0, 0, 255])
    expect(pixelAt(output, center.x, center.y - 14)).toEqual(pixelAt(source, center.x, center.y - 14))
    expect(pixelAt(output, center.x - 14, center.y)).toEqual(pixelAt(source, center.x - 14, center.y))
    // And the number is actually set inside it.
    const badge = { x: center.x - size / 2, y: center.y - size / 2, width: size, height: size }
    expect(countColor(output, [255, 255, 255], badge)).toBeGreaterThan(0)
  })
})

describe('obscureStrength', () => {
  const box = (width: number, height: number): Rect => ({ x: 0, y: 0, width, height })

  it('never falls below the absolute floor, whatever the tool asked for', () => {
    expect(obscureStrength('pixelate', box(16, 16), 0)).toBe(6)
    expect(obscureStrength('pixelate', box(16, 16), -40)).toBe(6)
    expect(obscureStrength('blur', box(16, 16), 0)).toBe(4)
    expect(obscureStrength('blur', box(16, 16), -40)).toBe(4)
  })

  it('derives a minimum from the region\'s smaller side', () => {
    // A quarter of the smaller side for a mosaic: four blocks across the text.
    expect(obscureStrength('pixelate', box(400, 48), 1)).toBe(12)
    expect(obscureStrength('pixelate', box(48, 400), 1)).toBe(12)
    // A sixth of it for a blur, whose window spans about three of them.
    expect(obscureStrength('blur', box(400, 48), 1)).toBe(8)
    expect(obscureStrength('blur', box(48, 400), 1)).toBe(8)
  })

  it('honours an intensity stronger than either minimum', () => {
    expect(obscureStrength('pixelate', box(48, 48), 24)).toBe(24)
    expect(obscureStrength('blur', box(48, 48), 24)).toBe(24)
  })

  it('reads a flipped region by its size, not its sign', () => {
    expect(obscureStrength('pixelate', { x: 40, y: 40, width: -400, height: -48 }, 1)).toBe(12)
  })

  it('falls back to the floor for an intensity or a region that is not a number', () => {
    expect(obscureStrength('pixelate', box(16, 16), Number.NaN)).toBe(6)
    expect(obscureStrength('blur', box(16, 16), Number.POSITIVE_INFINITY)).toBe(4)
    expect(obscureStrength('pixelate', box(Number.NaN, 16), 0)).toBe(6)
  })
})
