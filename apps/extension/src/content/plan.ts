import type { PageMetrics } from './measure'

/**
 * Where to scroll, what to keep of each capture and where it goes in the
 * composite. Pure arithmetic over `PageMetrics`, which is what lets the
 * off-by-one that decides whether a full page comes out seamless be tested
 * without a browser.
 */

export type ScrollStep = {
  index: number
  scrollY: number // CSS px, where the page is scrolled for this layer
  sourceTop: number // CSS px, where the useful part of the layer starts in the viewport
  height: number // CSS px, what this layer contributes
  destTop: number // CSS px, where it is pasted in the composite
}

export type ScrollPlan = {
  steps: ScrollStep[]
  compositeWidth: number // CSS px
  compositeHeight: number // CSS px
  truncated: boolean
}

/** A composite has to have at least one row, however small the budget is. */
const MIN_COMPOSITE_HEIGHT = 1

export function planScroll(metrics: PageMetrics, maxPixels: number): ScrollPlan {
  const { documentHeight, viewportHeight, viewportWidth, devicePixelRatio } = metrics

  // The composite is measured in CSS pixels but it is paid for in device
  // pixels, which is where both ratios come in.
  const pixelsPerCssRow = viewportWidth * devicePixelRatio * devicePixelRatio
  const truncated = documentHeight * pixelsPerCssRow > maxPixels
  const compositeHeight = truncated
    ? Math.max(MIN_COMPOSITE_HEIGHT, Math.floor(maxPixels / pixelsPerCssRow))
    : documentHeight

  // A page shorter than one viewport still needs one capture, and no step is
  // produced for a band that truncation has already cut off the composite.
  const count = Math.max(1, Math.ceil(compositeHeight / viewportHeight))
  const highestScrollY = Math.max(0, documentHeight - viewportHeight)

  const steps: ScrollStep[] = []
  for (let index = 0; index < count; index += 1) {
    const isLast = index === count - 1
    // The last step carries whatever the ones before it did not cover, which is
    // a whole viewport when the composite divides evenly and less when it does
    // not.
    const height = isLast ? compositeHeight - index * viewportHeight : viewportHeight
    const destTop = isLast ? compositeHeight - height : index * viewportHeight
    const scrollY = Math.min(index * viewportHeight, highestScrollY)

    // The viewport shows document rows `scrollY` onwards, and this step wants
    // rows `destTop` onwards, so the difference is what to skip at the top of
    // the capture. On the last step of a page that cannot scroll far enough it
    // is the overlap with the step before, and on every other step it is zero.
    const sourceTop = destTop - scrollY

    steps.push({ index, scrollY, sourceTop, height, destTop })
  }

  return { steps, compositeWidth: viewportWidth, compositeHeight, truncated }
}
