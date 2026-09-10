import { describe, expect, it } from 'vitest'

import type { PageMetrics } from './measure'
import { planScroll, type ScrollPlan } from './plan'

/**
 * The scroll plan is where a full-page capture is won or lost. Every number
 * here is CSS pixels, and every test is arithmetic over plain data, so this
 * file belongs to the `node` project.
 */

/** A page of `documentHeight` seen through a `viewportHeight`-tall window. */
function page(documentHeight: number, viewportHeight: number): PageMetrics {
  return {
    documentHeight,
    viewportHeight,
    viewportWidth: 1000,
    devicePixelRatio: 1,
  }
}

/** More pixels than any page in this file needs, so nothing truncates. */
const NO_LIMIT = Number.MAX_SAFE_INTEGER

/** The page and budget P5 truncates, named so P6 can plan it too. */
const OVERSIZED_PAGE: PageMetrics = {
  documentHeight: 4000,
  viewportHeight: 1000,
  viewportWidth: 1000,
  devicePixelRatio: 2,
}
const OVERSIZED_BUDGET = 5_000_000

describe('planScroll', () => {
  it('splits a page that is a whole number of viewports tall into steps that do not overlap', () => {
    // P1. 3000 / 1000. Nothing here asserts `scrollY`: at a whole multiple the
    // last step lands on `documentHeight - viewportHeight` anyway, and P2 is
    // where the sequence of scroll positions is pinned down.
    const plan = planScroll(page(3000, 1000), NO_LIMIT)

    expect(plan.steps).toHaveLength(3)
    expect(plan.steps.map((step) => step.height)).toEqual([1000, 1000, 1000])
    expect(plan.steps.map((step) => step.destTop)).toEqual([0, 1000, 2000])
    expect(plan.compositeHeight).toBe(3000)
    expect(plan.compositeWidth).toBe(1000)
    expect(plan.truncated).toBe(false)
  })

  it('overlaps the last step and pastes the overlapping band only once', () => {
    // P2, the off-by-one this whole module exists for. The page cannot scroll
    // past 1500, so the third capture repeats the band from 1500 to 2000 that
    // the second one already carries; `sourceTop` is what drops it.
    const plan = planScroll(page(2500, 1000), NO_LIMIT)

    expect(plan.steps).toEqual([
      { index: 0, scrollY: 0, sourceTop: 0, height: 1000, destTop: 0 },
      { index: 1, scrollY: 1000, sourceTop: 0, height: 1000, destTop: 1000 },
      { index: 2, scrollY: 1500, sourceTop: 500, height: 500, destTop: 2000 },
    ])
    expect(plan.steps.reduce((sum, step) => sum + step.height, 0)).toBe(2500)
    expect(plan.compositeHeight).toBe(2500)
  })

  it('captures a page shorter than the viewport in one step with no blank band', () => {
    // P3. The window is 1000 tall but only 800 of it is page, and the 200 below
    // it is whatever the browser paints under a short document.
    const plan = planScroll(page(800, 1000), NO_LIMIT)

    expect(plan.steps).toEqual([{ index: 0, scrollY: 0, sourceTop: 0, height: 800, destTop: 0 }])
    expect(plan.compositeHeight).toBe(800)
    expect(plan.truncated).toBe(false)
  })

  it('invents no overlap when the page is exactly two viewports tall', () => {
    // P4. The mirror of P2: here the last capture starts exactly where the
    // previous one ended, so trimming its top would throw away real rows.
    const plan = planScroll(page(2000, 1000), NO_LIMIT)

    expect(plan.steps).toHaveLength(2)
    expect(plan.steps[1]).toEqual({
      index: 1,
      scrollY: 1000,
      sourceTop: 0,
      height: 1000,
      destTop: 1000,
    })
  })

  it('truncates a page whose composite would not fit the pixel budget', () => {
    // P5. 4000 CSS px at a device pixel ratio of 2 is 4000 x 1000 x 4 = 16M
    // device pixels; the budget is 5M, which pays for 1250 CSS px of height.
    const plan = planScroll(OVERSIZED_PAGE, OVERSIZED_BUDGET)

    expect(plan.truncated).toBe(true)
    expect(plan.compositeHeight).toBe(1250)
    const devicePixels =
      plan.compositeHeight *
      plan.compositeWidth *
      OVERSIZED_PAGE.devicePixelRatio *
      OVERSIZED_PAGE.devicePixelRatio
    expect(devicePixels).toBeLessThanOrEqual(OVERSIZED_BUDGET)
    // Two steps, not the four the untruncated page would need: no step is
    // produced for a band that will not be in the composite.
    expect(plan.steps).toHaveLength(2)
  })

  it('tiles the composite exactly once, with neither a gap nor an overlap', () => {
    // P6. The invariant the compositor depends on, asserted over every plan the
    // tests above build. A gap paints a blank band and an overlap paints a band
    // twice, and a step-by-step assertion that reads one step at a time sees
    // neither: only the sequence shows it.
    const plans: ScrollPlan[] = [
      planScroll(page(3000, 1000), NO_LIMIT),
      planScroll(page(2500, 1000), NO_LIMIT),
      planScroll(page(800, 1000), NO_LIMIT),
      planScroll(page(2000, 1000), NO_LIMIT),
      planScroll(OVERSIZED_PAGE, OVERSIZED_BUDGET),
    ]

    for (const plan of plans) {
      const starts = plan.steps.map((step) => step.destTop)
      const ends = plan.steps.map((step) => step.destTop + step.height)

      expect(plan.steps.length).toBeGreaterThan(0)
      // The composite starts at the first step and every step ends where the
      // next one starts.
      expect(starts).toEqual([0, ...ends.slice(0, -1)])
      // The last step ends on the last row of the composite, so the heights add
      // up to it exactly.
      expect(ends.at(-1)).toBe(plan.compositeHeight)
      expect(plan.steps.reduce((sum, step) => sum + step.height, 0)).toBe(plan.compositeHeight)
    }
  })

  it('never asks for a scroll position the page would refuse', () => {
    // P7. Below zero and past `documentHeight - viewportHeight` are both
    // positions the browser silently clamps, and a silently clamped scroll is a
    // layer captured at the wrong offset.
    const cases: PageMetrics[] = [
      page(3000, 1000),
      page(2500, 1000),
      page(800, 1000),
      page(2000, 1000),
    ]
    for (const metrics of cases) {
      const plan = planScroll(metrics, NO_LIMIT)
      const highest = Math.max(0, metrics.documentHeight - metrics.viewportHeight)
      for (const step of plan.steps) {
        expect(step.scrollY).toBeGreaterThanOrEqual(0)
        expect(step.scrollY).toBeLessThanOrEqual(highest)
      }
    }
  })
})
