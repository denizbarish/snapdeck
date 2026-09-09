import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { useEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent, SyntheticEvent } from 'react'
import { magnifierSourceRect, samplePixel, toHex, type Rgba } from './magnifier'
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
   * `'window'` picks whole windows by hovering them, `'display'` starts with
   * the whole display selected, and anything else drags a free region. Written
   * by Rust into the overlay URL, so the mode never changes under a live
   * overlay.
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

/** The mode that arrives with the whole display already selected. */
const DISPLAY_MODE = 'display'

/** CSS pixels the magnifier gives each display point. */
const MAGNIFIER_ZOOM = 8

/** Edge of the region the magnifier shows, in display points. */
const MAGNIFIER_SOURCE = 16

/** Edge of the magnifier itself, in CSS pixels. */
const MAGNIFIER_SIZE = MAGNIFIER_SOURCE * MAGNIFIER_ZOOM

/** Gap between the pointer and the magnifier, in CSS pixels. */
const MAGNIFIER_GAP = 16

/** Height of the hex readout under the magnifier, in CSS pixels. */
const MAGNIFIER_READOUT_HEIGHT = 32

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
  /**
   * The frozen frame's pixels, in device pixels, once it has been decoded.
   *
   * A ref rather than state: it is written once, nothing renders differently
   * because of it, and putting 30 MB of image data through `useState` would
   * re-render the overlay for an event the user cannot see.
   */
  const pixels = useRef<{ data: Uint8ClampedArray; width: number } | null>(null)
  /**
   * Width of the decoded frame in device pixels, once there is one.
   *
   * The same number `pixels.current.width` holds, mirrored into state because
   * the magnifier needs it while rendering and a ref read during render is not
   * a subscription: the buffer arrives after the first paint, and nothing would
   * re-render to pick it up. Only the width is duplicated, never the buffer.
   */
  const [frameWidth, setFrameWidth] = useState<number | null>(null)
  /**
   * Flipped once the backdrop has loaded and the window has been shown.
   *
   * The gate on the offscreen decode below. Rust gives the overlay a fixed
   * deadline to become visible, and the decode is a second full read of the
   * same multi-megabyte file plus a full-frame `getImageData` copy, all on the
   * main thread. Started on mount it competes with the backdrop load inside
   * that deadline for a feature nobody can use until the window is on screen.
   */
  const [revealed, setRevealed] = useState(false)
  /** Where the pointer is, in display points, or null before it has moved. */
  const [pointer, setPointer] = useState<Point | null>(null)
  /** Colour of the device pixel under the pointer, or null until one is read. */
  const [hoverColor, setHoverColor] = useState<Rgba | null>(null)

  // Built once per render and shared by the backdrop, the magnifier's zoomed
  // view and the offscreen decode, so all three name the same URL and the
  // webview is asked for the file under one name.
  const frozenSrc = convertFileSrc(framePath)

  // The window is exactly one display, so the viewport is the display and the
  // selection may go anywhere in it. Read every render rather than cached: a
  // display that changes resolution while the overlay is up would otherwise
  // clamp against a size that no longer exists.
  const bounds = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }

  // The magnifier reads pixels, and pixels only come out of a canvas, so the
  // frame is decoded a second time into an offscreen one. Once per overlay: the
  // screen is frozen for the whole life of this window, so the buffer can never
  // go stale, and `getImageData` on a full display costs about 30 MB and a
  // full-frame copy that has no business running on every pointer move.
  //
  // Deliberately not on mount. This is the second read and the second decode of
  // the same file, and because it is a CORS-mode request while the backdrop is
  // no-cors it cannot be served from the backdrop's response: two reads, two
  // decodes and a 30 MB copy, all on the main thread. Rust times the reveal and
  // closes an overlay that misses its deadline, so gating on `revealed` puts
  // every byte of this after the window is already up. The magnifier appears a
  // fraction of a second into an overlay that lives for seconds, which is the
  // cheap half of the trade.
  //
  // `crossOrigin` is the load-bearing line, and it was measured rather than
  // assumed. The frozen frame is served over Tauri's asset protocol from
  // `asset://localhost`, which is a different origin from the page, so a plain
  // load taints the canvas and `getImageData` throws
  // `SecurityError: The operation is insecure`. Tauri's asset protocol already
  // answers with `Access-Control-Allow-Origin` set to this window's own origin;
  // `crossOrigin = 'anonymous'` is what makes the webview perform a CORS-mode
  // fetch so that header is honoured, and the canvas stays readable. It relaxes
  // nothing: without it the response header is simply ignored.
  useEffect(() => {
    if (!revealed) return
    const image = new Image()
    image.crossOrigin = 'anonymous'
    // An image load cannot be cancelled, so the cleanup revokes the handlers
    // instead and this flag makes that decision stick. `StrictMode` mounts the
    // effect twice in development, and without it the first load would still be
    // in flight when the second starts and would write its buffer over the
    // newer one on arrival.
    let abandoned = false
    image.onload = () => {
      if (abandoned) return
      const canvas = document.createElement('canvas')
      canvas.width = image.naturalWidth
      canvas.height = image.naturalHeight
      const context = canvas.getContext('2d', { willReadFrequently: true })
      if (!context) {
        console.error('overlay: no 2d context, the magnifier will stay hidden')
        return
      }
      context.drawImage(image, 0, 0)
      const buffer = context.getImageData(0, 0, canvas.width, canvas.height)
      pixels.current = { data: buffer.data, width: canvas.width }
      setFrameWidth(canvas.width)
    }
    // The backdrop's own `onError` already closes the overlay when the file is
    // missing, so this is here for the case the two loads disagree, which is
    // exactly the CORS one above. Without it a refused load leaves a magnifier
    // that silently never appears and nothing anywhere saying why.
    image.onerror = () => {
      if (abandoned) return
      console.error(`overlay: could not decode ${frozenSrc} for the magnifier`)
    }
    image.src = frozenSrc
    return () => {
      abandoned = true
      image.onload = null
      image.onerror = null
    }
  }, [frozenSrc, revealed])

  /**
   * Records where the pointer is and what colour is under it.
   *
   * Called at the top of every pointer move, before the two modes split. The
   * magnifier belongs to neither of them and both return early, so anywhere
   * further down would leave it frozen in one mode or the other.
   */
  const trackPointer = (x: number, y: number) => {
    setPointer({ x, y })
    const frame = pixels.current
    // The pointer is in CSS points and the frozen frame is in device pixels.
    // The conversion is measured from the frame that was actually decoded, not
    // taken from the `scale` in the URL: the frame is exactly `bounds.width`
    // points wide by definition, so `frame.width / bounds.width` is the ratio
    // by construction, where `scale` is only equal to it as long as the two
    // agree. Task 6 kept the frame unresampled and lossless precisely so the
    // pixel it lands on is the pixel the user is pointing at rather than an
    // average of the ones around it, and sampling with a ratio that is merely
    // probably right would give that away for a wrong colour with no symptom.
    const ratio = frame ? frame.width / bounds.width : 0
    setHoverColor(
      frame ? samplePixel(frame.data, frame.width, { x: x * ratio, y: y * ratio }) : null,
    )
  }

  /** Puts the hex under the pointer on the clipboard. */
  const copyHex = (color: Rgba) => {
    // Handled rather than merely marked with `void`: this is a menu bar agent
    // with no console in a release build, and a colour that silently did not
    // reach the clipboard is indistinguishable from one that did.
    writeText(toHex(color)).catch((error: unknown) => {
      console.error('overlay: could not copy the colour to the clipboard', error)
    })
  }

  const confirm = (rect: Rect) => {
    // Enter on a selection too small to be worth capturing does nothing, and
    // the overlay stays up so the user can fix it. Ending the whole session
    // here would answer a deliberate keystroke with no file, no message and no
    // screen to try again on. Only Escape and a real capture dismiss.
    if (!isUsable(rect)) return
    // Display-local points; converting them to the global space is the
    // command's job, because only Rust knows where this display sits.
    //
    // Nothing is dismissed here on the way out, and nothing awaits the result
    // either. `capture_region` closes the overlays itself and has to: it
    // re-captures the region at native resolution rather than cropping the
    // frozen frame, so it cannot start until this window is off the screen,
    // and by the time it answers there is no webview left to answer to.
    invoke('capture_region', { displayId, rect }).catch((error: unknown) => {
      console.error(`overlay: could not capture the selection on display ${displayId}`, error)
      // Reachable for exactly the two failures that happen while this window is
      // still on screen: the refused capture slot, and the dismissal that timed
      // out, which is the one that leaves the overlays up. Everything after
      // that point has already destroyed this webview, so its rejection is
      // delivered to nobody and this never runs; the cleanup those paths need
      // is Rust's, not this one's. Where it does run, leaving every display
      // covered under a selection the user already confirmed is the one
      // outcome worse than a missing file.
      dismissAll()
    })
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

  // Full-screen mode is region mode with the selection already made: the whole
  // display is selected the moment the overlay appears, so `Enter` captures it
  // and `Esc` cancels, the same two keys the other two modes end on. That is
  // the entire difference between the modes, which is why there is no third
  // branch anywhere below: the drag, the handles and the arrow keys all keep
  // working on the preselected rect, so a full-screen shortcut pressed by
  // mistake is one drag away from being a region.
  //
  // On mount, and again on every `focus`. Mount alone is not enough on more
  // than one display: every overlay is built focused and each one asks for
  // focus again once its frame has loaded, so on a two-display setup one of
  // them loses that race and receives `blur`, and the blur handler below clears
  // its selection. With a mount-only effect that display's preselection was
  // gone for good, `Enter` did nothing on it, and the only way back was a click
  // that region mode turns into an empty rect. Re-applying on focus costs a
  // trimmed selection that the blur had already thrown away, and it is the same
  // rect the mode starts with, so the display the user just moved to is always
  // the one the README describes: whole display selected, `Enter` captures it.
  useEffect(() => {
    if (mode !== DISPLAY_MODE) return
    const preselect = () =>
      setSelection({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight })
    preselect()
    window.addEventListener('focus', preselect)
    return () => window.removeEventListener('focus', preselect)
  }, [mode])

  // No dependency array on purpose. The handler closes over `selection` and
  // `bounds`, both of which change on almost every render, so a memoised
  // listener would nudge a stale rect. Re-subscribing costs one
  // add/removeEventListener pair per render on a window that exists for a few
  // seconds.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') return dismissAll()
      // Before the selection guard, because the colour picker works on bare
      // desktop: there is nothing to select for a colour to belong to, and a
      // magnifier showing a hex that only some of the time can be copied would
      // be worse than no key at all. No modifier check, so Cmd-C reaches it
      // too, which is the shortcut a hand goes to for "copy this" anyway.
      if (event.key.toLowerCase() === 'c') {
        if (hoverColor) copyHex(hoverColor)
        return
      }
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
      // The magnifier goes with it. The pointer events that would move it are
      // going somewhere else now, so what is left is a frozen swatch claiming
      // to be the colour under a cursor it can no longer see, and a `C` that
      // would copy it. The next pointer move over this display brings it back.
      setPointer(null)
      setHoverColor(null)
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
      // Only now is the magnifier's own decode allowed to start. Rust's reveal
      // deadline has been met by this point, so the second read of the frame
      // can no longer be the reason the window missed it.
      .then(() => setRevealed(true))
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
    // The backdrop is one <img>, so to WebKit a press-and-drag on it is both
    // this app's marquee gesture and the platform's gesture for selecting the
    // document. Without this the drag leaves a document selection containing
    // the frozen frame, and WebKit paints the system selection colour over the
    // whole image: #010000 renders as #324A63, #FF0000 as #BA82A5, inside the
    // marquee as well as outside it. The `user-select: none` in overlay.html
    // does not keep an image out of a selection.
    event.preventDefault()
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
    trackPointer(event.clientX, event.clientY)
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
        src={frozenSrc}
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
                    // stopPropagation means this press never reaches the
                    // backdrop's handler, so it needs its own guard against
                    // WebKit selecting the frozen frame. Same tint otherwise.
                    event.preventDefault()
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
      {/*
        Last child, so the magnifier paints over the dim sheet, the selection
        outline and the handles. It is the one thing on screen that is meant to
        be read pixel for pixel, and anything drawn on top of it would be
        reporting a colour it was covering.
      */}
      {pointer && hoverColor && frameWidth !== null && (
        <Magnifier
          frozenSrc={frozenSrc}
          pointer={pointer}
          color={hoverColor}
          bounds={bounds}
          // The same ratio `trackPointer` sampled with, so the marker lands on
          // the pixel whose colour is in the readout. Deriving one of them from
          // the frame and the other from the URL's `scale` would let the two
          // drift apart on any display where they disagree.
          pixelsPerPoint={frameWidth / bounds.width}
        />
      )}
    </div>
  )
}

type MagnifierProps = {
  frozenSrc: string
  pointer: Point
  color: Rgba
  bounds: Rect
  /** Device pixels of the decoded frame per display point. */
  pixelsPerPoint: number
}

/**
 * The zoomed view of the frozen frame under the pointer, and the hex it reads.
 *
 * The zoom is a background image rather than a canvas because the frame is
 * already decoded for the backdrop: naming the same URL reuses that decode,
 * where a canvas would mean a third copy of a full-display image and a redraw
 * on every pointer move. `background-size` stretches the frame so one display
 * point is `MAGNIFIER_ZOOM` CSS pixels, and `background-position` slides the
 * source region under the clip.
 */
function Magnifier({ frozenSrc, pointer, color, bounds, pixelsPerPoint }: MagnifierProps) {
  const source = magnifierSourceRect(pointer, MAGNIFIER_SOURCE, bounds)
  const { left, top } = magnifierPlacement(pointer, bounds)
  // The marked pixel is the one `samplePixel` was asked for, recomputed the
  // same way, and not the middle of the view. Near an edge `magnifierSourceRect`
  // clamps, and there the pointer is not at the centre; a marker fixed to the
  // middle would then point at a pixel whose colour is not the one on show.
  const marker = {
    left: (Math.floor(pointer.x * pixelsPerPoint) / pixelsPerPoint - source.x) * MAGNIFIER_ZOOM,
    top: (Math.floor(pointer.y * pixelsPerPoint) / pixelsPerPoint - source.y) * MAGNIFIER_ZOOM,
    // One device pixel, which is what a sample is. On a Retina display that is
    // half of what one display point occupies in the zoomed view.
    size: MAGNIFIER_ZOOM / pixelsPerPoint,
  }

  return (
    <div style={{ position: 'absolute', left, top, pointerEvents: 'none' }}>
      <div
        style={{
          position: 'relative',
          width: MAGNIFIER_SIZE,
          height: MAGNIFIER_SIZE,
          backgroundImage: `url("${frozenSrc}")`,
          backgroundRepeat: 'no-repeat',
          backgroundSize: `${bounds.width * MAGNIFIER_ZOOM}px ${bounds.height * MAGNIFIER_ZOOM}px`,
          backgroundPosition: `${-source.x * MAGNIFIER_ZOOM}px ${-source.y * MAGNIFIER_ZOOM}px`,
          // Without this the webview smooths the enlargement and the magnifier
          // shows a blurred average of the pixels instead of the pixels, while
          // the hex underneath still reports a single one of them.
          imageRendering: 'pixelated',
          outline: '1px solid #fff',
        }}
      >
        <div
          style={{
            position: 'absolute',
            left: marker.left,
            top: marker.top,
            width: marker.size,
            height: marker.size,
            // Both rings are drawn outside the box, so the pixel being reported
            // stays visible inside them. The dark one is what keeps the white
            // one findable over a light pixel.
            outline: '1px solid #fff',
            boxShadow: '0 0 0 2px rgba(0,0,0,0.55)',
          }}
        />
      </div>
      <div
        style={{
          boxSizing: 'border-box',
          width: MAGNIFIER_SIZE,
          height: MAGNIFIER_READOUT_HEIGHT,
          padding: '2px 6px',
          background: '#000',
          color: '#fff',
          font: '12px ui-monospace, monospace',
          lineHeight: '15px',
          textAlign: 'center',
        }}
      >
        {toHex(color)}
        {/* Same reason the size readout spells out Enter and Esc: a keystroke
            nothing on screen mentions is a feature nobody finds. */}
        <div style={{ opacity: 0.7, fontSize: 10, lineHeight: '11px' }}>C to copy</div>
      </div>
    </div>
  )
}

/**
 * Where the magnifier sits, in display points.
 *
 * Beside the pointer, and it flips to the other side rather than sliding along
 * an edge: a magnifier pushed back from the right edge would end up underneath
 * the pointer, hiding the pixels it exists to show.
 */
function magnifierPlacement(pointer: Point, bounds: Rect): { left: number; top: number } {
  const height = MAGNIFIER_SIZE + MAGNIFIER_READOUT_HEIGHT
  const right = pointer.x + MAGNIFIER_GAP
  const below = pointer.y + MAGNIFIER_GAP
  const left =
    right + MAGNIFIER_SIZE <= bounds.x + bounds.width
      ? right
      : pointer.x - MAGNIFIER_GAP - MAGNIFIER_SIZE
  const top =
    below + height <= bounds.y + bounds.height ? below : pointer.y - MAGNIFIER_GAP - height
  // A display narrower than the magnifier is not a real case; the clamp is
  // here so a flipped box cannot start off the top-left of the screen.
  return { left: Math.max(bounds.x, left), top: Math.max(bounds.y, top) }
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
