/**
 * Window snapping for the overlay.
 *
 * Pure, exactly like `./selection`: no DOM, no React, no Tauri. The rules for
 * which window the pointer is over, and where that window sits on this
 * display, are testable without a screen.
 *
 * Two coordinate spaces meet here, and mixing them up is the whole risk. A
 * window's `bounds` arrive in the global point space that spans every display,
 * because that is what ScreenCaptureKit reports. The overlay draws in
 * display-local CSS points, because its window is exactly one display.
 * `toLocalRect` is the only conversion, and it runs one way; the caller
 * converts its pointer the other way before asking `windowUnderPoint`.
 */

import type { Point, Rect } from './selection'

export type WindowBounds = {
  id: number
  title: string | null
  appName: string | null
  bounds: Rect
  layer: number
}

/**
 * Layers above this belong to system UI: the Dock (20), the menu bar (24),
 * notification banners, and anything else that floats over ordinary
 * application windows. Snapdeck's own overlays live up there too, at 25.
 *
 * Filtering here is a second line of defence for the overlays, not the first
 * one. `list_windows` already drops them by window id, which does not depend
 * on the level they happen to be raised to. What this filter is really for is
 * the system UI, which no user means to pick when they aim at the window
 * underneath it.
 */
const NORMAL_LAYER = 0

function contains(rect: Rect, point: Point): boolean {
  return (
    point.x >= rect.x &&
    point.y >= rect.y &&
    point.x < rect.x + rect.width &&
    point.y < rect.y + rect.height
  )
}

/**
 * Frontmost normal-layer window containing the point, in global points.
 *
 * The list is expected in front-to-back order, so the first match wins and
 * overlapping windows resolve the way they look on screen. Producing that
 * order is the provider's job, not this function's; see `list_windows`.
 */
export function windowUnderPoint(windows: WindowBounds[], point: Point): WindowBounds | null {
  return windows.find((w) => w.layer <= NORMAL_LAYER && contains(w.bounds, point)) ?? null
}

/** Rebases global (multi-display) coordinates onto one display's overlay. */
export function toLocalRect(bounds: Rect, displayOrigin: Point): Rect {
  return {
    x: bounds.x - displayOrigin.x,
    y: bounds.y - displayOrigin.y,
    width: bounds.width,
    height: bounds.height,
  }
}
