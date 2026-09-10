import { measurePage, type PageMetrics } from './measure'
import { planScroll, type ScrollPlan } from './plan'
import { findPinned, hidePinned, restorePinned, type PinnedElement } from './sticky'

/**
 * Content script shell. Every decision it makes belongs to `measure`, `plan` or
 * `sticky`; what is left here is the wiring to `chrome.runtime` and the one
 * thing a pure module cannot hold, which is the list of elements hidden between
 * `prepare` and `restore`.
 *
 * This file is injected on demand by `chrome.scripting`, never declared in the
 * manifest, so nothing runs on a page the user did not point at. It imports
 * nothing from `@snapdeck/protocol` and must keep it that way: the bridge is
 * the service worker's business, and the less this file carries the less rides
 * along into someone's page. `maxPixels` arrives in the message for that
 * reason: the budget is the protocol's number, and the worker knows it.
 */

export type ContentRequest =
  | { command: 'measure'; maxPixels: number }
  | { command: 'prepare' }
  | { command: 'scrollTo'; scrollY: number }
  | { command: 'restore' }

export type ContentResponse =
  | { command: 'measure'; metrics: PageMetrics; plan: ScrollPlan }
  | { command: 'prepare'; pinned: number }
  | { command: 'scrollTo'; scrollY: number }
  | { command: 'restore'; restored: number }

/** Hidden by `prepare`, and the only thing this file remembers. */
let hidden: PinnedElement[] = []

function handle(request: ContentRequest): ContentResponse {
  switch (request.command) {
    case 'measure': {
      const metrics = measurePage(window)
      return { command: 'measure', metrics, plan: planScroll(metrics, request.maxPixels) }
    }
    case 'prepare': {
      // Restoring first keeps a second `prepare` from recording an already
      // hidden element as one that started out hidden.
      restorePinned(hidden)
      hidden = hidePinned(findPinned(document))
      return { command: 'prepare', pinned: hidden.length }
    }
    case 'scrollTo': {
      window.scrollTo(0, request.scrollY)
      // The browser clamps a scroll it cannot honour, and the caller needs to
      // know where the page actually came to rest.
      return { command: 'scrollTo', scrollY: window.scrollY }
    }
    case 'restore': {
      const restored = hidden.length
      restorePinned(hidden)
      hidden = []
      return { command: 'restore', restored }
    }
  }
}

chrome.runtime.onMessage.addListener(
  (request: ContentRequest, _sender, sendResponse: (response: ContentResponse) => void) => {
    sendResponse(handle(request))
    return false
  },
)
