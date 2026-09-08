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
import { toLocalRect, windowUnderPoint, type WindowBounds } from './snap'

export interface OverlayProps {
  displayId: number
  /**
   * `'window'` picks whole windows by hovering them; anything else drags a
   * free region. Written by Rust into the overlay URL, so the two modes never
   * change under a live overlay.
   */
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

/** `PointerEvent.button` for the left mouse button, the only one that draws. */
const PRIMARY_BUTTON = 0

/** Dimming applied to everything outside the selection. */
const DIM = 'rgba(0,0,0,0.35)'

/** The mode that snaps to whole windows instead of dragging a region. */
const WINDOW_MODE = 'window'

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
 * Two ways to end up with a selection, and only one is live at a time. Region
 * mode drags one out and then lets it be adjusted; window mode snaps it to
 * whatever window is under the pointer and takes a click. The branches are
 * kept apart at the top of each handler rather than blended, because the two
 * gestures disagree about what a press and a move mean.
 *
 * Everything the selection is allowed to do lives in `./selection` and
 * `./snap`, both pure and unit tested. What is left here is event plumbing and
 * paint.
 */
export function Overlay({ displayId, mode, scale, framePath }: OverlayProps) {
  const snapsToWindows = mode === WINDOW_MODE
  const [selection, setSelection] = useState<Rect | null>(null)
  /** Candidate windows, in global points and in the order `list_windows` gave. */
  const [windows, setWindows] = useState<WindowBounds[]>([])
  /** Where this display sits in the global point space. */
  const [displayOrigin, setDisplayOrigin] = useState<Point>({ x: 0, y: 0 })
  /** Where the current drag began, or `null` when no drag is in progress. */
  const dragStart = useRef<Point | null>(null)
  /** The handle being dragged, or `null` when the pointer is not on one. */
  const activeHandle = useRef<Handle | null>(null)
  /**
   * The selection as it was when the active handle was pressed. Every move of
   * that handle is measured from this rect and never from the live selection,
   * which is what keeps the three edges the user is not dragging anchored.
   */
  const resizeOrigin = useRef<Rect | null>(null)

  // The window is exactly one display, so the viewport is the display and the
  // selection may go anywhere in it. Read every render rather than cached: a
  // display that changes resolution while the overlay is up would otherwise
  // clamp against a size that no longer exists.
  const bounds = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }

  const confirm = (rect: Rect) => {
    // Enter on a selection too small to be worth capturing does nothing, and
    // the overlay stays up so the user can fix it. Ending the whole session
    // here would answer a deliberate keystroke with no file, no message and no
    // screen to try again on. Only Escape and a real capture dismiss.
    if (!isUsable(rect)) return
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

  /**
   * Forgets whatever gesture was in progress. Every path that ends one goes
   * through here, so no ref can outlive the press that set it and silently
   * steer the next pointer event.
   */
  const endGesture = () => {
    dragStart.current = null
    activeHandle.current = null
    resizeOrigin.current = null
  }

  // Fetched once, when a window-mode overlay mounts, and never again. The
  // screen is frozen for the whole life of this overlay, so the windows cannot
  // move or restack underneath it; asking again on every pointer move would
  // pay two ScreenCaptureKit round trips per mouse movement to be told the
  // same thing. The list arrives after the first paint, so hovering during
  // that window highlights nothing, which is the same as hovering empty
  // desktop and needs no separate state.
  useEffect(() => {
    if (!snapsToWindows) return
    invoke<{ windows: WindowBounds[]; origin: Point }>('list_windows', { displayId })
      .then((result) => {
        setWindows(result.windows)
        setDisplayOrigin(result.origin)
      })
      // Not fatal: the overlay stays up with nothing to snap to, and Escape
      // still works. Closing it would answer a failed enumeration by throwing
      // away the frozen frame the user is looking at.
      .catch((error: unknown) => {
        console.error(`overlay: could not list the windows on display ${displayId}`, error)
      })
  }, [displayId, snapsToWindows])

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
      if (!delta) return
      // Otherwise the arrow keys scroll the page under the selection. Swallowed
      // in both modes, and before the window-mode return below: the frozen
      // frame is a full-bleed image, and an arrow key that reaches the document
      // scrolls it away from the screen it is standing in for, whether or not
      // there is anything to nudge.
      event.preventDefault()
      // Nothing to nudge in window mode: the selection is a window's own
      // rectangle, and the next pointer move recomputes it from scratch, so a
      // moved rect would either be discarded or capture a region that is no
      // longer the window it is drawn around.
      if (snapsToWindows) return
      setSelection(nudgeRect(selection, delta[0], delta[1], bounds))
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  // At most one selection exists across all displays. Each overlay keeps its
  // own, but only the focused window receives Enter, so a rect left behind on
  // another display would sit there promising a capture that its own keystroke
  // cannot reach. Losing focus also ends any gesture: the pointer events that
  // would have finished it are going somewhere else now.
  //
  // The refs and `setSelection` are stable, so the empty dependency array is
  // honest here, unlike the keyboard effect above.
  useEffect(() => {
    const onBlur = () => {
      endGesture()
      setSelection(null)
    }
    window.addEventListener('blur', onBlur)
    return () => window.removeEventListener('blur', onBlur)
  }, [])

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
    // A press starts nothing in window mode. The selection is whatever the
    // pointer is over, so opening a drag here would replace a highlighted
    // window with a zero-size rect at the press and then follow the pointer
    // instead of the windows.
    if (snapsToWindows) return
    // Only the left button draws. Any other one would throw the current
    // selection away and open a drag that its own release never ends, because
    // a right-click on the backdrop is not an instruction to select anything.
    if (event.button !== PRIMARY_BUTTON) return
    dragStart.current = { x: event.clientX, y: event.clientY }
    setSelection({ x: event.clientX, y: event.clientY, width: 0, height: 0 })
  }

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    // Checked before the button guard below: hovering is the whole gesture in
    // window mode, and it happens with no button held.
    if (snapsToWindows) {
      // The pointer is display-local and the windows are global, so one of
      // them has to move into the other's space. The pointer goes global, one
      // addition per event, instead of rebasing every window in the list.
      const hit = windowUnderPoint(windows, {
        x: event.clientX + displayOrigin.x,
        y: event.clientY + displayOrigin.y,
      })
      // Clamped because a window may hang off this display, and the overlay
      // may only offer what its own display can capture.
      setSelection(hit ? clampRect(toLocalRect(hit.bounds, displayOrigin), bounds) : null)
      return
    }
    // No button held means this move is not part of a gesture. If one is still
    // recorded its release was lost, which happens when the pointer comes up
    // outside the window, and acting on it would resize the selection under a
    // pointer that is only passing over.
    if (event.buttons === 0) {
      endGesture()
      return
    }
    const pointer = { x: event.clientX, y: event.clientY }
    // A handle drag is checked first: it starts without touching `dragStart`,
    // so the two are never both live.
    if (activeHandle.current && resizeOrigin.current) {
      // Measured from the rect the handle was pressed on, never from the live
      // selection. Feeding the current rect back in destroys the anchor the
      // moment the pointer crosses the opposite edge: the edge that was
      // anchored becomes wherever the pointer was one event ago, so the
      // selection stops growing and starts sliding, as wide as the distance
      // between two move events.
      setSelection(resizeRect(resizeOrigin.current, activeHandle.current, pointer, bounds))
      return
    }
    if (!dragStart.current) return
    setSelection(clampRect(normalizeRect(dragStart.current, pointer), bounds))
  }

  // In region mode, releasing the pointer ends the drag or the resize and
  // confirms nothing. The selection stays adjustable, so the eight handles and
  // the arrow keys are reachable: Enter captures it, Escape cancels. Capturing
  // on release would make all three dead UI, because the overlay would already
  // be gone by the time the user reached for them. Window mode has none of
  // those three to protect, which is why it does capture on release.
  const onPointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    // A click picks the highlighted window. On the release rather than the
    // press, so the gesture is still a click the user can back out of by
    // moving off the window, and so a stray press does not capture before the
    // highlight has been read.
    if (snapsToWindows) {
      if (event.button === PRIMARY_BUTTON && selection) confirm(selection)
      return
    }
    endGesture()
    // A press with no drag, or a resize collapsed onto itself, leaves a rect
    // with no area. Nothing confirms it any more, so it would stay on screen
    // as a one-pixel outline with all eight handles stacked into a single
    // white square and a readout saying `0 × 0`. Dropping it restores the
    // plain dim sheet, which is what a stray click should leave behind.
    setSelection((current) => (current && isUsable(current) ? current : null))
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
            <span style={{ opacity: 0.7, marginLeft: 8 }}>
              {snapsToWindows ? 'Click to capture' : 'Enter to capture'} · Esc to cancel
            </span>
          </div>
          {/*
            No handles in window mode. The rectangle belongs to a window rather
            than to the user, and the next pointer move replaces it, so a
            handle could never be dragged anywhere; all it would do is sit on
            the outline swallowing the click that picks the window.
          */}
          {!snapsToWindows &&
            HANDLES.map((handle) => {
              const position = handlePosition(selection, handle)
              return (
                <div
                  key={handle}
                  // Without this the press also reaches the backdrop and starts
                  // a fresh drag, which throws away the selection being
                  // resized.
                  onPointerDown={(event) => {
                    event.stopPropagation()
                    if (event.button !== PRIMARY_BUTTON) return
                    activeHandle.current = handle
                    // The anchor for this entire resize, frozen at the press.
                    resizeOrigin.current = selection
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
