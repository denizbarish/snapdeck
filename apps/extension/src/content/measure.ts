/**
 * What the page is, in CSS pixels, before anything is captured.
 *
 * Pure, and it takes a structural type rather than a `Window`, for the reason
 * `planScroll` takes `PageMetrics` rather than reading globals: the two
 * decisions in here are arithmetic, arithmetic is worth testing, and a live
 * `window` is not something a unit test can arrange. A real `window` satisfies
 * `MeasurableWindow`, so the shell passes one straight in.
 */

export type MeasurableWindow = {
  innerWidth: number
  innerHeight: number
  devicePixelRatio: number
  document: {
    documentElement: { scrollHeight: number; clientHeight: number }
    body: { scrollHeight: number }
  }
}

export type PageMetrics = {
  documentHeight: number // CSS px
  viewportHeight: number // CSS px
  viewportWidth: number // CSS px
  devicePixelRatio: number
}

/**
 * The ratio to use when the window reports one that cannot be multiplied by.
 * Zero would size the composite canvas at zero pixels and NaN would spread
 * through every number derived from it.
 */
const FALLBACK_DEVICE_PIXEL_RATIO = 1

export function measurePage(win: MeasurableWindow): PageMetrics {
  const { documentElement, body } = win.document

  // Which of the two is the honest height depends on the page's own layout, and
  // reading either alone captures a short page on the sites where the other is
  // the tall one.
  const documentHeight = Math.max(documentElement.scrollHeight, body.scrollHeight)

  // The scroll position a browser will actually honour tops out at
  // `scrollHeight - clientHeight`, so `clientHeight` is the figure the plan has
  // to clamp against. It differs from `innerHeight` by the height of a
  // horizontal scrollbar, and a plan built on the larger of the two asks for a
  // scroll the page silently refuses. `innerHeight` covers the case where the
  // document element has no box at all.
  const viewportHeight =
    documentElement.clientHeight > 0 ? documentElement.clientHeight : win.innerHeight

  const ratio = win.devicePixelRatio
  const devicePixelRatio =
    Number.isFinite(ratio) && ratio > 0 ? ratio : FALLBACK_DEVICE_PIXEL_RATIO

  return {
    documentHeight,
    viewportHeight,
    viewportWidth: win.innerWidth,
    devicePixelRatio,
  }
}
