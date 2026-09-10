/**
 * Elements that stay on screen while the page scrolls under them. Left alone
 * they print into every layer of a full-page capture, so the capture hides them
 * for the duration and puts them back exactly as it found them.
 */

export type PinnedElement = { element: HTMLElement; previousVisibility: string }

/** The two used `position` values that keep an element in view while scrolling. */
const PINNED_POSITIONS: readonly string[] = ['fixed', 'sticky']

/** What a hidden element is set to. See `hidePinned` for why it is not `none`. */
const HIDDEN_VISIBILITY = 'hidden'

export function findPinned(root: Document): HTMLElement[] {
  const view = root.defaultView
  if (!view) return []

  const pinned: HTMLElement[] = []
  for (const element of root.querySelectorAll<HTMLElement>('*')) {
    if (PINNED_POSITIONS.includes(view.getComputedStyle(element).position)) pinned.push(element)
  }
  return pinned
}

/**
 * Hides each element and reports what it was, so the page can be handed back.
 *
 * `visibility: hidden` and not `display: none`: a sticky element takes up room
 * in the flow, and taking that room away pulls everything below it upwards. The
 * layers captured after that would be of a differently laid-out page, and they
 * would not line up with the ones captured before it.
 */
export function hidePinned(elements: HTMLElement[]): PinnedElement[] {
  return elements.map((element) => {
    const previousVisibility = element.style.visibility
    element.style.visibility = HIDDEN_VISIBILITY
    return { element, previousVisibility }
  })
}

/**
 * Puts back each element's own inline `visibility`, the empty string included:
 * writing `visible` over an element that had no inline value would override
 * whatever the page's stylesheet said and leave the user's page changed.
 */
export function restorePinned(hidden: PinnedElement[]): void {
  for (const { element, previousVisibility } of hidden) {
    element.style.visibility = previousVisibility
  }
}
