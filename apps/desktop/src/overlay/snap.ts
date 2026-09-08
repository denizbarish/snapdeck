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
 * The layer ordinary application windows live on, and the only one window mode
 * will pick from.
 *
 * Above it is system UI: the Dock (20), the menu bar (24), notification
 * banners, and anything else that floats over ordinary windows. Snapdeck's own
 * overlays live up there too, at 25. Filtering those here is a second line of
 * defence, not the first one: `list_windows` already drops the overlays by
 * window id, which does not depend on the level they happen to be raised to.
 *
 * Below it is the desktop, and that is why this is an equality rather than a
 * `<=`. macOS parks the wallpaper, the desktop icon layer and the window
 * server's backstop at huge negative layers, and every one of them covers its
 * whole display; measured on a 1710x1112 screen:
 *
 *     layer=-2147483603 Finder        1710x1112
 *     layer=-2147483624 Dock          1710x1112
 *     layer=-2147483626 Window Server 1710x1112
 *
 * `SCShareableContent::get()` is not the desktop-excluding variant, so all of
 * them reach this function. Admitting them means a hover over empty desktop
 * highlights the entire display and a click captures the whole screen, which
 * is precisely what window mode is not.
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
  return windows.find((w) => w.layer === NORMAL_LAYER && contains(w.bounds, point)) ?? null
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
