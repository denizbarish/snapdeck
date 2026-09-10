import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { PageMetrics } from '../content/measure'
import { planScroll } from '../content/plan'
import { captureFullPage, type CaptureDeps, type FullPageCapture } from './capture'

/**
 * The `node` project. What the capture loop decides is an order of calls, and
 * an order of calls is visible in a log; the pixels those calls produce are
 * `composite`'s claim and are tested against a real canvas there.
 *
 * `composite` is mocked for the same reason: an `OffscreenCanvas` is the one
 * thing in the loop Node does not have, and stubbing the global would put a
 * fake canvas in the place the browser project exists to keep real.
 */
vi.mock('./composite', () => ({
  compositeToPng: (): Promise<Blob> =>
    Promise.resolve(new Blob([new Uint8Array([0])], { type: 'image/png' })),
}))

/** More pixels than any page in this file needs, so nothing truncates. */
const NO_LIMIT = Number.MAX_SAFE_INTEGER

/**
 * The throttle inside the loop sleeps for real milliseconds, so the timers are
 * faked and driven by hand. Without this every test here would wait out
 * Chrome's quota once per step.
 */
beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

function metricsOf(
  documentHeight: number,
  viewportHeight: number,
  viewportWidth = 1000,
  devicePixelRatio = 1,
): PageMetrics {
  return { documentHeight, viewportHeight, viewportWidth, devicePixelRatio }
}

/** Stands in for a captured viewport: the loop only ever hands it on. */
const FAKE_BITMAP = { width: 1, height: 1 } as unknown as ImageBitmap

type Recorded = { deps: CaptureDeps; calls: string[] }

/**
 * Records every call in order. `failAtCapture` is 1-based and makes that
 * capture throw, which is the only way C2's failure path is reachable.
 */
function record(metrics: PageMetrics, failAtCapture?: number): Recorded {
  const calls: string[] = []
  let captures = 0
  const deps: CaptureDeps = {
    measure: () => {
      calls.push('measure')
      return Promise.resolve(metrics)
    },
    setPinnedHidden: (hidden: boolean) => {
      calls.push(`setPinnedHidden(${String(hidden)})`)
      return Promise.resolve()
    },
    scrollTo: (y: number) => {
      calls.push(`scrollTo(${String(y)})`)
      return Promise.resolve()
    },
    settle: () => {
      calls.push('settle')
      return Promise.resolve()
    },
    captureVisible: () => {
      captures += 1
      calls.push('captureVisible')
      if (captures === failAtCapture) {
        return Promise.reject(new Error('the tab refused the capture'))
      }
      return Promise.resolve(FAKE_BITMAP)
    },
    restoreScroll: () => {
      calls.push('restoreScroll')
      return Promise.resolve()
    },
  }
  return { deps, calls }
}

/** Runs the loop to completion with the throttle's sleeps fast-forwarded. */
async function run(deps: CaptureDeps, maxPixels: number): Promise<FullPageCapture | Error> {
  const finished = captureFullPage(deps, maxPixels).catch((error: unknown) =>
    error instanceof Error ? error : new Error(String(error)),
  )
  await vi.runAllTimersAsync()
  return finished
}

describe('captureFullPage', () => {
  it('hides the pinned elements only after the first layer is captured', async () => {
    // C1. The page's own header is pinned, and hiding it before the first
    // capture would leave it out of every layer, so the composite would be a
    // page nobody has ever seen. Hiding it after the first is what puts it in
    // exactly once.
    const { deps, calls } = record(metricsOf(2500, 1000))

    await run(deps, NO_LIMIT)

    expect(calls.slice(0, 5)).toEqual([
      'measure',
      'scrollTo(0)',
      'settle',
      'captureVisible',
      'setPinnedHidden(true)',
    ])
  })

  it('restores the page even when a capture fails halfway down it', async () => {
    // C2. A failed capture is the user's problem for a second; a page left
    // scrolled with its header invisible is the user's problem until they
    // reload it.
    const { deps, calls } = record(metricsOf(2500, 1000), 3)

    const outcome = await run(deps, NO_LIMIT)

    expect(outcome).toBeInstanceOf(Error)
    expect(calls).toContain('setPinnedHidden(false)')
    expect(calls).toContain('restoreScroll')
    expect(calls.at(-1)).toBe('restoreScroll')
  })

  it('waits for the page to settle before every capture', async () => {
    // C3. Lazy-loaded images arrive after the scroll, not with it, and a
    // capture taken before they land is a band of placeholders in the middle of
    // the composite.
    const { deps, calls } = record(metricsOf(2500, 1000))

    await run(deps, NO_LIMIT)

    const steps = planScroll(metricsOf(2500, 1000), NO_LIMIT).steps.length
    expect(calls.filter((call) => call === 'settle')).toHaveLength(steps)
    expect(calls.filter((call) => call === 'captureVisible')).toHaveLength(steps)
    const unsettled = calls.filter(
      (call, index) => call === 'captureVisible' && calls[index - 1] !== 'settle',
    )
    expect(unsettled).toEqual([])
  })

  it('scrolls to the position each step names', async () => {
    // C4. The last step of a page that does not divide evenly stops short of
    // where counting viewports would put it, because the page has no more room
    // to scroll. Asking for the position the plan did not name captures the
    // wrong band and the composite repeats it.
    const metrics = metricsOf(2500, 1000)
    const { deps, calls } = record(metrics)

    await run(deps, NO_LIMIT)

    const scrolls = calls
      .filter((call) => call.startsWith('scrollTo('))
      .map((call) => Number(call.slice('scrollTo('.length, -1)))
    expect(scrolls).toEqual(planScroll(metrics, NO_LIMIT).steps.map((step) => step.scrollY))
    expect(scrolls).toEqual([0, 1000, 1500])
  })

  it('hands the caller the plan and the metrics it worked from', async () => {
    // C5. `truncated` and the composite's size go out over the bridge, and the
    // layer that sends them has no way of knowing them except from here.
    const metrics = metricsOf(4000, 1000, 1000, 2)
    const budget = 5_000_000
    const { deps } = record(metrics)

    const outcome = await run(deps, budget)

    if (outcome instanceof Error) {
      throw outcome
    }
    expect(outcome.metrics).toEqual(metrics)
    expect(outcome.plan).toEqual(planScroll(metrics, budget))
    expect(outcome.plan.truncated).toBe(true)
  })
})
