import { describe, expect, it } from 'vitest'

import {
  CAPTURE_INTERVAL_MS,
  createThrottle,
  MAX_CAPTURE_VISIBLE_TAB_CALLS_PER_SECOND,
  MS_PER_SECOND,
} from './throttle'

/**
 * Nothing in here waits on real time. The clock is a number the test moves and
 * `sleep` is what moves it, which is the only way an assertion about how long
 * something waited can be exact and still run in a millisecond.
 *
 * The `node` project, because a throttle is arithmetic over a clock.
 */

/** Deliberately not `CAPTURE_INTERVAL_MS`: the interval is an argument. */
const INTERVAL_MS = 100

type FakeClock = {
  now: () => number
  sleep: (ms: number) => Promise<void>
  /** Moves the clock without the throttle having asked to wait. */
  advance: (ms: number) => void
  /** Every wait the throttle asked for, in order. */
  sleeps: number[]
}

function fakeClock(): FakeClock {
  let time = 0
  const sleeps: number[] = []
  return {
    now: () => time,
    sleep: (ms: number) => {
      sleeps.push(ms)
      time += ms
      return Promise.resolve()
    },
    advance: (ms: number) => {
      time += ms
    },
    sleeps,
  }
}

describe('createThrottle', () => {
  it('runs the first call at once and spaces the ones behind it by the interval', async () => {
    // H1. Three jobs with nothing between them: the quota is what has to hold
    // them apart, and the total wait says by how much.
    const clock = fakeClock()
    const throttle = createThrottle(INTERVAL_MS, clock.now, clock.sleep)

    const startedAt: number[] = []
    for (let call = 0; call < 3; call += 1) {
      startedAt.push(await throttle(() => Promise.resolve(clock.now())))
    }

    expect(startedAt).toEqual([0, INTERVAL_MS, 2 * INTERVAL_MS])
    expect(clock.sleeps).toEqual([INTERVAL_MS, INTERVAL_MS])
    const slept = clock.sleeps.reduce((total, wait) => total + wait, 0)
    expect(slept).toBe(2 * INTERVAL_MS)
  })

  it('does not wait when the interval has already passed on its own', async () => {
    // H2. A capture loop scrolls and waits for lazy content between calls, and
    // that work is often longer than the quota window. Sleeping again on top of
    // it would pay the interval twice for no reason.
    const clock = fakeClock()
    const throttle = createThrottle(INTERVAL_MS, clock.now, clock.sleep)

    await throttle(() => Promise.resolve(undefined))
    clock.advance(INTERVAL_MS * 2)
    await throttle(() => Promise.resolve(undefined))

    expect(clock.sleeps).toEqual([])
  })

  it('leaves the shipped interval under the quota Chrome enforces', () => {
    // H3. The number the capture loop actually runs with. Chrome refuses the
    // call rather than queueing it, so a rate at or above the limit is a
    // capture that fails halfway down a page.
    expect(MS_PER_SECOND / CAPTURE_INTERVAL_MS).toBeLessThan(
      MAX_CAPTURE_VISIBLE_TAB_CALLS_PER_SECOND,
    )
  })
})
