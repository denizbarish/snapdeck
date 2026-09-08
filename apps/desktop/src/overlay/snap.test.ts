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
})
