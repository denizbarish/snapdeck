import { describe, expect, it } from 'vitest'
import {
  fitViewport,
  toDocumentLength,
  toDocumentPoint,
  toLocalPoint,
  toLocalRect,
} from './viewport'

const image = { x: 0, y: 0, width: 800, height: 600 }

describe('fitViewport', () => {
  it('scales by the tighter of the two axes and centres what is left over', () => {
    // 400/800 is tighter than 600/600, so the width decides and the spare
    // height is split above and below.
    const viewport = fitViewport(image, 400, 600)
    expect(viewport.scale).toBe(0.5)
    expect(viewport.offsetX).toBe(0)
    expect(viewport.offsetY).toBe(150)
  })

  // A screenshot blown up past its own resolution is a blurred lie about what
  // will be exported: the export is always at source resolution, so the
  // preview must not claim detail the file cannot carry.
  it('never enlarges, however much room there is', () => {
    const viewport = fitViewport(image, 4000, 3000)
    expect(viewport.scale).toBe(1)
    expect(viewport.offsetX).toBe(1600)
    expect(viewport.offsetY).toBe(1200)
  })

  // The first render happens before the ResizeObserver has reported anything.
  // A scale of Infinity or NaN reaches `setTransform` and blanks the canvas.
  it('falls back to 1:1 for a box that has not been measured', () => {
    expect(fitViewport(image, 0, 0)).toEqual({ scale: 1, offsetX: 0, offsetY: 0 })
  })

  it('falls back to 1:1 for a view with no area', () => {
    expect(fitViewport({ x: 0, y: 0, width: 0, height: 0 }, 500, 500).scale).toBe(1)
  })

  // Centring a view wider than its box would push its left half off the edge,
  // where the pointer cannot reach it.
  it('pins a view larger than its box to the top-left instead of centring it', () => {
    const viewport = fitViewport({ x: 0, y: 0, width: 800, height: 600 }, 100, 600)
    expect(viewport.offsetX).toBe(0)
    expect(viewport.offsetY).toBeGreaterThanOrEqual(0)
  })
})

describe('toDocumentPoint', () => {
  // Measured from the canvas's own top-left, not the stage's: the canvas
  // element is exactly the view, which is what lets a crop show as a crop
  // without a clip that `putImageData` would ignore.
  it('undoes the scale', () => {
    const viewport = fitViewport(image, 400, 600)
    expect(viewport.scale).toBe(0.5)
    expect(toDocumentPoint({ x: 0, y: 0 }, image, viewport)).toEqual({ x: 0, y: 0 })
    expect(toDocumentPoint({ x: 200, y: 150 }, image, viewport)).toEqual({ x: 400, y: 300 })
  })

  // Layers keep their source coordinates whether or not the document is
  // cropped, so a pointer over a cropped view has to arrive in the same space
  // or every annotation would be placed by the width of the crop's origin.
  it('adds the crop origin, so a pointer lands where the layers live', () => {
    const crop = { x: 100, y: 50, width: 400, height: 300 }
    const viewport = fitViewport(crop, 400, 300)
    expect(viewport.scale).toBe(1)
    expect(toDocumentPoint({ x: 10, y: 10 }, crop, viewport)).toEqual({ x: 110, y: 60 })
  })

  it('is the inverse of toLocalPoint', () => {
    const crop = { x: 37, y: 11, width: 500, height: 400 }
    const viewport = fitViewport(crop, 250, 400)
    const point = { x: 300, y: 200 }
    const round = toDocumentPoint(toLocalPoint(point, crop, viewport), crop, viewport)
    expect(round.x).toBeCloseTo(point.x, 10)
    expect(round.y).toBeCloseTo(point.y, 10)
  })
})

describe('toLocalPoint', () => {
  // The chrome is positioned with this, over a canvas that is exactly the
  // view, so the top-left of the view is the top-left of the canvas whatever
  // letterboxing the stage has around it.
  it('measures from the canvas corner, whatever the stage put around it', () => {
    const tall = fitViewport(image, 400, 600)
    expect(tall.offsetY).toBe(150)
    expect(toLocalPoint({ x: 0, y: 0 }, image, tall)).toEqual({ x: 0, y: 0 })
    expect(toLocalPoint({ x: 800, y: 600 }, image, tall)).toEqual({ x: 400, y: 300 })
  })

  // A crop moves the origin of the view, and the chrome has to follow it or
  // every outline would be drawn a crop's width away from its layer.
  it('subtracts the crop origin', () => {
    const crop = { x: 200, y: 150, width: 400, height: 300 }
    const viewport = fitViewport(crop, 400, 300)
    expect(viewport.scale).toBe(1)
    expect(toLocalPoint({ x: 250, y: 200 }, crop, viewport)).toEqual({ x: 50, y: 50 })
  })
})

describe('toLocalRect', () => {
  it('maps the origin and scales the size', () => {
    const crop = { x: 100, y: 100, width: 400, height: 400 }
    const viewport = fitViewport(crop, 200, 200)
    expect(toLocalRect({ x: 150, y: 200, width: 100, height: 50 }, crop, viewport)).toEqual({
      x: 25,
      y: 50,
      width: 50,
      height: 25,
    })
  })

  // Scaled rather than derived from a second converted corner: a rect that
  // arrived negative should stay negative rather than be silently normalised
  // by the conversion, because whoever produced it is the one to fix.
  it('keeps a negative size negative', () => {
    const viewport = fitViewport(image, 800, 600)
    expect(toLocalRect({ x: 10, y: 10, width: -20, height: -30 }, image, viewport).width).toBe(-20)
  })
})

describe('toDocumentLength', () => {
  // Hit tolerances and handle sizes are chosen in screen pixels, where the
  // cursor is, but `hit.ts` measures in document space. Converting is what
  // keeps a target the same physical size at any zoom.
  it('grows a screen length as the view shrinks', () => {
    expect(toDocumentLength(9, fitViewport(image, 400, 300))).toBe(18)
    expect(toDocumentLength(9, fitViewport(image, 800, 600))).toBe(9)
  })
})
