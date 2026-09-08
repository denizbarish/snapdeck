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

export function Overlay({ framePath }: OverlayProps) {
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
    void image
      .decode()
      .catch(() => undefined)
      .then(() => getCurrentWindow().show())
  }

  // Without this a broken path leaves a window that never shows and never
  // explains itself.
  const reportMissingFrame = () => {
    console.error(`overlay: could not load the frozen frame at ${framePath}`)
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
