import { describe, expect, it } from 'vitest'
import { magnifierSourceRect, samplePixel, toHex } from './magnifier'

// 2x1 RGBA image: red pixel, then green pixel.
const data = new Uint8ClampedArray([255, 0, 0, 255, 0, 128, 0, 255])

describe('samplePixel', () => {
  it('reads the pixel at the given point', () => {
    expect(samplePixel(data, 2, { x: 1, y: 0 })).toEqual({ r: 0, g: 128, b: 0, a: 255 })
  })

  it('returns null outside the buffer', () => {
    expect(samplePixel(data, 2, { x: 5, y: 0 })).toBeNull()
  })

  it('returns null for negative coordinates', () => {
    expect(samplePixel(data, 2, { x: -1, y: 0 })).toBeNull()
  })
})

describe('toHex', () => {
  it('formats an uppercase six-digit hex string', () => {
    expect(toHex({ r: 0, g: 128, b: 0, a: 255 })).toBe('#008000')
  })

  it('pads single-digit channels', () => {
    expect(toHex({ r: 1, g: 2, b: 3, a: 255 })).toBe('#010203')
  })
})

describe('magnifierSourceRect', () => {
  const bounds = { x: 0, y: 0, width: 100, height: 100 }

  it('centers the rect on the point', () => {
    expect(magnifierSourceRect({ x: 50, y: 50 }, 10, bounds)).toEqual({
      x: 45,
      y: 45,
      width: 10,
      height: 10,
    })
  })

  it('clamps at the top-left corner', () => {
    expect(magnifierSourceRect({ x: 1, y: 1 }, 10, bounds)).toEqual({
      x: 0,
      y: 0,
      width: 10,
      height: 10,
    })
  })

  it('clamps at the bottom-right corner', () => {
    expect(magnifierSourceRect({ x: 99, y: 99 }, 10, bounds)).toEqual({
      x: 90,
      y: 90,
      width: 10,
      height: 10,
    })
  })
})
