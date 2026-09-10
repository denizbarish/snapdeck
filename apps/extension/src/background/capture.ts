import type { PageMetrics } from '../content/measure'
import { planScroll, type ScrollPlan } from '../content/plan'
import { compositeToPng, type CapturedLayer } from './composite'
import { CAPTURE_INTERVAL_MS, createThrottle } from './throttle'

/**
 * The loop itself. Everything it knows about a page it asks `deps` for, and
 * every number it works from comes out of `planScroll`, so what is left here is
 * an order of calls: what happens, in what order, and what still happens when
 * one of them fails.
 *
 * `deps` rather than `chrome` for the same reason the content modules take
 * plain data: the order is the part worth testing, and the tab it runs against
 * is not something a test can arrange.
 */

export type CaptureDeps = {
  measure(): Promise<PageMetrics>
  /** Hides every sticky and fixed element, or restores them. */
  setPinnedHidden(hidden: boolean): Promise<void>
  scrollTo(y: number): Promise<void>
  /** Waits for lazy-loaded content to arrive. */
  settle(): Promise<void>
  captureVisible(): Promise<ImageBitmap>
  restoreScroll(): Promise<void>
}

export type FullPageCapture = { blob: Blob; plan: ScrollPlan; metrics: PageMetrics }

/** The index of the layer the pinned elements are still visible for. */
const LAYER_THAT_KEEPS_THE_PINNED_ELEMENTS = 0

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms)
  })
}

export async function captureFullPage(
  deps: CaptureDeps,
  maxPixels: number,
): Promise<FullPageCapture> {
  const metrics = await deps.measure()
  const plan = planScroll(metrics, maxPixels)

  // One throttle per capture, because one full-page capture is one run down a
  // page and the quota only has to hold within it.
  const throttled = createThrottle(CAPTURE_INTERVAL_MS, Date.now, sleep)
  const layers: CapturedLayer[] = []

  try {
    for (const [index, step] of plan.steps.entries()) {
      await deps.scrollTo(step.scrollY)
      // Lazy-loaded content arrives after the scroll, not with it.
      await deps.settle()
      layers.push({ bitmap: await throttled(() => deps.captureVisible()), step })

      // The page's own header is usually pinned, and hiding it before the first
      // capture would leave it out of every layer. Hiding it after the first is
      // what puts it in the composite exactly once.
      if (index === LAYER_THAT_KEEPS_THE_PINNED_ELEMENTS) {
        await deps.setPinnedHidden(true)
      }
    }
  } finally {
    // A failed capture costs the user a screenshot. A page left scrolled with
    // its header invisible costs them a reload, and they have no way of knowing
    // that is what it needs. Both repairs are attempted even if the first one
    // fails, because they undo two separate things.
    await attemptRepair(deps.setPinnedHidden(false))
    await attemptRepair(deps.restoreScroll())
  }

  try {
    return { blob: await compositeToPng(layers, plan, metrics.devicePixelRatio), plan, metrics }
  } finally {
    // Every layer is a bitmap the size of the window. A long page holds dozens
    // of them at once, and waiting for the collector to notice is how a service
    // worker with a memory ceiling gets killed in the middle of a capture.
    for (const layer of layers) {
      layer.bitmap.close()
    }
  }
}

/** Runs a repair, and reports its failure rather than raising it. */
async function attemptRepair(repair: Promise<void>): Promise<void> {
  try {
    await repair
  } catch (error) {
    console.warn('snapdeck: could not put the page back', error)
  }
}
