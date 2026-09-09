/**
 * The editor document: what an annotated screenshot is made of.
 *
 * Pure data and pure functions. Nothing here touches Tauri, the DOM or React,
 * because this package is shared with the browser extension and its tests run
 * headless. Rendering, hit testing and the toolbar all read this model; none
 * of them may write to it in place. Every edit goes through `./commands`, and
 * every command returns a new document.
 */

export type Point = { x: number; y: number }
export type Rect = { x: number; y: number; width: number; height: number }

/** A colour in `#RRGGBB` form. */
export type Color = string

export type StrokeStyle = { color: Color; width: number }
export type ShapeStyle = { stroke: StrokeStyle; fill: Color | null }
export type TextStyle = { color: Color; size: number; family: string }
export type BadgeStyle = { fill: Color; color: Color; size: number }

/**
 * One annotation.
 *
 * The kinds differ in what they are anchored by, and that is the distinction
 * `boundsOf` has to bridge. `arrow`, `line` and `step` are anchored by points
 * the user dragged or clicked, so their box is computed. The rest carry a rect
 * that the tool already normalised, so their box is that rect.
 */
export type Layer =
  | { id: string; kind: 'arrow'; from: Point; to: Point; style: StrokeStyle }
  | { id: string; kind: 'rect' | 'ellipse'; rect: Rect; style: ShapeStyle }
  | { id: string; kind: 'line'; points: Point[]; style: StrokeStyle }
  | { id: string; kind: 'text'; rect: Rect; content: string; style: TextStyle }
  | { id: string; kind: 'highlight'; rect: Rect; color: Color }
  | { id: string; kind: 'obscure'; rect: Rect; mode: 'blur' | 'pixelate' | 'blackout'; intensity: number }
  | { id: string; kind: 'step'; center: Point; index: number; style: BadgeStyle }

/**
 * A capture plus its annotations.
 *
 * `width` and `height` are the source image in pixels and never change; `crop`
 * is a view onto it, so cropping stays reversible and layers keep their
 * coordinates in source space. `layers` is in paint order, back to front,
 * which is why undoing a removal has to restore the layer at its old index.
 */
export type EditorDocument = {
  width: number
  height: number
  crop: Rect | null
  layers: Layer[]
}

export function createDocument(width: number, height: number): EditorDocument {
  return { width, height, crop: null, layers: [] }
}

/**
 * The number the next step badge should carry.
 *
 * The largest index in use plus one, not the number of badges plus one. Delete
 * badge 2 of three and 1 and 3 are left, so the next one is 4: counting would
 * hand out 3 again and put two identical badges on the same screenshot.
 */
export function nextStepIndex(doc: EditorDocument): number {
  let largest = 0
  for (const layer of doc.layers) {
    if (layer.kind === 'step') largest = Math.max(largest, layer.index)
  }
  return largest + 1
}

/**
 * Axis-aligned bounding box of a layer, in source-image pixels.
 *
 * Always positive, whichever way the user dragged. Selection handles, hit
 * testing and "does this still fit inside the crop" all read this, and a box
 * with a negative width would break every one of them.
 */
export function boundsOf(layer: Layer): Rect {
  switch (layer.kind) {
    case 'arrow':
      return boxAround([layer.from, layer.to])
    case 'line':
      return boxAround(layer.points)
    case 'step':
      return boxAround([layer.center], layer.style.size)
    // Listed rather than defaulted: a layer kind added later without a rect is
    // then a type error here instead of a silent crash at runtime.
    case 'rect':
    case 'ellipse':
    case 'text':
    case 'highlight':
    case 'obscure':
      // A copy, not the layer's own rect: handing out the stored object would
      // let a caller resize the layer by writing to what it was only shown.
      return { ...layer.rect }
  }
}

/**
 * Smallest positive rect covering every point, optionally grown to a square of
 * `size` around them.
 *
 * The `size` argument is what a step badge needs: it is stored by its centre,
 * so its box is the badge diameter laid out around that centre rather than a
 * zero-sized box at it.
 *
 * `points` is never empty in practice. An arrow has both ends from the moment
 * it is created, and a freehand line carries at least the point where the
 * stroke started, because the pointer-down that creates it is also its first
 * sample.
 */
function boxAround(points: Point[], size = 0): Rect {
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const point of points) {
    minX = Math.min(minX, point.x)
    minY = Math.min(minY, point.y)
    maxX = Math.max(maxX, point.x)
    maxY = Math.max(maxY, point.y)
  }
  const margin = size / 2
  return {
    x: minX - margin,
    y: minY - margin,
    width: maxX - minX + size,
    height: maxY - minY + size,
  }
}
