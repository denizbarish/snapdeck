import { describe, expect, it } from 'vitest'
import { createDocument, type EditorDocument, type Layer } from './model'
import {
  badgeSizeFor,
  clampRectToBounds,
  handlePosition,
  hasResizeHandles,
  isCommittable,
  isDragTool,
  layerFor,
  measureTextRect,
  normalizeRect,
  obscureIntensityFor,
  readableTextColor,
  restyleLayer,
  selectionBounds,
  TEXT_FONT_STACK,
  textSizeFor,
  withDraft,
  type Gesture,
  type ToolSettings,
} from './tools'

const settings: ToolSettings = { color: '#ff3b30', strokeWidth: 4, obscureMode: 'blur' }

function drag(fromX: number, fromY: number, toX: number, toY: number): Gesture {
  return {
    start: { x: fromX, y: fromY },
    current: { x: toX, y: toY },
    samples: [
      { x: fromX, y: fromY },
      { x: (fromX + toX) / 2, y: (fromY + toY) / 2 },
      { x: toX, y: toY },
    ],
  }
}

describe('layerFor', () => {
  it('builds an arrow from the two ends of the drag', () => {
    const layer = layerFor('arrow', 'a', drag(10, 10, 100, 50), settings, 1)
    expect(layer).toEqual({
      id: 'a',
      kind: 'arrow',
      from: { x: 10, y: 10 },
      to: { x: 100, y: 50 },
      style: { color: '#ff3b30', width: 4 },
    })
  })

  // Dragging up and to the left is the same rectangle as dragging down and to
  // the right. Everything downstream, `boundsOf`, hit testing and the
  // exporter, assumes a positive box.
  it('normalises a rectangle dragged backwards', () => {
    const layer = layerFor('rect', 'a', drag(100, 100, 40, 60), settings, 1)
    expect(layer?.kind === 'rect' && layer.rect).toEqual({ x: 40, y: 60, width: 60, height: 40 })
  })

  // An outline shape is drawn around something the user wants seen, and
  // `hit.ts` only lets an unfilled shape be grabbed by its edge. A fill would
  // make every rectangle swallow the clicks meant for what it surrounds.
  it('leaves shapes unfilled', () => {
    const layer = layerFor('ellipse', 'a', drag(0, 0, 10, 10), settings, 1)
    expect(layer?.kind === 'ellipse' && layer.style.fill).toBeNull()
  })

  it('builds a freehand line from every sample, not from the two ends', () => {
    const layer = layerFor('line', 'a', drag(0, 0, 100, 100), settings, 1)
    expect(layer?.kind === 'line' && layer.points).toHaveLength(3)
  })

  // The surface pushes onto the gesture's array on every pointer move, so a
  // layer holding that same array would grow with the pointer after it was
  // committed. This file promises new values; handing out the caller's array
  // is the one place it did not.
  it('copies the samples rather than aliasing the gesture', () => {
    const gesture = drag(0, 0, 100, 100)
    const layer = layerFor('line', 'a', gesture, settings, 1)
    gesture.samples.push({ x: 200, y: 200 })
    expect(layer?.kind === 'line' && layer.points).toHaveLength(3)
  })

  it('carries the obscure mode and derives its intensity from the width', () => {
    const wide = { ...settings, strokeWidth: 9, obscureMode: 'pixelate' as const }
    const layer = layerFor('obscure', 'a', drag(0, 0, 50, 50), wide, 1)
    expect(layer?.kind === 'obscure' && layer.mode).toBe('pixelate')
    // The width knob, not a constant: a mosaic that ignored it would be the
    // same block size whatever the user asked for.
    expect(layer?.kind === 'obscure' && layer.intensity).toBe(obscureIntensityFor(9))
    expect(obscureIntensityFor(9)).not.toBe(obscureIntensityFor(4))
  })

  // The badge is placed by a click, so it belongs at the press and not at
  // wherever the pointer drifted before the release.
  it('centres a step badge on the press, not on the release', () => {
    const layer = layerFor('step', 'a', drag(30, 40, 300, 400), settings, 7)
    expect(layer?.kind === 'step' && layer.center).toEqual({ x: 30, y: 40 })
    expect(layer?.kind === 'step' && layer.index).toBe(7)
  })

  it('gives the badge number a colour that is legible on the badge', () => {
    const onYellow = layerFor('step', 'a', drag(0, 0, 0, 0), { ...settings, color: '#ffcc00' }, 1)
    const onNavy = layerFor('step', 'b', drag(0, 0, 0, 0), { ...settings, color: '#001a4d' }, 1)
    expect(onYellow?.kind === 'step' && onYellow.style.color).toBe('#000000')
    expect(onNavy?.kind === 'step' && onNavy.style.color).toBe('#ffffff')
  })

  it('builds nothing for the tools that do not add layers', () => {
    expect(layerFor('select', 'a', drag(0, 0, 10, 10), settings, 1)).toBeNull()
    expect(layerFor('crop', 'a', drag(0, 0, 10, 10), settings, 1)).toBeNull()
    // Text has no size until there is something in it, so it is built by the
    // text box rather than by the gesture that opened it.
    expect(layerFor('text', 'a', drag(0, 0, 10, 10), settings, 1)).toBeNull()
  })
})

describe('isDragTool', () => {
  it('separates the tools that drag from the tools that are placed by a click', () => {
    expect(isDragTool('arrow')).toBe(true)
    expect(isDragTool('obscure')).toBe(true)
    expect(isDragTool('step')).toBe(false)
    expect(isDragTool('text')).toBe(false)
    expect(isDragTool('select')).toBe(false)
    expect(isDragTool('crop')).toBe(false)
  })
})

describe('isCommittable', () => {
  // A click that was meant to be a drag leaves a shape with no size:
  // invisible, unhittable, and still on the undo stack, so the next Cmd+Z
  // would appear to do nothing.
  it('rejects a rectangle a stray click left behind', () => {
    const layer = layerFor('rect', 'a', drag(50, 50, 50, 50), settings, 1)
    expect(layer && isCommittable(layer)).toBe(false)
  })

  it('rejects an arrow with no length and accepts one with some', () => {
    const stub = layerFor('arrow', 'a', drag(50, 50, 51, 50), settings, 1)
    const real = layerFor('arrow', 'b', drag(50, 50, 90, 50), settings, 1)
    expect(stub && isCommittable(stub)).toBe(false)
    expect(real && isCommittable(real)).toBe(true)
  })

  // `render.ts` paints a one-sample stroke as a dot and `hit.ts` finds it, so
  // a tap with the pen is a mark the user meant to make.
  it('accepts a freehand line of a single sample', () => {
    const dot: Layer = {
      id: 'a',
      kind: 'line',
      points: [{ x: 5, y: 5 }],
      style: { color: '#ff3b30', width: 4 },
    }
    expect(isCommittable(dot)).toBe(true)
  })

  it('accepts a step badge placed without a drag', () => {
    const layer = layerFor('step', 'a', drag(20, 20, 20, 20), settings, 1)
    expect(layer && isCommittable(layer)).toBe(true)
  })
})

describe('restyleLayer', () => {
  const next: ToolSettings = { color: '#0a84ff', strokeWidth: 10, obscureMode: 'pixelate' }

  it('restyles an arrow without moving it', () => {
    const arrow: Layer = {
      id: 'a',
      kind: 'arrow',
      from: { x: 1, y: 2 },
      to: { x: 3, y: 4 },
      style: { color: '#ff3b30', width: 4 },
    }
    const restyled = restyleLayer(arrow, next)
    expect(restyled).toEqual({ ...arrow, style: { color: '#0a84ff', width: 10 } })
  })

  it('keeps a fill that came from somewhere else', () => {
    const filled: Layer = {
      id: 'a',
      kind: 'rect',
      rect: { x: 0, y: 0, width: 10, height: 10 },
      style: { stroke: { color: '#ff3b30', width: 4 }, fill: '#00ff00' },
    }
    expect(restyleLayer(filled, next)).toEqual({
      ...filled,
      style: { stroke: { color: '#0a84ff', width: 10 }, fill: '#00ff00' },
    })
  })

  // The number on a badge has to stay readable against the fill the user just
  // picked, and both are one click, so both are one command.
  it('fixes the badge number contrast in the same change as the fill', () => {
    const badge: Layer = {
      id: 'a',
      kind: 'step',
      center: { x: 10, y: 10 },
      index: 1,
      style: { fill: '#001a4d', color: '#ffffff', size: 28 },
    }
    const restyled = restyleLayer(badge, { ...next, color: '#ffcc00' })
    expect(restyled.kind === 'step' && restyled.style.fill).toBe('#ffcc00')
    expect(restyled.kind === 'step' && restyled.style.color).toBe('#000000')
    expect(restyled.kind === 'step' && restyled.style.size).toBe(badgeSizeFor(10))
  })

  // A redaction has no colour, so the width knob is the only thing about it
  // there is to tune, and the mode comes from the toolbar's own control.
  it('gives an obscure layer the mode and the intensity, and no colour', () => {
    const obscure: Layer = {
      id: 'a',
      kind: 'obscure',
      rect: { x: 0, y: 0, width: 10, height: 10 },
      mode: 'blur',
      intensity: 8,
    }
    const restyled = restyleLayer(obscure, next)
    expect(restyled).toEqual({ ...obscure, mode: 'pixelate', intensity: obscureIntensityFor(10) })
  })

  // The origin is what this function may not move, not the box. The box is a
  // measurement of the ink at a size this call has just changed, and measuring
  // needs a rasteriser this file does not have, so the width and the height
  // that come back are stale by construction and the caller re-measures them in
  // the same command. Asserting the whole rect here would certify that stale
  // pair as the answer, which is how a caption twice the size of its own
  // selection outline got past a green suite once already. `Editor.test.tsx`
  // covers the box, in a browser, where a glyph can actually be measured.
  it('sizes text from the width knob and never moves its origin', () => {
    const text: Layer = {
      id: 'a',
      kind: 'text',
      rect: { x: 5, y: 6, width: 100, height: 20 },
      content: 'hello',
      style: { color: '#ff3b30', size: 24, family: 'Inter' },
    }
    const restyled = restyleLayer(text, next)
    expect(restyled.kind === 'text' && restyled.style.size).toBe(textSizeFor(10))
    expect(restyled.kind === 'text' && restyled.style.family).toBe('Inter')
    expect(restyled.kind === 'text' && restyled.rect.x).toBe(text.rect.x)
    expect(restyled.kind === 'text' && restyled.rect.y).toBe(text.rect.y)
  })

  it('never touches geometry', () => {
    const highlight: Layer = {
      id: 'a',
      kind: 'highlight',
      rect: { x: 3, y: 4, width: 50, height: 6 },
      color: '#ffcc00',
    }
    expect(restyleLayer(highlight, next)).toEqual({ ...highlight, color: '#0a84ff' })
  })
})

describe('selectionBounds', () => {
  // `boundsOf` measures the geometry and excludes the stroke, so an outline
  // drawn straight on it sits inside the ink of a thick line and reads as part
  // of the annotation rather than as a selection.
  it('clears the ink of a thick stroke', () => {
    const arrow: Layer = {
      id: 'a',
      kind: 'arrow',
      from: { x: 100, y: 100 },
      to: { x: 200, y: 100 },
      style: { color: '#ff3b30', width: 20 },
    }
    expect(selectionBounds(arrow, 0)).toEqual({ x: 90, y: 90, width: 120, height: 20 })
  })

  it('adds the padding on every side', () => {
    const highlight: Layer = {
      id: 'a',
      kind: 'highlight',
      rect: { x: 10, y: 10, width: 100, height: 20 },
      color: '#ffcc00',
    }
    expect(selectionBounds(highlight, 3)).toEqual({ x: 7, y: 7, width: 106, height: 26 })
  })

  // The badge's diameter is already laid around its centre by `boundsOf`, so
  // adding half of it again would draw the chrome a badge-width clear of it.
  it('adds no stroke reach to a step badge', () => {
    const badge: Layer = {
      id: 'a',
      kind: 'step',
      center: { x: 50, y: 50 },
      index: 1,
      style: { fill: '#ff3b30', color: '#ffffff', size: 28 },
    }
    expect(selectionBounds(badge, 0)).toEqual({ x: 36, y: 36, width: 28, height: 28 })
  })
})

describe('hasResizeHandles', () => {
  // Two layers whose box is a measurement of something else, so a drag that
  // changed the box without changing the thing would leave the two describing
  // different shapes. A resize keeps the badge's size and only re-centres it on
  // the box the drag is making, so the badge moves half as far as the pointer
  // and appears to lag the hand. A caption's rect comes from `measureTextRect`
  // at its point size and nothing re-measures after a resize, so the renderer
  // would paint the old size from the new origin while the outline and the hit
  // test described the dragged box; the width control resizes text instead, and
  // it re-measures.
  it('offers none on a step badge or a caption, and some on everything else', () => {
    const badge: Layer = {
      id: 'a',
      kind: 'step',
      center: { x: 10, y: 10 },
      index: 1,
      style: { fill: '#ff3b30', color: '#ffffff', size: 28 },
    }
    const rect: Layer = {
      id: 'b',
      kind: 'rect',
      rect: { x: 0, y: 0, width: 10, height: 10 },
      style: { stroke: { color: '#ff3b30', width: 4 }, fill: null },
    }
    const caption: Layer = {
      id: 'c',
      kind: 'text',
      rect: { x: 0, y: 0, width: 40, height: 25 },
      content: 'hello',
      style: { color: '#ff3b30', size: 20, family: TEXT_FONT_STACK },
    }
    expect(hasResizeHandles(badge)).toBe(false)
    expect(hasResizeHandles(caption)).toBe(false)
    expect(hasResizeHandles(rect)).toBe(true)
  })
})

describe('handlePosition', () => {
  const bounds = { x: 100, y: 200, width: 80, height: 40 }

  it('puts the handles on the corners and the edge midpoints', () => {
    expect(handlePosition(bounds, 'nw')).toEqual({ x: 100, y: 200 })
    expect(handlePosition(bounds, 'n')).toEqual({ x: 140, y: 200 })
    expect(handlePosition(bounds, 'ne')).toEqual({ x: 180, y: 200 })
    expect(handlePosition(bounds, 'e')).toEqual({ x: 180, y: 220 })
    expect(handlePosition(bounds, 'se')).toEqual({ x: 180, y: 240 })
    expect(handlePosition(bounds, 's')).toEqual({ x: 140, y: 240 })
    expect(handlePosition(bounds, 'sw')).toEqual({ x: 100, y: 240 })
    expect(handlePosition(bounds, 'w')).toEqual({ x: 100, y: 220 })
  })
})

describe('withDraft', () => {
  function doc(): EditorDocument {
    const base = createDocument(800, 600)
    return {
      ...base,
      layers: [
        { id: 'a', kind: 'highlight', rect: { x: 0, y: 0, width: 5, height: 5 }, color: '#ffcc00' },
        { id: 'b', kind: 'highlight', rect: { x: 5, y: 5, width: 5, height: 5 }, color: '#ffcc00' },
      ],
    }
  }

  const draft: Layer = {
    id: 'a',
    kind: 'highlight',
    rect: { x: 50, y: 50, width: 5, height: 5 },
    color: '#ffcc00',
  }

  // Layers paint back to front. A drag on the bottom layer that appended it
  // instead would pull it silently to the front of the picture.
  it('replaces a layer in place, keeping its paint order', () => {
    const preview = withDraft(doc(), draft)
    expect(preview.layers.map((layer) => layer.id)).toEqual(['a', 'b'])
    expect(preview.layers[0]).toBe(draft)
  })

  it('puts a layer the document does not have on top', () => {
    const preview = withDraft(doc(), { ...draft, id: 'c' })
    expect(preview.layers.map((layer) => layer.id)).toEqual(['a', 'b', 'c'])
  })

  it('does not touch the document it was given', () => {
    const original = doc()
    withDraft(original, draft)
    const first = original.layers[0]
    expect(first?.kind === 'highlight' && first.rect).toEqual({ x: 0, y: 0, width: 5, height: 5 })
  })
})

describe('normalizeRect', () => {
  it('is positive whichever way the drag went', () => {
    expect(normalizeRect({ x: 10, y: 10 }, { x: 0, y: 0 })).toEqual({ x: 0, y: 0, width: 10, height: 10 })
    expect(normalizeRect({ x: 0, y: 0 }, { x: 10, y: 10 })).toEqual({ x: 0, y: 0, width: 10, height: 10 })
  })
})

describe('clampRectToBounds', () => {
  // The crop is the region that gets exported. A crop that ran off the edge
  // would ask the exporter for pixels the capture does not have, and those
  // come back transparent: a screenshot with a blank margin.
  it('trims a crop that ran off the picture', () => {
    expect(clampRectToBounds({ x: -50, y: -50, width: 200, height: 200 }, 800, 600)).toEqual({
      x: 0,
      y: 0,
      width: 150,
      height: 150,
    })
    expect(clampRectToBounds({ x: 700, y: 500, width: 400, height: 400 }, 800, 600)).toEqual({
      x: 700,
      y: 500,
      width: 100,
      height: 100,
    })
  })

  it('collapses a rect entirely outside the picture rather than clamping it to an edge', () => {
    const clamped = clampRectToBounds({ x: 900, y: 700, width: 50, height: 50 }, 800, 600)
    expect(clamped.width).toBe(0)
    expect(clamped.height).toBe(0)
  })

  it('leaves a rect that already fits alone', () => {
    const rect = { x: 10, y: 20, width: 100, height: 50 }
    expect(clampRectToBounds(rect, 800, 600)).toEqual(rect)
  })
})

describe('measureTextRect', () => {
  const measure = (line: string): number => line.length * 10

  it('is as wide as the widest line and as tall as the line height allows', () => {
    // Two lines at 20px with the 1.25 ratio `render.ts` paints with.
    expect(measureTextRect({ x: 5, y: 6 }, 'ab\nabcd', 20, measure)).toEqual({
      x: 5,
      y: 6,
      width: 40,
      height: 50,
    })
  })

  // The box is what a click has to land in to select the layer. A box that
  // hugged the glyphs would make the space between two lines a miss.
  it('counts a trailing empty line, because the caret was there', () => {
    expect(measureTextRect({ x: 0, y: 0 }, 'a\n', 10, measure).height).toBe(25)
  })
})

describe('readableTextColor', () => {
  it('reads black on light fills and white on dark ones', () => {
    expect(readableTextColor('#ffcc00')).toBe('#000000')
    expect(readableTextColor('#ffffff')).toBe('#000000')
    expect(readableTextColor('#000000')).toBe('#ffffff')
    expect(readableTextColor('#001a4d')).toBe('#ffffff')
    // The system blue is on the light side of the threshold, and that is not
    // a rounding accident: black on it clears 5:1 where white manages 3.6:1.
    expect(readableTextColor('#0a84ff')).toBe('#000000')
  })

  it('accepts the three-digit form', () => {
    expect(readableTextColor('#fff')).toBe('#000000')
    expect(readableTextColor('#000')).toBe('#ffffff')
  })

  // Weighted by the eye's sensitivity, not by an average of the channels:
  // full green is bright enough to need black on it and full blue is not, and
  // a plain mean gets both wrong.
  it('weights the channels rather than averaging them', () => {
    expect(readableTextColor('#00ff00')).toBe('#000000')
    expect(readableTextColor('#0000ff')).toBe('#ffffff')
  })

  it('falls back to white on a colour it cannot parse', () => {
    expect(readableTextColor('rebeccapurple')).toBe('#ffffff')
    expect(readableTextColor('#12345')).toBe('#ffffff')
    // `#RRGGBBAA` is a colour, but it is not one this understands, and reading
    // its first six digits would answer for a colour the caller did not name.
    // Nothing in the toolbar emits one; a document written by another host
    // could.
    expect(readableTextColor('#ffffff00')).toBe('#ffffff')
  })
})

describe('size derivations', () => {
  it('scales the badge, the text and the redaction with the width knob', () => {
    expect(badgeSizeFor(4)).toBe(28)
    expect(textSizeFor(4)).toBe(24)
    expect(obscureIntensityFor(4)).toBe(8)
  })

  // A badge or a caption too small to read is an annotation that says nothing,
  // however small the width knob is turned.
  it('keeps a floor under every one of them', () => {
    expect(badgeSizeFor(1)).toBe(12)
    expect(textSizeFor(1)).toBe(10)
    expect(obscureIntensityFor(1)).toBe(2)
  })

  // Below the slider's own minimum, so nothing in the toolbar can ask for it.
  // The floor is what makes the function safe on its own all the same: a
  // mosaic of one-pixel blocks is the identity function, each block's mean
  // being the pixel itself, so an intensity under two is a redaction that
  // redacted nothing. `render.ts` holds the same floor at the point of paint;
  // this one keeps it out of the stored layer as well.
  it('keeps a redaction from collapsing into no redaction', () => {
    expect(obscureIntensityFor(0.5)).toBe(2)
    expect(badgeSizeFor(0.1)).toBe(12)
    expect(textSizeFor(0.1)).toBe(10)
  })
})
