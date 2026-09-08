import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent, SyntheticEvent } from 'react'
import {
  clampRect,
  isUsable,
  normalizeRect,
  nudgeRect,
  resizeRect,
  type Handle,
  type Point,
  type Rect,
} from './selection'

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

/** Every resize handle, in clockwise order from the top-left corner. */
const HANDLES: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']

/** Edge length of a handle, in CSS pixels. */
const HANDLE_SIZE = 8

/** How far above the selection the size readout sits, in CSS pixels. */
const READOUT_OFFSET = 24

/** Arrow-key step, and the coarse step Shift asks for. */
const NUDGE_STEP = 1
const NUDGE_STEP_COARSE = 10

/** Dimming applied to everything outside the selection. */
const DIM = 'rgba(0,0,0,0.35)'

/**
 * The overlay for one display: a frozen backdrop with a selection drawn on it.
 *
 * Two outcomes only for the backdrop: the frame loads and the window shows
 * itself, or it fails and the window closes. There is deliberately no timer for
 * the case where neither happens. The window is created hidden and is therefore
 * never composited, which is exactly when WebKit throttles DOM timers, and a
 * page that fails before React mounts never arms one at all. Rust holds the
 * deadline instead: it built the window, it can see whether it ever became
 * visible, and it closes the ones that did not.
 *
 * Everything the selection is allowed to do lives in `./selection`, which is
 * pure and unit tested. What is left here is event plumbing and paint.
 */
export function Overlay({ displayId, scale, framePath }: OverlayProps) {
  const [selection, setSelection] = useState<Rect | null>(null)
  /** Where the current drag began, or `null` when no drag is in progress. */
  const dragStart = useRef<Point | null>(null)
  /** The handle being dragged, or `null` when the pointer is not on one. */
  const activeHandle = useRef<Handle | null>(null)

  // The window is exactly one display, so the viewport is the display and the
  // selection may go anywhere in it. Read every render rather than cached: a
  // display that changes resolution while the overlay is up would otherwise
  // clamp against a size that no longer exists.
  const bounds = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }

  const confirm = (rect: Rect) => {
    // Confirming a selection too small to be worth capturing is a cancel, not
    // a capture of nothing.
    if (!isUsable(rect)) return dismissAll()
    // `capture_region` belongs to Task 10 and does not exist yet. Logging the
    // exact rect that would have been sent keeps the gap visible instead of
    // letting a finished selection disappear as if it had been saved. The
    // numbers are display-local points; converting them to the global space is
    // that command's job, because only Rust knows where this display sits.
    console.warn(
      `overlay: region capture is not wired yet (Task 10), dropping the selection for display ${displayId}:`,
      rect,
    )
    dismissAll()
  }

  // No dependency array on purpose. The handler closes over `selection` and
  // `bounds`, both of which change on almost every render, so a memoised
  // listener would nudge a stale rect. Re-subscribing costs one
  // add/removeEventListener pair per render on a window that exists for a few
  // seconds.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') return dismissAll()
      if (!selection) return
      if (event.key === 'Enter') return confirm(selection)
      const step = event.shiftKey ? NUDGE_STEP_COARSE : NUDGE_STEP
      const deltas: Record<string, [number, number]> = {
        ArrowLeft: [-step, 0],
        ArrowRight: [step, 0],
        ArrowUp: [0, -step],
        ArrowDown: [0, step],
      }
      const delta = deltas[event.key]
      if (delta) {
        // Otherwise the arrow keys scroll the page under the selection.
        event.preventDefault()
        setSelection(nudgeRect(selection, delta[0], delta[1], bounds))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

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
      // Visible is not enough. Escape and the arrow keys are the only way out
      // of an overlay and the only way to fine-tune a selection, and this is a
      // menu bar agent that is not the active application, so its key window
      // receives nothing until the app itself is activated. Requested after
      // `show` because AppKit ignores a focus request for a window that is not
      // on screen yet.
      .then(() => getCurrentWindow().setFocus())
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

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    dragStart.current = { x: event.clientX, y: event.clientY }
    setSelection({ x: event.clientX, y: event.clientY, width: 0, height: 0 })
  }

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = { x: event.clientX, y: event.clientY }
    // A handle drag is checked first: it starts without touching `dragStart`,
    // so the two are never both live.
    if (activeHandle.current && selection) {
      setSelection(resizeRect(selection, activeHandle.current, pointer, bounds))
      return
    }
    if (!dragStart.current) return
    setSelection(clampRect(normalizeRect(dragStart.current, pointer), bounds))
  }

  // Releasing the pointer ends the drag or the resize and confirms nothing. The
  // selection stays adjustable, so the eight handles and the arrow keys are
  // reachable: Enter captures it, Escape cancels. Capturing on release would
  // make all three dead UI, because the overlay would already be gone by the
  // time the user reached for them.
  const onPointerUp = () => {
    activeHandle.current = null
    dragStart.current = null
  }

  return (
    <div
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      style={{ position: 'relative', width: '100%', height: '100%' }}
    >
      <img
        src={convertFileSrc(framePath)}
        alt=""
        onLoad={revealWindow}
        onError={reportMissingFrame}
        style={{ width: '100%', height: '100%', display: 'block' }}
      />
      {/*
        Two ways to dim, one at a time. With no selection there is nothing to
        cut a hole in, so a plain sheet covers the display. With a selection the
        hole is the selection itself, and the only way to leave it untouched is
        a spread `box-shadow` painted around it; keeping the sheet as well would
        dim the selected region too, which is the one region that has to stay
        true to what will be captured.
      */}
      {!selection && <div style={{ position: 'absolute', inset: 0, background: DIM }} />}
      {selection && (
        <>
          <div
            style={{
              position: 'absolute',
              left: selection.x,
              top: selection.y,
              width: selection.width,
              height: selection.height,
              boxShadow: `0 0 0 9999px ${DIM}`,
              outline: '1px solid #fff',
            }}
          />
          <div
            style={{
              position: 'absolute',
              left: selection.x,
              top: Math.max(0, selection.y - READOUT_OFFSET),
              padding: '2px 6px',
              background: '#000',
              color: '#fff',
              font: '12px ui-monospace, monospace',
              borderRadius: 4,
            }}
          >
            {/* Device pixels, which is what the saved file will contain. */}
            {Math.round(selection.width * scale)} × {Math.round(selection.height * scale)}
            {/*
              The release no longer captures, so the two keys that finish the
              capture have to be visible; otherwise a finished selection just
              sits there with no clue how to commit it.
            */}
            <span style={{ opacity: 0.7, marginLeft: 8 }}>Enter to capture · Esc to cancel</span>
          </div>
          {HANDLES.map((handle) => {
            const position = handlePosition(selection, handle)
            return (
              <div
                key={handle}
                // Without this the press also reaches the backdrop and starts a
                // fresh drag, which throws away the selection being resized.
                onPointerDown={(event) => {
                  event.stopPropagation()
                  activeHandle.current = handle
                }}
                style={{
                  position: 'absolute',
                  left: position.left - HANDLE_SIZE / 2,
                  top: position.top - HANDLE_SIZE / 2,
                  width: HANDLE_SIZE,
                  height: HANDLE_SIZE,
                  background: '#fff',
                  border: '1px solid #000',
                  boxSizing: 'border-box',
                  cursor: `${handle}-resize`,
                }}
              />
            )
          })}
        </>
      )}
    </div>
  )
}

/** Centre of a handle, in the same display-local points as the selection. */
function handlePosition(rect: Rect, handle: Handle): { left: number; top: number } {
  const left = handle.includes('w')
    ? rect.x
    : handle.includes('e')
      ? rect.x + rect.width
      : rect.x + rect.width / 2
  const top = handle.includes('n')
    ? rect.y
    : handle.includes('s')
      ? rect.y + rect.height
      : rect.y + rect.height / 2
  return { left, top }
}

/**
 * Dismisses the capture on every display.
 *
 * All of them, not just this one. Every overlay is built focused, so on a
 * multi-display setup only the last one built holds the keyboard, and closing
 * that window alone would leave the other displays covered by overlays with
 * nothing left to press Escape in.
 */
function dismissAll() {
  invoke('close_overlays').catch((error: unknown) => {
    console.error('overlay: could not close the overlays', error)
    // One display left uncovered beats every display staying covered.
    dismiss()
  })
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
