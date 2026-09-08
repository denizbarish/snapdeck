import { describe, expect, it } from 'vitest'
import { toLocalRect, windowUnderPoint, type WindowBounds } from './snap'

const win = (id: number, x: number, y: number, w: number, h: number, layer = 0): WindowBounds => ({
  id,
  title: `w${id}`,
  appName: 'App',
  bounds: { x, y, width: w, height: h },
  layer,
})

describe('windowUnderPoint', () => {
  it('returns null when no window contains the point', () => {
    expect(windowUnderPoint([win(1, 0, 0, 10, 10)], { x: 500, y: 500 })).toBeNull()
  })

  it('returns the frontmost window when several overlap', () => {
    const windows = [win(1, 0, 0, 100, 100), win(2, 10, 10, 50, 50)]
    // The list is front-to-back, so the first match wins.
    expect(windowUnderPoint(windows, { x: 20, y: 20 })?.id).toBe(1)
  })

  it('ignores windows above the normal layer, such as the overlay itself', () => {
    const windows = [win(9, 0, 0, 100, 100, 25), win(1, 0, 0, 100, 100, 0)]
    expect(windowUnderPoint(windows, { x: 20, y: 20 })?.id).toBe(1)
  })

  // The desktop layer is the reason the filter has to match the normal layer
  // exactly. macOS keeps the wallpaper, the desktop icon layer and the
  // window-server backstop far below zero, and every one of them covers the
  // whole display, so a `<=` filter turns a hover over empty desktop into a
  // highlight of the entire screen and a click into a full-screen capture.
  it('ignores windows below the normal layer, such as the full-display wallpaper', () => {
    const wallpaper = win(85318, 0, 0, 1710, 1112, -2147483624)
    expect(windowUnderPoint([wallpaper], { x: 20, y: 20 })).toBeNull()
  })

  it('picks an ordinary window over a desktop window listed ahead of it', () => {
    // Desktop first, so only the layer test can save the ordinary window: the
    // front-to-back rule would hand back the wallpaper.
    const windows = [win(85318, 0, 0, 1710, 1112, -2147483624), win(1, 10, 10, 100, 100)]
    expect(windowUnderPoint(windows, { x: 20, y: 20 })?.id).toBe(1)
  })
})

describe('toLocalRect', () => {
  it('converts global window bounds to display-local coordinates', () => {
    expect(toLocalRect({ x: 1500, y: 100, width: 200, height: 100 }, { x: 1440, y: 0 })).toEqual({
      x: 60,
      y: 100,
      width: 200,
      height: 100,
    })
  })

  // A display placed to the left of, or above, the primary one has a negative
  // origin, so the rebase adds rather than subtracts. Getting the sign wrong
  // here draws the highlight off the far edge of the overlay.
  it('converts bounds on a display with a negative origin', () => {
    expect(toLocalRect({ x: -1820, y: -980, width: 200, height: 100 }, { x: -1920, y: -1080 })).toEqual(
      { x: 100, y: 100, width: 200, height: 100 },
    )
  })
})
