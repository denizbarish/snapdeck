import { describe, expect, it } from 'vitest'

import type { PageMetrics } from '../content/measure'
import { planScroll } from '../content/plan'
import { compositeLayers, compositeToPng, type CapturedLayer } from './composite'

/**
 * These belong to the `browser` project, and a fake canvas would make every one
 * of them meaningless. K1 claims a size in device pixels, K3 claims that a
 * source rectangle really lands where it says, and K4 claims that a real 2D
 * context leaves the pixels it copies alone. All three are answers only a real
 * `OffscreenCanvas` has; a stub would answer out of the values the test itself
 * wrote.
 */

/** More pixels than any composite in this file needs, so nothing truncates. */
const NO_LIMIT = Number.MAX_SAFE_INTEGER

/** The ratio every case here runs at: the one where CSS pixels are not device pixels. */
const DPR = 2

/** Opaque, and far enough apart that a blend of any two is neither. */
type Rgb = [number, number, number]
const RED: Rgb = [255, 0, 0]
const BLUE: Rgb = [0, 0, 255]
const GREEN: Rgb = [0, 255, 0]
const MAGENTA: Rgb = [255, 0, 255]
const WHITE: Rgb = [255, 255, 255]
const BLACK: Rgb = [0, 0, 0]

const OPAQUE = 255
const CHANNELS_PER_PIXEL = 4
const COLOUR_CHANNELS = 3

function metrics(
  documentHeight: number,
  viewportHeight: number,
  viewportWidth: number,
  devicePixelRatio: number,
): PageMetrics {
  return { documentHeight, viewportHeight, viewportWidth, devicePixelRatio }
}

function contextOf(canvas: OffscreenCanvas): OffscreenCanvasRenderingContext2D {
  const context = canvas.getContext('2d')
  if (context === null) {
    throw new Error('the browser refused a 2d context, which nothing here can work without')
  }
  return context
}

/** A bitmap the size of a captured viewport, painted by `colourAt`. */
async function bitmap(
  width: number,
  height: number,
  colourAt: (x: number, y: number) => Rgb,
): Promise<ImageBitmap> {
  const image = new ImageData(width, height)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      image.data.set([...colourAt(x, y), OPAQUE], (y * width + x) * CHANNELS_PER_PIXEL)
    }
  }
  return createImageBitmap(image)
}

/** A bitmap of one colour throughout. */
function solid(width: number, height: number, colour: Rgb): Promise<ImageBitmap> {
  return bitmap(width, height, () => colour)
}

function pixelAt(canvas: OffscreenCanvas, x: number, y: number): Rgb {
  const data = contextOf(canvas).getImageData(x, y, 1, 1).data
  const [red, green, blue] = Array.from(data.slice(0, COLOUR_CHANNELS))
  return [red ?? -1, green ?? -1, blue ?? -1]
}

/** Every pixel of the composite whose colour is not the one `colourAt` names. */
function differencesFrom(
  canvas: OffscreenCanvas,
  colourAt: (x: number, y: number) => Rgb,
): { x: number; y: number; colour: Rgb; expected: Rgb }[] {
  const image = contextOf(canvas).getImageData(0, 0, canvas.width, canvas.height)
  const differences: { x: number; y: number; colour: Rgb; expected: Rgb }[] = []
  for (let y = 0; y < canvas.height; y += 1) {
    for (let x = 0; x < canvas.width; x += 1) {
      const start = (y * canvas.width + x) * CHANNELS_PER_PIXEL
      const [red, green, blue] = Array.from(
        image.data.slice(start, start + COLOUR_CHANNELS),
      )
      const colour: Rgb = [red ?? -1, green ?? -1, blue ?? -1]
      const expected = colourAt(x, y)
      if (colour.some((channel, index) => channel !== expected[index])) {
        differences.push({ x, y, colour, expected })
      }
    }
  }
  return differences
}

describe('compositeLayers', () => {
  it('sizes the canvas in device pixels, not CSS pixels', () => {
    // K1. The plan is CSS pixels because that is what the page is measured and
    // scrolled in; the captures are device pixels because that is what the
    // screen has. A canvas built at the plan's own numbers throws away half of
    // every capture on a retina display.
    const plan = planScroll(metrics(600, 600, 400, DPR), NO_LIMIT)

    const canvas = compositeLayers([], plan, DPR)

    expect(plan.compositeWidth).toBe(400)
    expect(plan.compositeHeight).toBe(600)
    expect(canvas.width).toBe(800)
    expect(canvas.height).toBe(1200)
  })

  it('pastes each layer at the row its own step names', async () => {
    // K2. Two viewports of solid colour, with the second step overlapping the
    // first because the page does not divide evenly. The boundary between the
    // two colours is the only evidence of where the second layer landed.
    const page = metrics(14, 10, 4, DPR)
    const plan = planScroll(page, NO_LIMIT)
    const [first, second] = plan.steps
    if (first === undefined || second === undefined) {
      throw new Error('this page is two steps tall')
    }
    const width = page.viewportWidth * DPR
    const height = page.viewportHeight * DPR
    const layers: CapturedLayer[] = [
      { bitmap: await solid(width, height, RED), step: first },
      { bitmap: await solid(width, height, BLUE), step: second },
    ]

    const canvas = compositeLayers(layers, plan, DPR)

    const boundary = second.destTop * DPR
    expect(pixelAt(canvas, 0, boundary - 1)).toEqual(RED)
    expect(pixelAt(canvas, 0, boundary + 1)).toEqual(BLUE)
  })

  it('drops the band of the last capture that the one before it already carries', async () => {
    // K3. The whole reason `sourceTop` exists. The page cannot scroll a full
    // viewport for the last step, so the top of that capture repeats rows the
    // previous layer already holds. Painted here in a colour that appears
    // nowhere else, so a composite that pastes it is caught by its own colour.
    const page = metrics(25, 10, 4, DPR)
    const plan = planScroll(page, NO_LIMIT)
    const [first, second, third] = plan.steps
    if (first === undefined || second === undefined || third === undefined) {
      throw new Error('this page is three steps tall')
    }
    const width = page.viewportWidth * DPR
    const height = page.viewportHeight * DPR
    const repeatedRows = third.sourceTop * DPR
    const layers: CapturedLayer[] = [
      { bitmap: await solid(width, height, RED), step: first },
      { bitmap: await solid(width, height, BLUE), step: second },
      {
        bitmap: await bitmap(width, height, (_x, y) => (y < repeatedRows ? MAGENTA : GREEN)),
        step: third,
      },
    ]

    const canvas = compositeLayers(layers, plan, DPR)

    const boundary = third.destTop * DPR
    // The band the second layer already painted is still the second layer's.
    expect(pixelAt(canvas, 0, boundary - 1)).toEqual(BLUE)
    // And the last layer contributes the rows below the repeat, not the repeat.
    expect(pixelAt(canvas, 0, boundary)).toEqual(GREEN)
    expect(pixelAt(canvas, 0, canvas.height - 1)).toEqual(GREEN)
  })

  it('resamples nothing, so a one pixel line stays one pixel of one colour', async () => {
    // K4. A page shorter than the viewport: the capture carries rows the
    // document does not have, and the source rectangle is what keeps them out.
    // A five argument `drawImage` would squeeze the whole capture into the
    // document's height instead, and the give-away is a blend appearing where
    // the composite should hold nothing but the two stripe colours.
    const page = metrics(6, 10, 8, DPR)
    const plan = planScroll(page, NO_LIMIT)
    const [only] = plan.steps
    if (only === undefined) {
      throw new Error('every page is at least one step tall')
    }
    const width = page.viewportWidth * DPR
    const height = page.viewportHeight * DPR
    const documentRows = page.documentHeight * DPR
    const stripe = (x: number): Rgb => (x % 2 === 0 ? WHITE : BLACK)
    const layers: CapturedLayer[] = [
      {
        bitmap: await bitmap(width, height, (x, y) => (y < documentRows ? stripe(x) : MAGENTA)),
        step: only,
      },
    ]

    const canvas = compositeLayers(layers, plan, DPR)

    expect({ width: canvas.width, height: canvas.height }).toEqual({ width, height: documentRows })
    expect(differencesFrom(canvas, (x) => stripe(x))).toEqual([])
  })
})

describe('compositeToPng', () => {
  it('encodes the composite as a PNG of the canvas it was drawn on', async () => {
    // K5. The bridge carries a PNG and nothing else, and a blob that decodes to
    // a different size than the canvas would mean the encoder, not the
    // composite, decided what the user gets.
    const plan = planScroll(metrics(600, 600, 400, DPR), NO_LIMIT)

    const blob = await compositeToPng([], plan, DPR)

    expect(blob.type).toBe('image/png')
    expect(blob.size).toBeGreaterThan(0)
    const decoded = await createImageBitmap(blob)
    expect({ width: decoded.width, height: decoded.height }).toEqual({ width: 800, height: 1200 })
  })
})
