/**
 * The one rule the capture loop has to obey that has nothing to do with pages:
 * Chrome's quota on `captureVisibleTab`.
 *
 * The clock and the wait are arguments rather than `Date.now` and `setTimeout`
 * for the reason `measurePage` takes a structural window: what this module
 * decides is arithmetic over elapsed time, and a test that has to wait out the
 * interval to check the interval is a test nobody runs.
 */

/**
 * Chrome refuses more than MAX_CAPTURE_VISIBLE_TAB_CALLS_PER_SECOND (2) calls
 * to captureVisibleTab in any second, and the refusal is an error rather than
 * a queue. 550 rather than 500 because the quota is measured against a moving
 * window and a call landing exactly on the boundary is the one that fails.
 */
export const CAPTURE_INTERVAL_MS = 550

/** The quota itself, as Chrome documents it. */
export const MAX_CAPTURE_VISIBLE_TAB_CALLS_PER_SECOND = 2

/** The window the quota is counted over. */
export const MS_PER_SECOND = 1000

export function createThrottle(
  intervalMs: number,
  now: () => number,
  sleep: (ms: number) => Promise<void>,
): <T>(work: () => Promise<T>) => Promise<T> {
  // Before the first call there is no slot to wait for, and a plain zero would
  // be a real point in time on a clock that starts below it.
  let nextSlotAt = Number.NEGATIVE_INFINITY

  return async <T>(work: () => Promise<T>): Promise<T> => {
    // The slot is claimed before the wait, not after it: two callers that
    // arrive together then queue behind each other instead of both reading the
    // same free slot and running at once.
    const slotAt = Math.max(now(), nextSlotAt)
    nextSlotAt = slotAt + intervalMs

    // Whatever the caller did between calls counts towards the interval, so a
    // loop that scrolls and waits for lazy content pays nothing here.
    const wait = slotAt - now()
    if (wait > 0) {
      await sleep(wait)
    }
    return work()
  }
}
