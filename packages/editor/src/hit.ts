/**
 * Selection geometry: what the pointer is over, and what a drag does to it.
 *
 * Pure like the rest of the package. Pointer events, cursors and handle
 * rendering belong to the editor surface; everything here is arithmetic over
 * the model, so a drag can be replayed in a test without a screen.
 *
 * All coordinates are source-image pixels, the space layers are stored in.
 * `tolerance` and `handleSize` are the caller's slack in that same space: the
 * editor converts them from screen pixels, so a target stays the same physical
 * size however far the user has zoomed in.
 */

import { boundsOf, type Layer, type Point, type Rect } from './model'

export type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w'

/**
 * The layer under a point, or null.
 *
 * Searched front to back, so the answer is the layer the user can actually see
 * there. `layers` is in paint order, back to front, which puts the topmost of
 * an overlapping pair last.
 */
export function layerAtPoint(layers: Layer[], point: Point, tolerance: number): Layer | null {
  for (let index = layers.length - 1; index >= 0; index -= 1) {
    const layer = layers[index]
    if (layer && hitsLayer(layer, point, tolerance)) return layer
  }
  return null
}

/**
 * The resize handle under a point, or null.
 *
 * Handles are squares of `handleSize` centred on the corners and edge
 * midpoints of the layer's bounding box, matching how they are drawn.
 */
export function handleAtPoint(layer: Layer, point: Point, handleSize: number): Handle | null {
  const bounds = boundsOf(layer)
  const left = bounds.x
  const right = bounds.x + bounds.width
  const top = bounds.y
  const bottom = bounds.y + bounds.height
  const midX = left + bounds.width / 2
  const midY = top + bounds.height / 2

  // Corners first. On a layer smaller than the handles themselves the corner
  // and edge squares overlap, and the corner is the one worth having: it
  // resizes both axes, so it can still get the layer out of that size.
  const handles: [Handle, Point][] = [
    ['nw', { x: left, y: top }],
    ['ne', { x: right, y: top }],
    ['se', { x: right, y: bottom }],
    ['sw', { x: left, y: bottom }],
    ['n', { x: midX, y: top }],
    ['e', { x: right, y: midY }],
    ['s', { x: midX, y: bottom }],
    ['w', { x: left, y: midY }],
  ]

  const reach = handleSize / 2
  for (const [handle, position] of handles) {
    if (Math.abs(point.x - position.x) <= reach && Math.abs(point.y - position.y) <= reach) {
      return handle
    }
  }
  return null
}

/**
 * The layer shifted by `dx`, `dy`.
 *
 * A new layer every time. History keeps the layer it was handed as the state
 * to undo to, so a shift written in place would rewrite the past along with
 * the present.
 */
export function moveLayer(layer: Layer, dx: number, dy: number): Layer {
  switch (layer.kind) {
    case 'arrow':
      return { ...layer, from: shift(layer.from, dx, dy), to: shift(layer.to, dx, dy) }
    case 'line':
      // Every sample, not just the box: a line drawn from stale points would
      // paint itself back where it started.
      return { ...layer, points: layer.points.map((point) => shift(point, dx, dy)) }
    case 'step':
      return { ...layer, center: shift(layer.center, dx, dy) }
    case 'rect':
    case 'ellipse':
    case 'text':
    case 'highlight':
    case 'obscure':
      return { ...layer, rect: { ...layer.rect, x: layer.rect.x + dx, y: layer.rect.y + dy } }
  }
}

/**
 * The layer resized by dragging `handle` to `pointer`.
 *
 * `origin` is the layer as it stood when the drag began, and it is the only
 * geometry this reads: the anchor is the edge of `origin` opposite the handle,
 * so it holds still for the whole drag. Deriving the anchor from the layer
 * being resized instead survives exactly as far as the pointer crossing that
 * opposite edge; past it the anchor is recomputed from a box that has already
 * flipped, and the shape stops growing and starts sliding along with the
 * pointer. `layer` supplies everything that is not geometry, so a style change
 * mid-drag is kept.
 *
 * The new box may be a flip of the old one, and that is the point: dragging
 * the east edge past the west one mirrors the shape rather than refusing to
 * move, because refusing makes the handle feel stuck. For the point-anchored
 * kinds the mirroring is visible as a swap: the projection preserves the order
 * of the points, so past a flip the end the user grabbed lands on the anchor
 * and an arrow reverses on screen. That matches what the same drag does in
 * Figma, and it is what the arrow test pins.
 *
 * `origin` must be `layer` as it stood when the drag began, and therefore of
 * the same kind. Nothing else is a coherent anchor, so the contract is the
 * caller's to keep rather than something this branches on.
 */
export function resizeLayer(layer: Layer, handle: Handle, pointer: Point, origin: Layer): Layer {
  const before = boundsOf(origin)
  const after = dragEdges(before, handle, pointer)

  switch (layer.kind) {
    case 'arrow': {
      // The cast states the contract above: TypeScript cannot narrow a second
      // parameter through a switch on the first, and there is no fallback
      // worth writing for a pairing that is a caller bug.
      const source = origin as Extract<Layer, { kind: 'arrow' }>
      return {
        ...layer,
        from: project(source.from, before, after),
        to: project(source.to, before, after),
      }
    }
    case 'line': {
      const source = origin as Extract<Layer, { kind: 'line' }>
      return { ...layer, points: source.points.map((point) => project(point, before, after)) }
    }
    case 'step':
      // The badge keeps its size, which is a style, and rides to the centre of
      // the new box. Scaling a numbered badge with the drag would leave the
      // steps of one screenshot at different sizes.
      return { ...layer, center: { x: after.x + after.width / 2, y: after.y + after.height / 2 } }
    case 'rect':
    case 'ellipse':
    case 'text':
    case 'highlight':
    case 'obscure':
      return { ...layer, rect: after }
  }
}

/** Whether a point is on a layer, given the caller's slack. */
function hitsLayer(layer: Layer, point: Point, tolerance: number): boolean {
  switch (layer.kind) {
    case 'arrow':
      return distanceToSegment(point, layer.from, layer.to) <= reachOf(tolerance, layer.style.width)
    case 'line': {
      const reach = reachOf(tolerance, layer.style.width)
      let previous: Point | null = null
      for (const current of layer.points) {
        // A stroke that has only been sampled once is a dot, and the segment
        // from a point to itself is exactly that.
        if (distanceToSegment(point, previous ?? current, current) <= reach) return true
        previous = current
      }
      return false
    }
    // An unfilled shape is a frame, not a surface. Hit testing it by its box
    // would let it swallow every click meant for whatever it is drawn around.
    case 'rect': {
      // Filled or not, the stroke straddles the edge of the rect, so half of
      // it is painted outside the box. Reaching by the tolerance alone would
      // miss a click on the visible ink of a thick-stroked filled shape.
      const reach = reachOf(tolerance, layer.style.stroke.width)
      return layer.style.fill === null
        ? onEdge(layer.rect, point, reach, containsPoint)
        : containsPoint(inflate(layer.rect, reach), point)
    }
    case 'ellipse': {
      const reach = reachOf(tolerance, layer.style.stroke.width)
      return layer.style.fill === null
        ? onEdge(layer.rect, point, reach, insideEllipse)
        : insideEllipse(inflate(layer.rect, reach), point)
    }
    // Opaque surfaces: text sits in a box the tool sized to it, and the other
    // two are painted regions. None carries a stroke, so the reach is the
    // tolerance alone; it goes through `reachOf` all the same, so every kind
    // reads its target size from one place.
    case 'text':
    case 'highlight':
    case 'obscure':
      return containsPoint(inflate(layer.rect, reachOf(tolerance, 0)), point)
    case 'step':
      return Math.hypot(point.x - layer.center.x, point.y - layer.center.y) <=
        layer.style.size / 2 + tolerance
  }
}

/**
 * How far from the drawn centreline still counts as a hit.
 *
 * Half the stroke, because that is where the ink ends, plus the caller's
 * tolerance. Without the stroke term a thick highlighter line would be a miss
 * everywhere the user can plainly see it.
 */
function reachOf(tolerance: number, strokeWidth: number): number {
  return tolerance + strokeWidth / 2
}

/** Whether a point is within `reach` of the outline of a shape, inside or out. */
function onEdge(
  rect: Rect,
  point: Point,
  reach: number,
  inside: (rect: Rect, point: Point) => boolean,
): boolean {
  return inside(inflate(rect, reach), point) && !inside(inflate(rect, -reach), point)
}

function containsPoint(rect: Rect, point: Point): boolean {
  return (
    point.x >= rect.x &&
    point.x <= rect.x + rect.width &&
    point.y >= rect.y &&
    point.y <= rect.y + rect.height
  )
}

/** Whether a point is inside the ellipse inscribed in a rect. */
function insideEllipse(rect: Rect, point: Point): boolean {
  const radiusX = rect.width / 2
  const radiusY = rect.height / 2
  if (radiusX <= 0 || radiusY <= 0) return false
  const dx = (point.x - (rect.x + radiusX)) / radiusX
  const dy = (point.y - (rect.y + radiusY)) / radiusY
  return dx * dx + dy * dy <= 1
}

/**
 * A rect grown by `amount` on every side.
 *
 * A negative amount shrinks it, and one large enough turns the width or height
 * negative. That is left alone rather than clamped: `containsPoint` reports
 * nothing inside such a rect, which is the right answer for a shape too small
 * to have an interior distinct from its edge.
 */
function inflate(rect: Rect, amount: number): Rect {
  return {
    x: rect.x - amount,
    y: rect.y - amount,
    width: rect.width + amount * 2,
    height: rect.height + amount * 2,
  }
}

/** The bounding box with the dragged edges moved to the pointer, kept positive. */
function dragEdges(bounds: Rect, handle: Handle, pointer: Point): Rect {
  let left = bounds.x
  let top = bounds.y
  let right = bounds.x + bounds.width
  let bottom = bounds.y + bounds.height

  if (handle.includes('w')) left = pointer.x
  if (handle.includes('e')) right = pointer.x
  if (handle.includes('n')) top = pointer.y
  if (handle.includes('s')) bottom = pointer.y

  return {
    x: Math.min(left, right),
    y: Math.min(top, bottom),
    width: Math.abs(right - left),
    height: Math.abs(bottom - top),
  }
}

/** A point carried from one box to another, keeping its relative position. */
function project(point: Point, before: Rect, after: Rect): Point {
  return {
    x: interpolate(point.x, before.x, before.width, after.x, after.width),
    y: interpolate(point.y, before.y, before.height, after.y, after.height),
  }
}

function interpolate(
  value: number,
  fromStart: number,
  fromSize: number,
  toStart: number,
  toSize: number,
): number {
  // A shape that is flat on this axis, a horizontal line or a two-point
  // vertical arrow, has no extent to spread the value across. It follows the
  // moved edge instead of dividing by zero.
  if (fromSize === 0) return toStart
  return toStart + ((value - fromStart) / fromSize) * toSize
}

/**
 * Distance from a point to a line segment, not to the infinite line.
 *
 * The projection is clamped to the segment, so a click level with an arrow but
 * well past its tip is as far away as it looks. A segment of zero length is a
 * point, which is what the first sample of a freehand stroke is.
 */
function distanceToSegment(point: Point, start: Point, end: Point): number {
  const dx = end.x - start.x
  const dy = end.y - start.y
  const lengthSquared = dx * dx + dy * dy
  if (lengthSquared === 0) return Math.hypot(point.x - start.x, point.y - start.y)
  const along = ((point.x - start.x) * dx + (point.y - start.y) * dy) / lengthSquared
  const clamped = Math.max(0, Math.min(1, along))
  return Math.hypot(point.x - (start.x + clamped * dx), point.y - (start.y + clamped * dy))
}

function shift(point: Point, dx: number, dy: number): Point {
  return { x: point.x + dx, y: point.y + dy }
}
