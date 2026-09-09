/**
 * Where the document sits on screen, and how a pointer gets back into it.
 *
 * The editor stores every layer in source-image pixels and shows them at
 * whatever size the window happens to allow, so two conversions exist and both
 * live here: document space to CSS pixels for the selection chrome, and CSS
 * pixels back to document space for every pointer event.
 *
 * The scale is expressed as a transform on the canvas and never as a clip.
 * `render.ts` redacts through `getImageData`/`putImageData`, which address raw
 * device pixels and ignore the clip entirely: a clipped view would let an
 * obscure layer write its redaction over pixels the user cannot see, outside
 * the region on show. A transform is honoured by `deviceRect`, so the same
 * redaction lands in the same place at any zoom.
 *
 * Pure: no DOM types cross this boundary. The caller subtracts the canvas box
 * origin from a client coordinate before asking, which keeps `DOMRect` and
 * `devicePixelRatio` in the React file where they belong.
 */

import type { Point, Rect } from './model'

/**
 * How the view is laid out inside the stage, in CSS pixels.
 *
 * `scale` is CSS pixels per source pixel. `offsetX`/`offsetY` are where the
 * canvas element itself is placed, and they are a layout fact rather than a
 * coordinate one: the canvas is exactly the view, so nothing inside it has to
 * know where the stage put it.
 */
export type Viewport = { scale: number; offsetX: number; offsetY: number }

/**
 * Fit a view into a box, centred, never enlarged.
 *
 * Capped at 1:1 rather than filling the box. A screenshot blown up past its
 * own resolution is a blurred lie about what will be exported, and the export
 * is always at source resolution, so magnifying the preview would show the
 * user something the file cannot contain.
 *
 * A box that has not been measured yet, or a view with no area, yields the
 * identity viewport instead of `Infinity` or `NaN`. The first render happens
 * before the `ResizeObserver` has reported anything, and a non-finite scale
 * would reach `setTransform` and blank the canvas.
 */
export function fitViewport(view: Rect, boxWidth: number, boxHeight: number): Viewport {
  const fit = Math.min(boxWidth / view.width, boxHeight / view.height)
  const scale = Number.isFinite(fit) && fit > 0 ? Math.min(1, fit) : 1
  return {
    scale,
    // Clamped at zero: a view wider than its box is pinned to the top-left
    // rather than centred off the edge, where its left half would be
    // unreachable by the pointer.
    offsetX: Math.max(0, (boxWidth - view.width * scale) / 2),
    offsetY: Math.max(0, (boxHeight - view.height * scale) / 2),
  }
}

/**
 * A point in the canvas, in CSS pixels from its top-left, in source pixels.
 *
 * `view` is the cropped region the canvas shows, and adding its origin is the
 * whole of what a crop costs here: layers keep their source coordinates
 * whether or not the document is cropped, so the pointer has to arrive in the
 * same space or every mark would be placed by the width of the crop.
 *
 * The canvas element is exactly the view, so where the stage placed it is not
 * part of this. That is deliberate: the canvas being the view is also what
 * makes the crop visible without a clip, and `render.ts` may not be clipped
 * because `putImageData` ignores a clip and would write a redaction outside it.
 */
export function toDocumentPoint(local: Point, view: Rect, viewport: Viewport): Point {
  return {
    x: view.x + local.x / viewport.scale,
    y: view.y + local.y / viewport.scale,
  }
}

/** A point in source-image pixels, in CSS pixels from the canvas's top-left. */
export function toLocalPoint(point: Point, view: Rect, viewport: Viewport): Point {
  return {
    x: (point.x - view.x) * viewport.scale,
    y: (point.y - view.y) * viewport.scale,
  }
}

/**
 * A rect in source-image pixels, converted to CSS pixels in the canvas box.
 *
 * The size is scaled rather than derived from a second converted corner, so a
 * rect that arrived with a negative width keeps it instead of being silently
 * normalised by the conversion.
 */
export function toLocalRect(rect: Rect, view: Rect, viewport: Viewport): Rect {
  const origin = toLocalPoint(rect, view, viewport)
  return {
    x: origin.x,
    y: origin.y,
    width: rect.width * viewport.scale,
    height: rect.height * viewport.scale,
  }
}

/**
 * A length in CSS pixels, in source-image pixels.
 *
 * Hit tolerances and handle sizes are chosen in screen pixels, because that is
 * where the finger and the cursor are, but `hit.ts` measures in document
 * space. Converting here keeps a target the same physical size at any zoom.
 */
export function toDocumentLength(cssLength: number, viewport: Viewport): number {
  return cssLength / viewport.scale
}
