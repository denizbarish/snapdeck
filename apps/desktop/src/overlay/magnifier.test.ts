import { describe, expect, it } from 'vitest'
import { magnifierSourceRect, samplePixel, toHex } from './magnifier'

// 2x1 RGBA image: red pixel, then green pixel.
const data = new Uint8ClampedArray([255, 0, 0, 255, 0, 128, 0, 255])

// 2x2 RGBA image, one distinct channel value per pixel, so a sample that lands
// on the wrong row or the wrong column names a different colour rather than
// coincidentally the right one.
//
//   (0,0) #0A0A0A   (1,0) #141414
//   (0,1) #1E1E1E   (1,1) #282828
const grid = new Uint8ClampedArray([
  10, 10, 10, 255, 20, 20, 20, 255, 30, 30, 30, 255, 40, 40, 40, 255,
])

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

  // Without the `x >= width` guard this offset is still inside the buffer: it
  // is the first pixel of the next row. A colour from somewhere else on the
  // screen, reported with no sign that anything went wrong.
  it('returns null past the right edge instead of wrapping to the next row', () => {
    expect(samplePixel(grid, 2, { x: 2, y: 0 })).toBeNull()
  })

  // The bottom edge has no width to check against, so only the buffer-length
  // test catches it. Without that test the four channel reads are all
  // `undefined`, and because they are typed `number` the malformed object
  // reaches `toHex`, which throws and takes the overlay down with it.
  it('returns null past the last row', () => {
    expect(samplePixel(grid, 2, { x: 0, y: 2 })).toBeNull()
  })

  // Pointer coordinates are fractional in practice, and this is the rounding
  // rule the whole feature rests on: the pixel the user is pointing at is the
  // one their cursor is inside, so both axes truncate. Rounding or ceiling
  // 1.9 and 0.9 would ask for (2, 1), which is off the right edge of the row.
  it('truncates a fractional point onto the pixel it falls inside', () => {
    expect(samplePixel(grid, 2, { x: 1.9, y: 0.9 })).toEqual({ r: 20, g: 20, b: 20, a: 255 })
  })
})

describe('toHex', () => {
  it('formats an uppercase six-digit hex string', () => {
    expect(toHex({ r: 0, g: 128, b: 0, a: 255 })).toBe('#008000')
  })

  it('pads single-digit channels', () => {
    expect(toHex({ r: 1, g: 2, b: 3, a: 255 })).toBe('#010203')
  })

  // The only case whose expected value contains a hex letter, and therefore the
  // only one that can tell `abcdef` from `ABCDEF`.
  it('uppercases the hex letters', () => {
    expect(toHex({ r: 171, g: 205, b: 239, a: 255 })).toBe('#ABCDEF')
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

  // Every other case uses an origin of zero, which makes `bounds.x` and
  // `bounds.y` indistinguishable from the constant 0. The function is
  // documented as working in whichever space it is handed, and window mode
  // already computes in a global space where a second display starts at
  // x = 1440, so the origin has to be pinned rather than assumed.
  it('clamps against a bounds origin that is not zero', () => {
    const offset = { x: 1440, y: 200, width: 100, height: 100 }
    expect(magnifierSourceRect({ x: 1441, y: 201 }, 10, offset)).toEqual({
      x: 1440,
      y: 200,
      width: 10,
      height: 10,
    })
  })
})
