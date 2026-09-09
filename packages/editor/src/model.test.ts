import { describe, expect, it } from 'vitest'
import { boundsOf, createDocument, nextStepIndex, type Layer } from './model'

const stroke = { color: '#ff0000', width: 4 }

function stepLayer(id: string, index: number): Layer {
  return {
    id,
    kind: 'step',
    center: { x: 100, y: 100 },
    index,
    style: { fill: '#ff0000', color: '#ffffff', size: 28 },
  }
}

describe('createDocument', () => {
  it('starts uncropped and empty', () => {
    const doc = createDocument(1920, 1080)
    expect(doc).toEqual({ width: 1920, height: 1080, crop: null, layers: [] })
  })
})

describe('nextStepIndex', () => {
  it('returns 1 for a document with no steps', () => {
    expect(nextStepIndex(createDocument(100, 100))).toBe(1)
  })

  // The largest index plus one, not the count plus one. Deleting step 2 out of
  // three leaves 1 and 3 behind, and the next badge has to be 4: reusing 3
  // would put two identically numbered badges on the same screenshot.
  it('returns the largest existing index plus one, not the count plus one', () => {
    const doc = createDocument(100, 100)
    const withSteps = { ...doc, layers: [stepLayer('a', 1), stepLayer('b', 3)] }
    expect(nextStepIndex(withSteps)).toBe(4)
  })
})

describe('boundsOf', () => {
  // An arrow is stored as the two points the user dragged between, in drag
  // order, so half of them run right-to-left or bottom-to-top. The bounding
  // box has to come out positive either way.
  it('returns the same positive box for an arrow whichever way it points', () => {
    const expected = { x: 10, y: 20, width: 90, height: 50 }
    const forwards: Layer = {
      id: 'a', kind: 'arrow', from: { x: 10, y: 20 }, to: { x: 100, y: 70 }, style: stroke,
    }
    const backwards: Layer = {
      id: 'b', kind: 'arrow', from: { x: 100, y: 70 }, to: { x: 10, y: 20 }, style: stroke,
    }
    expect(boundsOf(forwards)).toEqual(expected)
    expect(boundsOf(backwards)).toEqual(expected)
  })

  it('covers every point of a freehand line', () => {
    const line: Layer = {
      id: 'a',
      kind: 'line',
      points: [{ x: 10, y: 50 }, { x: 30, y: 10 }, { x: 20, y: 80 }],
      style: stroke,
    }
    expect(boundsOf(line)).toEqual({ x: 10, y: 10, width: 20, height: 70 })
  })

  it('returns the stored rect for the rect-shaped layers', () => {
    const rect = { x: 5, y: 6, width: 70, height: 80 }
    const shape: Layer = { id: 'a', kind: 'rect', rect, style: { stroke, fill: null } }
    const ellipse: Layer = { id: 'b', kind: 'ellipse', rect, style: { stroke, fill: '#00ff00' } }
    const text: Layer = {
      id: 'c', kind: 'text', rect, content: 'hi', style: { color: '#000000', size: 16, family: 'Inter' },
    }
    const highlight: Layer = { id: 'd', kind: 'highlight', rect, color: '#ffff00' }
    const obscure: Layer = { id: 'e', kind: 'obscure', rect, mode: 'blur', intensity: 8 }
    for (const layer of [shape, ellipse, text, highlight, obscure]) {
      expect(boundsOf(layer)).toEqual(rect)
    }
  })

  // A step badge is stored by its centre, so its box is the badge size laid
  // out around that centre rather than starting at it.
  it('centres the box of a step badge on the badge', () => {
    expect(boundsOf(stepLayer('a', 1))).toEqual({ x: 86, y: 86, width: 28, height: 28 })
  })

  it('does not alias the rect it was given', () => {
    const rect = { x: 5, y: 6, width: 70, height: 80 }
    const layer: Layer = { id: 'a', kind: 'highlight', rect, color: '#ffff00' }
    boundsOf(layer).x = 999
    expect(rect.x).toBe(5)
  })
})
