import { convertFileSrc } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useRef } from 'react'
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
 * How long the window may stay hidden waiting for its backdrop.
 *
 * The frame is a local file the asset protocol serves from the page cache it
 * was written to moments ago, so anything past this is a failure, not a slow
 * load. Without a bound, a window that never resolves either way stays hidden
 * forever: invisible to the user, a phantom in Tauri's window map, and holding
 * its label against the next capture.
 */
const REVEAL_TIMEOUT_MS = 500

export function Overlay({ framePath }: OverlayProps) {
  // Cleared as soon as the window is on screen; fires the fail-safe otherwise.
  const failSafe = useRef<number | undefined>(undefined)

  useEffect(() => {
    failSafe.current = window.setTimeout(() => {
      console.error(`overlay: the frozen frame at ${framePath} did not load in ${REVEAL_TIMEOUT_MS}ms`)
      dismiss()
    }, REVEAL_TIMEOUT_MS)
    return () => {
      window.clearTimeout(failSafe.current)
    }
  }, [framePath])

  // The window is created hidden. Showing it before the backdrop is ready would
  // put a fully transparent, click-swallowing rectangle over a screen that is
  // still moving, which is what freezing exists to prevent.
  //
  // The wait is on `decode`, not on a rendered frame: a hidden window is never
  // composited, so `requestAnimationFrame` does not run and waiting for a paint
  // would hang forever. A decoded image is composited with the window's first
  // frame.
  const revealWindow = (event: SyntheticEvent<HTMLImageElement>) => {
    const image = event.currentTarget
    image
      .decode()
      .catch(() => undefined)
      .then(() => {
        window.clearTimeout(failSafe.current)
        return getCurrentWindow().show()
      })
      // Terminal handler, not `void`: `void` silences the lint, not the
      // rejection. A window that cannot show itself has to close, or it is a
      // hidden phantom holding its label, and in an LSUIElement release build
      // there is no console to notice it in.
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
