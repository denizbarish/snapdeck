import { convertFileSrc } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { SyntheticEvent } from 'react'

export interface OverlayProps {
  displayId: number
  mode: string
  scale: number
  /**
   * Absolute path of the frozen frame, passed by Rust. Keeping it out of the
   * frontend means no path IPC round trip and one place that knows the
   * `frozen-<id>.png` filename.
   */
  framePath: string
}

/**
 * The frozen backdrop for one display.
 *
 * Two outcomes only: the frame loads and the window shows itself, or it fails
 * and the window closes. There is deliberately no timer for the case where
 * neither happens. The window is created hidden and is therefore never
 * composited, which is exactly when WebKit throttles DOM timers, and a page
 * that fails before React mounts never arms one at all. Rust holds the deadline
 * instead: it built the window, it can see whether it ever became visible, and
 * it closes the ones that did not.
 */
export function Overlay({ framePath }: OverlayProps) {
  // Showing the window before the backdrop is ready would put a fully
  // transparent, click-swallowing rectangle over a screen that is still moving,
  // which is what freezing exists to prevent.
  //
  // The wait is on `decode`, not on a rendered frame: a hidden window is never
  // composited, so `requestAnimationFrame` does not run and waiting for a paint
  // would hang forever. A decoded image is composited with the window's first
  // frame. A failed decode is not worth blocking on either, so it falls through
  // to `show`: `onLoad` has already proved the file arrived and parsed.
  const revealWindow = (event: SyntheticEvent<HTMLImageElement>) => {
    const image = event.currentTarget
    image
      .decode()
      .catch(() => undefined)
      .then(() => getCurrentWindow().show())
      // A terminal handler rather than `void`. There is no floating-promise
      // lint in this repo to satisfy; the point is that the rejection is
      // handled instead of merely marked. A window that cannot show itself has
      // to close, or it is a hidden phantom holding its label, and in an
      // LSUIElement release build there is no console to notice it in.
      .catch((error: unknown) => {
        console.error(`overlay: could not show the window for ${framePath}`, error)
        dismiss()
      })
  }

  // Without this a broken path leaves a window that never shows and never
  // explains itself.
  const reportMissingFrame = () => {
    console.error(`overlay: could not load the frozen frame at ${framePath}`)
    dismiss()
  }

  return (
    <img
      src={convertFileSrc(framePath)}
      alt=""
      onLoad={revealWindow}
      onError={reportMissingFrame}
      style={{ width: '100%', height: '100%', display: 'block' }}
    />
  )
}

/**
 * Closes this overlay. Every failure path ends here, because the alternative is
 * a window nobody can see, dismiss, or account for.
 */
function dismiss() {
  getCurrentWindow()
    .close()
    .catch((error: unknown) => {
      console.error('overlay: could not close the window', error)
    })
}
