import { afterEach, describe, expect, it } from 'vitest'

import { findPinned, hidePinned, restorePinned } from './sticky'

/**
 * These belong to the `browser` project. `findPinned` asks the engine what an
 * element's used `position` is, and R2's whole claim is about what the engine
 * does to a page's layout when an element is taken out of it. A simulated DOM
 * would answer both from a lookup table of the values the test itself wrote,
 * which proves nothing about the page this code will run on.
 */

let container: HTMLDivElement | undefined

afterEach(() => {
  container?.remove()
  container = undefined
})

/** Puts `html` in a fresh container attached to the real document. */
function mount(html: string): HTMLDivElement {
  const mounted = document.createElement('div')
  mounted.innerHTML = html
  document.body.append(mounted)
  container = mounted
  return mounted
}

/** The pinned elements inside `root`, so the runner's own DOM cannot join in. */
function pinnedInside(root: HTMLElement): HTMLElement[] {
  return findPinned(document).filter((element) => root.contains(element))
}

describe('findPinned', () => {
  it('finds the fixed and sticky elements and leaves every other element alone', () => {
    // R1. `fixed` and `sticky` are the two that stay on screen while the page
    // scrolls under them, which is what would print them into every layer.
    const root = mount(`
      <div id="fixed" style="position: fixed">fixed</div>
      <div id="sticky" style="position: sticky; top: 0">sticky</div>
      <div id="static" style="position: static">static</div>
      <div id="relative" style="position: relative">relative</div>
      <div id="absolute" style="position: absolute">absolute</div>
    `)

    const found = pinnedInside(root)

    expect(found.map((element) => element.id)).toEqual(['fixed', 'sticky'])
  })
})

describe('hidePinned', () => {
  it('leaves the content below a pinned element exactly where it was', () => {
    // R2. The element is `sticky` on purpose: a sticky element takes up room in
    // the flow, so `display: none` would pull the paragraph below it upwards and
    // every layer after this one would be captured against a different layout.
    // `visibility: hidden` keeps the room and drops only the paint.
    const root = mount(`
      <div id="sticky" style="position: sticky; top: 0; height: 50px">sticky</div>
      <p id="below">below</p>
    `)
    const below = root.querySelector<HTMLElement>('#below')
    expect(below).not.toBeNull()
    const topBefore = below?.getBoundingClientRect().top

    hidePinned(pinnedInside(root))

    expect(below?.getBoundingClientRect().top).toBe(topBefore)
  })
})

describe('restorePinned', () => {
  it('puts back each element inline visibility, empty string included', () => {
    // R3. The page belongs to the user, not to the capture. An element that
    // carried no inline `visibility` has to end with none, and writing
    // `visible` over it would override a stylesheet that meant to hide it.
    const root = mount(`
      <div id="was-hidden" style="visibility: hidden">was hidden</div>
      <div id="had-none">had none</div>
    `)
    const wasHidden = root.querySelector<HTMLElement>('#was-hidden')
    const hadNone = root.querySelector<HTMLElement>('#had-none')
    expect(wasHidden).not.toBeNull()
    expect(hadNone).not.toBeNull()
    if (!wasHidden || !hadNone) return

    const hidden = hidePinned([wasHidden, hadNone])
    expect(wasHidden.style.visibility).toBe('hidden')
    expect(hadNone.style.visibility).toBe('hidden')

    restorePinned(hidden)

    expect(wasHidden.style.visibility).toBe('hidden')
    expect(hadNone.style.visibility).toBe('')
  })
})
