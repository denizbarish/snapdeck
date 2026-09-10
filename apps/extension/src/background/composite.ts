import type { ScrollPlan, ScrollStep } from '../content/plan'

/**
 * Where the layers become one image.
 *
 * The plan is CSS pixels because that is the only unit a page can be measured
 * and scrolled in; the captures are device pixels because that is what the
 * screen has. Every rectangle here is multiplied by the ratio on the way in,
 * and source and destination are always the same size, so no pixel is ever
 * resampled and the text stays as sharp as the screen had it.
 */

export type CapturedLayer = { bitmap: ImageBitmap; step: ScrollStep }

/** The only format the bridge carries, and the only lossless one on offer. */
const PNG_MIME_TYPE = 'image/png'

/**
 * Pastes each layer where its step says, in device pixels.
 *
 * The plan is in CSS pixels and the bitmaps are in device pixels, so every
 * rectangle is multiplied by the ratio on the way in. Source and destination
 * rectangles are always the same size: nothing here resamples, which is what
 * keeps text as sharp as the screen had it.
 */
export function compositeLayers(
  layers: CapturedLayer[],
  plan: ScrollPlan,
  devicePixelRatio: number,
): OffscreenCanvas {
  const width = plan.compositeWidth * devicePixelRatio
  const canvas = new OffscreenCanvas(width, plan.compositeHeight * devicePixelRatio)

  const context = canvas.getContext('2d')
  if (context === null) {
    throw new Error('the browser refused a 2d context for the composite canvas')
  }

  for (const { bitmap, step } of layers) {
    // `sourceTop` is what the step before this one already carries, so the
    // source rectangle starts below it and the band is pasted exactly once.
    const height = step.height * devicePixelRatio
    context.drawImage(
      bitmap,
      0,
      step.sourceTop * devicePixelRatio,
      width,
      height,
      0,
      step.destTop * devicePixelRatio,
      width,
      height,
    )
  }

  return canvas
}

export function compositeToPng(
  layers: CapturedLayer[],
  plan: ScrollPlan,
  devicePixelRatio: number,
): Promise<Blob> {
  return compositeLayers(layers, plan, devicePixelRatio).convertToBlob({ type: PNG_MIME_TYPE })
}
