/**
 * Selection geometry for the overlay.
 *
 * Deliberately pure: no DOM, no React, no Tauri. Every rule about what a
 * selection may look like lives here, where it can be tested without a screen,
 * and the overlay component is left with nothing but event plumbing.
 *
 * All coordinates are display-local CSS points, the space the overlay's own
 * pointer events arrive in. Converting to the global point space the capture
 * needs is the capture command's job, because only Rust knows where the
 * display sits.
 */

export type Point = { x: number; y: number }
export type Rect = { x: number; y: number; width: number; height: number }
export type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w'

/** Smallest selection worth capturing, in CSS pixels. */
const MIN_EDGE = 4

/** Builds a positive-size rect from two drag points, in any direction. */
export function normalizeRect(from: Point, to: Point): Rect {
  return {
    x: Math.min(from.x, to.x),
    y: Math.min(from.y, to.y),
    width: Math.abs(to.x - from.x),
    height: Math.abs(to.y - from.y),
  }
}

/** Trims a rect so it stays inside `bounds`. */
export function clampRect(rect: Rect, bounds: Rect): Rect {
  const x = Math.max(rect.x, bounds.x)
  const y = Math.max(rect.y, bounds.y)
  const right = Math.min(rect.x + rect.width, bounds.x + bounds.width)
  const bottom = Math.min(rect.y + rect.height, bounds.y + bounds.height)
  return { x, y, width: Math.max(0, right - x), height: Math.max(0, bottom - y) }
}

/** Moves a rect without resizing it; a move that would leave `bounds` is refused. */
export function nudgeRect(rect: Rect, dx: number, dy: number, bounds: Rect): Rect {
  const moved = { ...rect, x: rect.x + dx, y: rect.y + dy }
  const fits =
    moved.x >= bounds.x &&
    moved.y >= bounds.y &&
    moved.x + moved.width <= bounds.x + bounds.width &&
    moved.y + moved.height <= bounds.y + bounds.height
  return fits ? moved : rect
}

/** Drags one edge or corner to the pointer, staying positive and inside bounds. */
export function resizeRect(rect: Rect, handle: Handle, pointer: Point, bounds: Rect): Rect {
  let left = rect.x
  let top = rect.y
  let right = rect.x + rect.width
  let bottom = rect.y + rect.height

  if (handle.includes('w')) left = pointer.x
  if (handle.includes('e')) right = pointer.x
  if (handle.includes('n')) top = pointer.y
  if (handle.includes('s')) bottom = pointer.y

  const normalized = normalizeRect({ x: left, y: top }, { x: right, y: bottom })
  return clampRect(normalized, bounds)
}

/**
 * Whether a rect is worth capturing. A press with no drag produces a rect of
 * zero or near-zero size, and capturing it would write a useless file instead
 * of cancelling, which is what the user meant.
 */
export function isUsable(rect: Rect): boolean {
  return rect.width >= MIN_EDGE && rect.height >= MIN_EDGE
}
