import { describe, expect, it } from 'vitest'
import { clampRect, isUsable, normalizeRect, nudgeRect, resizeRect } from './selection'

const bounds = { x: 0, y: 0, width: 1000, height: 800 }

describe('normalizeRect', () => {
  it('builds a positive rect when dragging down-right', () => {
    expect(normalizeRect({ x: 10, y: 20 }, { x: 110, y: 70 })).toEqual({
      x: 10, y: 20, width: 100, height: 50,
    })
  })

  it('builds a positive rect when dragging up-left', () => {
    expect(normalizeRect({ x: 110, y: 70 }, { x: 10, y: 20 })).toEqual({
      x: 10, y: 20, width: 100, height: 50,
    })
  })
})

describe('clampRect', () => {
  it('trims a rect that runs past the right edge', () => {
    expect(clampRect({ x: 950, y: 10, width: 100, height: 50 }, bounds)).toEqual({
      x: 950, y: 10, width: 50, height: 50,
    })
  })

  it('trims a rect that starts before the origin', () => {
    expect(clampRect({ x: -20, y: -10, width: 100, height: 50 }, bounds)).toEqual({
      x: 0, y: 0, width: 80, height: 40,
    })
  })
})

describe('nudgeRect', () => {
  it('moves the rect by the delta', () => {
    expect(nudgeRect({ x: 10, y: 10, width: 50, height: 50 }, 5, -5, bounds)).toEqual({
      x: 15, y: 5, width: 50, height: 50,
    })
  })

  it('stops at the bounds instead of moving partially outside', () => {
    expect(nudgeRect({ x: 960, y: 10, width: 40, height: 40 }, 10, 0, bounds)).toEqual({
      x: 960, y: 10, width: 40, height: 40,
    })
  })
})

describe('resizeRect', () => {
  it('moves the east edge to the pointer', () => {
    const rect = { x: 100, y: 100, width: 100, height: 100 }
    expect(resizeRect(rect, 'e', { x: 250, y: 150 }, bounds)).toEqual({
      x: 100, y: 100, width: 150, height: 100,
    })
  })

  it('keeps the rect positive when the pointer crosses the opposite edge', () => {
    const rect = { x: 100, y: 100, width: 100, height: 100 }
    expect(resizeRect(rect, 'e', { x: 60, y: 150 }, bounds)).toEqual({
      x: 60, y: 100, width: 40, height: 100,
    })
  })

  it('keeps growing once the pointer has crossed the opposite edge', () => {
    // Two steps, not one. The caller resizes from the rect the handle was
    // pressed on, so the west edge stays at 100 for every event and the width
    // tracks how far past it the pointer has walked. A single-step assertion
    // cannot tell that apart from a rect that has stopped growing and started
    // sliding along with the pointer.
    const origin = { x: 100, y: 100, width: 100, height: 100 }
    expect(resizeRect(origin, 'e', { x: 80, y: 150 }, bounds)).toEqual({
      x: 80, y: 100, width: 20, height: 100,
    })
    expect(resizeRect(origin, 'e', { x: 70, y: 150 }, bounds)).toEqual({
      x: 70, y: 100, width: 30, height: 100,
    })
  })

  it('moves both edges of a corner handle', () => {
    // The only case that pins the `handle.includes` dispatch: `nw` has to move
    // the west edge and the north edge, and leave the other two alone.
    const rect = { x: 100, y: 100, width: 100, height: 100 }
    expect(resizeRect(rect, 'nw', { x: 60, y: 40 }, bounds)).toEqual({
      x: 60, y: 40, width: 140, height: 160,
    })
  })

  it('clamps a resize that leaves the display', () => {
    const rect = { x: 900, y: 100, width: 50, height: 50 }
    expect(resizeRect(rect, 'e', { x: 1200, y: 150 }, bounds)).toEqual({
      x: 900, y: 100, width: 100, height: 50,
    })
  })
})

describe('isUsable', () => {
  it('rejects an accidental click', () => {
    expect(isUsable({ x: 10, y: 10, width: 2, height: 2 })).toBe(false)
  })

  it('accepts a real selection', () => {
    expect(isUsable({ x: 10, y: 10, width: 40, height: 30 })).toBe(true)
  })
})
