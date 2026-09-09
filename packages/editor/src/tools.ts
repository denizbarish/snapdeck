/**
 * What each tool builds, and what the toolbar does to a layer that is already
 * there.
 *
 * The editor surface owns pointers, focus and paint; this file owns the rules
 * those events stand for. Keeping them apart is what lets a whole gesture be
 * replayed in a test without a screen: a press, a drag and a release are three
 * points, and what they should have produced is a pure function of them.
 *
 * Everything here returns new values. `History` keeps the layer it was handed
 * as the state to undo to, so a tool that edited one in place would rewrite
 * the past along with the present.
 */

import type { Handle } from './hit'
import { boundsOf, type Color, type EditorDocument, type Layer, type Point, type Rect } from './model'

export type ToolName =
  | 'select'
  | 'arrow'
  | 'rect'
  | 'ellipse'
  | 'line'
  | 'text'
  | 'highlight'
  | 'obscure'
  | 'step'
  | 'crop'

export type ObscureMode = 'blur' | 'pixelate' | 'blackout'

/**
 * The toolbar's current answer to "what should the next mark look like".
 *
 * One colour and one width for every tool rather than a set per tool. The
 * width is a stroke for the drawn kinds and the size knob for the two that
 * have no stroke: a step badge's diameter and a text layer's point size. That
 * is the whole of what `restyleLayer` has to spread across seven kinds, and it
 * is why the toolbar needs two controls rather than a panel per tool.
 */
export type ToolSettings = { color: Color; strokeWidth: number; obscureMode: ObscureMode }

/** A press, wherever the pointer has reached, and every sample in between. */
export type Gesture = {
  /** Where the pointer went down, in source-image pixels. */
  start: Point
  /** Where the pointer is now, in source-image pixels. */
  current: Point
  /**
   * Every sample since the press, oldest first, including `start`.
   *
   * Only the freehand line reads it. The other kinds are defined by their two
   * ends, and rebuilding a rect from a hundred samples would be the same rect.
   */
  samples: Point[]
}

/** The font a text layer is set in. `render.ts` reads it from the layer. */
export const TEXT_FONT_STACK = 'system-ui, -apple-system, "Helvetica Neue", sans-serif'

/**
 * Line height as a multiple of the font size.
 *
 * The same ratio `render.ts` paints with. It is repeated rather than imported
 * because the two uses are different questions: there it decides where the
 * second line is drawn, here it decides how tall the box a click has to land
 * in is. They have to agree, and this comment is the link between them.
 */
const LINE_HEIGHT_RATIO = 1.25

/** Source pixels a drag has to cover before it is worth committing. */
const MIN_EXTENT = 2

/** Badge diameter and text size, as multiples of the toolbar's stroke width. */
const BADGE_SIZE_STEP = 7
const TEXT_SIZE_STEP = 6

/** Smallest badge and text a layer may be built at, in source pixels. */
const MIN_BADGE_SIZE = 12
const MIN_TEXT_SIZE = 10

/** Blur radius and mosaic block, as a multiple of the stroke width. */
const OBSCURE_INTENSITY_STEP = 2
const MIN_OBSCURE_INTENSITY = 2

/**
 * Whether a tool builds its layer by dragging.
 *
 * `select` and `crop` drag but produce no layer, and `text` and `step` are
 * placed by a click with nothing to drag out. The surface branches on this
 * once, at the press, rather than asking again in the move handler.
 */
export function isDragTool(tool: ToolName): boolean {
  switch (tool) {
    case 'arrow':
    case 'rect':
    case 'ellipse':
    case 'line':
    case 'highlight':
    case 'obscure':
      return true
    case 'select':
    case 'crop':
    case 'text':
    case 'step':
      return false
  }
}

/**
 * The layer a gesture stands for, or null when the tool builds none.
 *
 * Called on every pointer move as well as on the release, so the same function
 * produces the preview and the committed layer and the two cannot disagree.
 * `select` and `crop` act on the document rather than adding to it, and `text`
 * needs its content before it has a size, so all three answer null.
 */
export function layerFor(
  tool: ToolName,
  id: string,
  gesture: Gesture,
  settings: ToolSettings,
  stepIndex: number,
): Layer | null {
  const stroke = { color: settings.color, width: settings.strokeWidth }
  switch (tool) {
    case 'arrow':
      return { id, kind: 'arrow', from: gesture.start, to: gesture.current, style: stroke }
    case 'rect':
    case 'ellipse':
      // No fill. An outline shape is drawn around something the user wants
      // seen, and `hit.ts` treats an unfilled shape as a frame, so it does not
      // swallow the clicks meant for whatever it surrounds.
      return {
        id,
        kind: tool,
        rect: normalizeRect(gesture.start, gesture.current),
        style: { stroke, fill: null },
      }
    case 'line':
      return { id, kind: 'line', points: gesture.samples, style: stroke }
    case 'highlight':
      return { id, kind: 'highlight', rect: normalizeRect(gesture.start, gesture.current), color: settings.color }
    case 'obscure':
      return {
        id,
        kind: 'obscure',
        rect: normalizeRect(gesture.start, gesture.current),
        mode: settings.obscureMode,
        intensity: obscureIntensityFor(settings.strokeWidth),
      }
    case 'step':
      // Placed by a click, so the badge is centred on the press and not on
      // wherever the pointer drifted before the release.
      return {
        id,
        kind: 'step',
        center: gesture.start,
        index: stepIndex,
        style: {
          fill: settings.color,
          color: readableTextColor(settings.color),
          size: badgeSizeFor(settings.strokeWidth),
        },
      }
    case 'select':
    case 'crop':
    case 'text':
      return null
  }
}

/**
 * Whether a finished layer is worth adding to the document.
 *
 * A click that was meant to be a drag leaves a zero-sized shape: invisible,
 * unhittable, and still on the undo stack, so the next Cmd+Z appears to do
 * nothing. The tools that are placed by a click are exempt, because for them
 * a press with no movement is the whole gesture.
 */
export function isCommittable(layer: Layer): boolean {
  switch (layer.kind) {
    case 'arrow':
      return Math.hypot(layer.to.x - layer.from.x, layer.to.y - layer.from.y) >= MIN_EXTENT
    case 'line':
      // A tap with the pen is a dot, which `render.ts` paints and `hit.ts`
      // finds, so one sample is enough.
      return layer.points.length > 0
    case 'rect':
    case 'ellipse':
    case 'text':
    case 'highlight':
    case 'obscure':
      return layer.rect.width >= MIN_EXTENT && layer.rect.height >= MIN_EXTENT
    case 'step':
      return layer.style.size > 0
  }
}

/**
 * The layer restyled to the toolbar's current settings.
 *
 * One command's worth of change, which is why this is a function of the whole
 * settings object rather than one per control: changing the colour of a
 * selected step badge also has to fix the contrast of the number on it, and
 * two commands for one click would take two undos to reverse.
 *
 * Geometry is never touched. A restyle that moved the layer would make the
 * colour swatches a second, unlabelled way to nudge things.
 */
export function restyleLayer(layer: Layer, settings: ToolSettings): Layer {
  const stroke = { color: settings.color, width: settings.strokeWidth }
  switch (layer.kind) {
    case 'arrow':
    case 'line':
      return { ...layer, style: stroke }
    case 'rect':
    case 'ellipse':
      // The fill is kept rather than reset. Nothing in this editor sets one
      // today, but a document loaded from elsewhere may carry it, and a
      // restyle is not the place to throw it away.
      return { ...layer, style: { stroke, fill: layer.style.fill } }
    case 'text':
      return {
        ...layer,
        style: { ...layer.style, color: settings.color, size: textSizeFor(settings.strokeWidth) },
      }
    case 'highlight':
      return { ...layer, color: settings.color }
    case 'obscure':
      // No colour: a redaction has none. The width knob drives the blur radius
      // and the mosaic block, which is the only thing about an obscure layer
      // there is to tune.
      return { ...layer, mode: settings.obscureMode, intensity: obscureIntensityFor(settings.strokeWidth) }
    case 'step':
      return {
        ...layer,
        style: {
          fill: settings.color,
          color: readableTextColor(settings.color),
          size: badgeSizeFor(settings.strokeWidth),
        },
      }
  }
}

/**
 * Whether the selection chrome should offer resize handles for this layer.
 *
 * Everything but a step badge. The badge keeps its size through a resize by
 * design, so dragging a handle only re-centres it on the box the drag is
 * making: the centre moves half as far as the pointer, in the direction of the
 * drag, and the badge appears to lag the hand. There is nothing to resize, so
 * there is nothing to grab.
 */
export function hasResizeHandles(layer: Layer): boolean {
  return layer.kind !== 'step'
}

/**
 * The box the selection chrome is drawn on, in source-image pixels.
 *
 * `boundsOf` measures the geometry and excludes the stroke, so an outline
 * drawn straight on it sits inside the ink of a thick line and reads as part
 * of the annotation rather than as a selection. Half the stroke puts the
 * chrome at the outer edge of what is painted; `padding` is the caller's gap
 * beyond that, in the same space.
 */
export function selectionBounds(layer: Layer, padding: number): Rect {
  const bounds = boundsOf(layer)
  const reach = strokeReachOf(layer) + padding
  return {
    x: bounds.x - reach,
    y: bounds.y - reach,
    width: bounds.width + reach * 2,
    height: bounds.height + reach * 2,
  }
}

/**
 * The document with a draft layer standing in for what is stored.
 *
 * A drag has to be visible while it happens, but it is one edit and belongs on
 * the undo stack once, at the release. So nothing is committed until then and
 * the preview is composed here instead: a layer the document already has is
 * replaced in place, keeping its paint order, and a new one goes on top.
 */
export function withDraft(doc: EditorDocument, draft: Layer): EditorDocument {
  const exists = doc.layers.some((layer) => layer.id === draft.id)
  return {
    ...doc,
    layers: exists
      ? doc.layers.map((layer) => (layer.id === draft.id ? draft : layer))
      : [...doc.layers, draft],
  }
}

/** The positive rect between two corners, whichever way the user dragged. */
export function normalizeRect(a: Point, b: Point): Rect {
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(b.x - a.x),
    height: Math.abs(b.y - a.y),
  }
}

/**
 * A rect trimmed to the source image.
 *
 * The crop is the region that gets exported, so a drag that ran off the edge
 * of the picture must not ask the exporter for pixels the capture does not
 * have; those come back transparent and turn a screenshot into one with a
 * blank margin. A rect entirely outside the image collapses to zero and is
 * rejected by the caller's committability check rather than clamped to a
 * sliver of an edge it never touched.
 */
export function clampRectToBounds(rect: Rect, width: number, height: number): Rect {
  const left = Math.min(Math.max(rect.x, 0), width)
  const top = Math.min(Math.max(rect.y, 0), height)
  const right = Math.min(Math.max(rect.x + rect.width, 0), width)
  const bottom = Math.min(Math.max(rect.y + rect.height, 0), height)
  return { x: left, y: top, width: right - left, height: bottom - top }
}

/**
 * The box a piece of text occupies, given something that can measure a line.
 *
 * The measurer is injected because measuring text needs a rasteriser and this
 * package has to stay testable without one. The editor passes a 2D context's
 * `measureText`; a test passes arithmetic.
 *
 * The height is the line count times the line height `render.ts` paints with,
 * not the measured ascent: the box is what a click has to land in to select
 * the layer, and a box that hugged the glyphs would make the space between two
 * lines a miss.
 */
export function measureTextRect(
  origin: Point,
  content: string,
  size: number,
  measureLine: (line: string) => number,
): Rect {
  const lines = content.split('\n')
  let width = 0
  for (const line of lines) width = Math.max(width, measureLine(line))
  return { x: origin.x, y: origin.y, width, height: lines.length * size * LINE_HEIGHT_RATIO }
}

/** Badge diameter for a stroke width. */
export function badgeSizeFor(strokeWidth: number): number {
  return Math.max(MIN_BADGE_SIZE, Math.round(strokeWidth * BADGE_SIZE_STEP))
}

/** Text size for a stroke width. */
export function textSizeFor(strokeWidth: number): number {
  return Math.max(MIN_TEXT_SIZE, Math.round(strokeWidth * TEXT_SIZE_STEP))
}

/** Blur radius or mosaic block for a stroke width, in source pixels. */
export function obscureIntensityFor(strokeWidth: number): number {
  return Math.max(MIN_OBSCURE_INTENSITY, Math.round(strokeWidth * OBSCURE_INTENSITY_STEP))
}

/**
 * Black or white, whichever is legible on `background`.
 *
 * A step badge carries a number on a fill the user chose from a palette that
 * runs from yellow to navy. White on yellow is unreadable and black on navy is
 * worse, and the number is the entire content of the badge.
 *
 * The threshold is the WCAG relative luminance at which black and white
 * contrast equally against a colour. A colour this cannot parse is treated as
 * dark, so the number is drawn in white: unreadable text on an unknown colour
 * is a bug either way, and white is the one that shows up against the dark
 * chrome this editor is drawn in.
 */
export function readableTextColor(background: Color): Color {
  const rgb = parseHexColor(background)
  if (!rgb) return '#ffffff'
  const luminance = 0.2126 * linearize(rgb[0]) + 0.7152 * linearize(rgb[1]) + 0.0722 * linearize(rgb[2])
  return luminance > 0.179 ? '#000000' : '#ffffff'
}

/** How far past its bounding box a layer's ink reaches, in source pixels. */
function strokeReachOf(layer: Layer): number {
  switch (layer.kind) {
    case 'arrow':
    case 'line':
      return layer.style.width / 2
    case 'rect':
    case 'ellipse':
      // The stroke straddles the edge of the rect, so half of it is painted
      // outside the box `boundsOf` reports.
      return layer.style.stroke.width / 2
    // No stroke: text, the two painted regions, and the badge, whose diameter
    // `boundsOf` has already laid around its centre.
    case 'text':
    case 'highlight':
    case 'obscure':
    case 'step':
      return 0
  }
}

/** `#rgb` or `#rrggbb` as three 0-255 channels, or null if it is neither. */
function parseHexColor(color: Color): [number, number, number] | null {
  const hex = color.trim().replace(/^#/, '')
  const expanded =
    hex.length === 3
      ? hex
          .split('')
          .map((digit) => digit + digit)
          .join('')
      : hex
  if (expanded.length !== 6 || !/^[0-9a-fA-F]{6}$/.test(expanded)) return null
  return [
    parseInt(expanded.slice(0, 2), 16),
    parseInt(expanded.slice(2, 4), 16),
    parseInt(expanded.slice(4, 6), 16),
  ]
}

/** One 0-255 channel as linear light, per the sRGB transfer function. */
function linearize(channel: number): number {
  const value = channel / 255
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
}

/**
 * Where a resize handle sits on a box.
 *
 * The same layout `hit.ts` tests against: corners and edge midpoints. It is
 * repeated here rather than exported from there because the two uses are
 * opposite directions of the same fact, drawing and hit testing, and they have
 * to agree; `handleAtPoint` is what the surface asks, and this is what it
 * draws, so a disagreement would show up as a handle that cannot be grabbed.
 */
export function handlePosition(bounds: Rect, handle: Handle): Point {
  const right = bounds.x + bounds.width
  const bottom = bounds.y + bounds.height
  const midX = bounds.x + bounds.width / 2
  const midY = bounds.y + bounds.height / 2
  switch (handle) {
    case 'nw':
      return { x: bounds.x, y: bounds.y }
    case 'n':
      return { x: midX, y: bounds.y }
    case 'ne':
      return { x: right, y: bounds.y }
    case 'e':
      return { x: right, y: midY }
    case 'se':
      return { x: right, y: bottom }
    case 's':
      return { x: midX, y: bottom }
    case 'sw':
      return { x: bounds.x, y: bottom }
    case 'w':
      return { x: bounds.x, y: midY }
  }
}
