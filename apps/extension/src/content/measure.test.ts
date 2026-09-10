import { describe, expect, it } from 'vitest'

import { measurePage, type MeasurableWindow } from './measure'

/**
 * `measurePage` takes a structural type rather than a `Window`, so the two
 * decisions it makes are arithmetic and belong to the `node` project. A real
 * `window` still satisfies the type, which is the point of writing it that way.
 */
function windowLike(overrides: {
  documentScrollHeight: number
  bodyScrollHeight: number
  devicePixelRatio?: number
  clientHeight?: number
  innerHeight?: number
  innerWidth?: number
}): MeasurableWindow {
  return {
    innerWidth: overrides.innerWidth ?? 1280,
    innerHeight: overrides.innerHeight ?? 800,
    devicePixelRatio: overrides.devicePixelRatio ?? 2,
    document: {
      documentElement: {
        scrollHeight: overrides.documentScrollHeight,
        clientHeight: overrides.clientHeight ?? 800,
      },
      body: { scrollHeight: overrides.bodyScrollHeight },
    },
  }
}

describe('measurePage', () => {
  it('takes the taller of the two heights the document reports', () => {
    // Q1. Which of the two is the honest one depends on the page's own layout,
    // and reading either one alone captures a short page on the sites where the
    // other is the tall one.
    expect(
      measurePage(windowLike({ documentScrollHeight: 5000, bodyScrollHeight: 800 })).documentHeight,
    ).toBe(5000)
    expect(
      measurePage(windowLike({ documentScrollHeight: 800, bodyScrollHeight: 5000 })).documentHeight,
    ).toBe(5000)
  })

  it('falls back to a device pixel ratio of 1 when the window reports an unusable one', () => {
    // Q2. A ratio of zero multiplies out to a zero-pixel canvas, and NaN
    // poisons every number the plan derives from it.
    expect(
      measurePage(
        windowLike({ documentScrollHeight: 2000, bodyScrollHeight: 2000, devicePixelRatio: 0 }),
      ).devicePixelRatio,
    ).toBe(1)
    expect(
      measurePage(
        windowLike({ documentScrollHeight: 2000, bodyScrollHeight: 2000, devicePixelRatio: NaN }),
      ).devicePixelRatio,
    ).toBe(1)
  })
})
