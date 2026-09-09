import { describe, expect, it } from 'vitest'
import { handleAtPoint, layerAtPoint, moveLayer, resizeLayer } from './hit'
import type { Layer } from './model'

const stroke = { color: '#ff0000', width: 4 }
const badge = { fill: '#ff0000', color: '#ffffff', size: 28 }
const textStyle = { color: '#000000', size: 16, family: 'Inter' }

function filledRect(id: string, x: number, y: number): Layer {
  return {
    id,
    kind: 'rect',
    rect: { x, y, width: 100, height: 100 },
    style: { stroke, fill: '#00ff00' },
  }
}

describe('layerAtPoint', () => {
  // Layers paint back to front, so the one the user sees under the pointer in
  // an overlap is the last of the array, not the first one that happens to
  // contain the point.
  it('returns the topmost of two overlapping layers', () => {
    const bottom = filledRect('bottom', 100, 100)
    const top = filledRect('top', 150, 150)
    expect(layerAtPoint([bottom, top], { x: 175, y: 175 }, 4)?.id).toBe('top')
  })

  it('finds an arrow by its body within the tolerance, and not far from it', () => {
    const arrow: Layer = {
      id: 'a', kind: 'arrow', from: { x: 100, y: 100 }, to: { x: 200, y: 100 }, style: stroke,
    }
    expect(layerAtPoint([arrow], { x: 150, y: 104 }, 6)?.id).toBe('a')
    expect(layerAtPoint([arrow], { x: 150, y: 140 }, 6)).toBeNull()
  })

  // An unfilled shape is a frame, not a surface. Hit testing it by its
  // bounding box would make it swallow every click on whatever sits inside it.
  it('finds an unfilled rectangle by its edge but not through its interior', () => {
    const outline: Layer = {
      id: 'a',
      kind: 'rect',
      rect: { x: 100, y: 100, width: 100, height: 100 },
      style: { stroke, fill: null },
    }
    expect(layerAtPoint([outline], { x: 150, y: 150 }, 4)).toBeNull()
    expect(layerAtPoint([outline], { x: 100, y: 150 }, 4)?.id).toBe('a')
  })

  it('finds a filled rectangle through its interior', () => {
    expect(layerAtPoint([filledRect('a', 100, 100)], { x: 150, y: 150 }, 4)?.id).toBe('a')
  })
})

describe('handleAtPoint', () => {
  // Bounds are x 100..300, y 100..200, so the eight handles sit at the corners
  // and the edge midpoints of that box.
  const layer: Layer = {
    id: 'a',
    kind: 'rect',
    rect: { x: 100, y: 100, width: 200, height: 100 },
    style: { stroke, fill: null },
  }

  it('finds each of the eight handles at its own position', () => {
    const positions = [
      ['nw', { x: 100, y: 100 }],
      ['n', { x: 200, y: 100 }],
      ['ne', { x: 300, y: 100 }],
      ['e', { x: 300, y: 150 }],
      ['se', { x: 300, y: 200 }],
      ['s', { x: 200, y: 200 }],
      ['sw', { x: 100, y: 200 }],
      ['w', { x: 100, y: 150 }],
    ] as const
    for (const [handle, point] of positions) {
      expect(handleAtPoint(layer, point, 10)).toBe(handle)
    }
  })

  it('returns null in the middle of the layer', () => {
    expect(handleAtPoint(layer, { x: 200, y: 150 }, 10)).toBeNull()
  })
})

describe('moveLayer', () => {
  it('shifts every layer kind and keeps its kind', () => {
    const rect = { x: 10, y: 20, width: 30, height: 40 }
    const cases: Layer[] = [
      { id: 'a', kind: 'arrow', from: { x: 10, y: 20 }, to: { x: 30, y: 40 }, style: stroke },
      { id: 'b', kind: 'rect', rect, style: { stroke, fill: null } },
      { id: 'c', kind: 'ellipse', rect, style: { stroke, fill: '#00ff00' } },
      { id: 'd', kind: 'line', points: [{ x: 1, y: 2 }, { x: 3, y: 4 }], style: stroke },
      { id: 'e', kind: 'text', rect, content: 'hi', style: textStyle },
      { id: 'f', kind: 'highlight', rect, color: '#ffff00' },
      { id: 'g', kind: 'obscure', rect, mode: 'blur', intensity: 8 },
      { id: 'h', kind: 'step', center: { x: 50, y: 60 }, index: 1, style: badge },
    ]

    const moved = cases.map((layer) => moveLayer(layer, 5, -7))

    expect(moved.map((layer) => layer.kind)).toEqual(cases.map((layer) => layer.kind))
    expect(moved[0]).toEqual({
      id: 'a', kind: 'arrow', from: { x: 15, y: 13 }, to: { x: 35, y: 33 }, style: stroke,
    })
    // Every point of a freehand line, not just its first: a line whose box is
    // right but whose samples are stale draws itself back where it started.
    expect(moved[3]).toEqual({
      id: 'd', kind: 'line', points: [{ x: 6, y: -5 }, { x: 8, y: -3 }], style: stroke,
    })
    // A step badge is anchored by its centre, so that is what has to move.
    expect(moved[7]).toEqual({
      id: 'h', kind: 'step', center: { x: 55, y: 53 }, index: 1, style: badge,
    })
    for (const layer of [moved[1], moved[2], moved[4], moved[5], moved[6]]) {
      expect(layer && 'rect' in layer ? layer.rect : null).toEqual({
        x: 15, y: 13, width: 30, height: 40,
      })
    }
    // The originals are untouched: history stores the layer it was handed, and
    // an in-place shift would rewrite the past as well as the present.
    expect(cases[1]).toEqual({ id: 'b', kind: 'rect', rect, style: { stroke, fill: null } })
    expect(rect).toEqual({ x: 10, y: 20, width: 30, height: 40 })
  })
})

describe('resizeLayer', () => {
  it('keeps growing from the fixed origin once the pointer crosses the opposite edge', () => {
    // Two steps, not one. The anchor is the west edge of `origin`, so it stays
    // at 100 for every event of the drag and the width tracks how far past it
    // the pointer has walked. Deriving the anchor from the layer being resized
    // passes a single-step assertion and then slides the shape along with the
    // pointer instead of growing it.
    const origin: Layer = {
      id: 'a',
      kind: 'rect',
      rect: { x: 100, y: 100, width: 100, height: 100 },
      style: { stroke, fill: null },
    }

    const first = resizeLayer(origin, 'e', { x: 80, y: 150 }, origin)
    expect(first).toEqual({ ...origin, rect: { x: 80, y: 100, width: 20, height: 100 } })

    // The live layer is fed back in, as the editor does on every pointer move.
    const second = resizeLayer(first, 'e', { x: 70, y: 150 }, origin)
    expect(second).toEqual({ ...origin, rect: { x: 70, y: 100, width: 30, height: 100 } })

    expect(origin.kind === 'rect' && origin.rect).toEqual({
      x: 100, y: 100, width: 100, height: 100,
    })
  })
})
