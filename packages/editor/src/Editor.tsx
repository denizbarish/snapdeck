/**
 * The editing surface: a toolbar, a canvas and the pointer and keyboard
 * plumbing between them.
 *
 * This is the only file in the package that knows about React or the DOM, and
 * it knows nothing about Tauri. A finished picture leaves through `onExport`
 * and `onCopy` as a `Blob`, so the desktop shell can write it to disk and the
 * planned browser extension can hand it to the extension APIs, and neither
 * choice reaches in here. It is also what makes this component drivable in a
 * plain browser: there is no host to stub.
 *
 * Two rules shape everything below.
 *
 * The canvas is painted by `renderDocument` and by nothing else, whether
 * directly or through `exportCanvas`, which is the same call at source
 * resolution. There is no second painter for the preview, so what the user
 * approves and what `exportCanvas` encodes cannot drift apart; that is the
 * property the whole package is arranged around. Selection outlines, resize
 * handles and the crop preview are therefore DOM elements laid over the canvas
 * rather than extra strokes on it: chrome is not part of the picture and must
 * never be able to reach the exported file.
 *
 * Zoom is a transform on the context, never a clip. `render.ts` redacts through
 * `getImageData`/`putImageData`, which address raw device pixels and ignore the
 * clip; under a clipped view an obscure layer would write its redaction outside
 * the visible region.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { CSSProperties, ChangeEvent, JSX, PointerEvent as ReactPointerEvent } from 'react'
import { TOOLBAR_MIN_WIDTH } from './chrome'
import { addLayer, History, removeLayer, setCrop, updateLayer, type Command } from './commands'
import { exportCanvas, toBlob, type ExportType } from './export'
import { handleAtPoint, layerAtPoint, moveLayer, resizeLayer, type Handle } from './hit'
import {
  boundsOf,
  createDocument,
  nextStepIndex,
  type EditorDocument,
  type Layer,
  type Point,
  type Rect,
} from './model'
import { renderDocument, viewOf } from './render'
import {
  handlePosition,
  hasResizeHandles,
  isCommittable,
  isDragTool,
  layerFor,
  measureTextRect,
  normalizeRect,
  restyleLayer,
  selectionBounds,
  clampRectToBounds,
  textSizeFor,
  withDraft,
  TEXT_FONT_STACK,
  type Gesture,
  type ObscureMode,
  type ToolName,
  type ToolSettings,
} from './tools'
import { fitViewport, toDocumentLength, toDocumentPoint, toLocalPoint, toLocalRect } from './viewport'

export type EditorProps = {
  image: CanvasImageSource
  width: number
  height: number
  /**
   * Takes the finished picture, and may name what it wrote.
   *
   * A returned string is shown in the status bar verbatim, which is how the
   * user finds out whether Save replaced the capture or left a second file
   * beside it. The editor cannot work that out for itself: it has no path, by
   * design, and the host is the only side that knows where the bytes landed.
   * Returning nothing is not an error, it simply says nothing.
   */
  onExport(blob: Blob, type: ExportType): void | string | Promise<void | string>
  onCopy(blob: Blob): void | Promise<void>
  onClose(): void
}

/**
 * The formats Save offers, in the order they appear, and the one it opens on.
 *
 * PNG first and PNG by default: the capture on disk is already a PNG, so it is
 * the format in which Save updates the file the user has rather than leaving a
 * second one beside it, and it is lossless, which is what a screenshot of text
 * wants. JPEG is the deliberate choice, for the case where the picture is going
 * to somebody over a link that will not take twelve megabytes.
 */
const FORMATS: { type: ExportType; label: string }[] = [
  { type: 'image/png', label: 'PNG' },
  { type: 'image/jpeg', label: 'JPEG' },
]
const DEFAULT_FORMAT: ExportType = 'image/png'

/**
 * The quality JPEG is encoded at.
 *
 * High, and deliberately higher than a photograph would be given, because a
 * screenshot is the worst case for this encoder rather than its best one. JPEG
 * spends its bit budget on smooth gradients and throws away the high-frequency
 * detail a photograph does not miss; a screenshot is almost entirely the thing
 * it throws away, hard edges between flat colours, and every one of those edges
 * is a glyph. The visible failure is ringing around text, which turns a
 * screenshot of a terminal into a screenshot of a terminal seen through water,
 * and it is worse on coloured text because the encoder subsamples chroma.
 *
 * 0.92 rather than 1.0: the top of the scale roughly doubles the file for
 * artefacts that are already below what the eye finds on text, which gives up
 * the entire reason somebody chose JPEG. Anything under about 0.85 starts to
 * show on thin glyph strokes, which is the one thing a screenshot is usually of.
 */
const JPEG_QUALITY = 0.92

/** What to encode `type` at. PNG is lossless and takes no quality at all. */
function qualityFor(type: ExportType): number | undefined {
  return type === 'image/jpeg' ? JPEG_QUALITY : undefined
}

/**
 * What Copy encodes, whatever the toolbar says.
 *
 * The format control is about a file: how big it is on disk and whether the
 * capture is replaced or joined by a second one. A clipboard image is neither.
 * It is handed to the next application as pixels, so encoding it as JPEG on the
 * way would throw detail away in exchange for nothing at all.
 */
const COPY_TYPE: ExportType = 'image/png'

/** `PointerEvent.button` for the left mouse button, the only one that draws. */
const PRIMARY_BUTTON = 0

/** Edge of a resize handle, and the square that grabs it, in CSS pixels. */
const HANDLE_SIZE = 9

/** How far from a layer still counts as a click on it, in CSS pixels. */
const HIT_TOLERANCE = 4

/** Gap between a layer's ink and its selection outline, in CSS pixels. */
const CHROME_PADDING = 3

/** Stroke widths the slider offers, in source-image pixels. */
const MIN_STROKE = 1
const MAX_STROKE = 24

/** Every resize handle, in clockwise order from the top-left corner. */
const HANDLES: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']

/**
 * The palette, and the one it opens on.
 *
 * Six, not a colour wheel: an annotation has to be legible against an arbitrary
 * screenshot, and a fixed set of saturated colours that a `#RRGGBB` field
 * cannot be talked out of is worth more here than free choice. The native
 * colour input is offered beside them for the case a screenshot is already red.
 */
const PALETTE = ['#ff3b30', '#ff9500', '#ffcc00', '#34c759', '#0a84ff', '#000000']
const DEFAULT_COLOR = '#ff3b30'
const DEFAULT_STROKE = 4

/** The tools, in the order they appear, with the label each is announced by. */
const TOOLS: { name: ToolName; label: string; glyph: string }[] = [
  { name: 'select', label: 'Select', glyph: '⌖' },
  { name: 'arrow', label: 'Arrow', glyph: '↗' },
  { name: 'rect', label: 'Rectangle', glyph: '▭' },
  { name: 'ellipse', label: 'Ellipse', glyph: '◯' },
  { name: 'line', label: 'Freehand line', glyph: '✎' },
  { name: 'text', label: 'Text', glyph: 'T' },
  { name: 'highlight', label: 'Highlight', glyph: '▬' },
  { name: 'obscure', label: 'Obscure', glyph: '▩' },
  { name: 'step', label: 'Step number', glyph: '①' },
  { name: 'crop', label: 'Crop', glyph: '⛶' },
]

const OBSCURE_MODES: { value: ObscureMode; label: string }[] = [
  { value: 'blur', label: 'Blur' },
  { value: 'pixelate', label: 'Pixelate' },
  { value: 'blackout', label: 'Black out' },
]

/**
 * The mode the obscure tool opens in.
 *
 * Blackout, because it is the only one of the three that destroys what it
 * covers. Measured on the packaged build, over a 1074 x 100 band of text at
 * the default intensity: blackout leaves contrast 0 and correlation 0 with the
 * original and collapses the band from 1498 colours to one, where pixelate
 * leaves 189 and 0.439 and blur leaves 107 and 0.422. All three replace every
 * source pixel, but only blackout leaves no signal behind.
 *
 * Somebody reaching for a redaction tool is covering a password, a token or a
 * face, and the default has to be the mode that cannot be undone by looking
 * harder. Blur and pixelate are one click away for the softer case, where the
 * point is that something was there rather than that it is gone.
 */
const DEFAULT_OBSCURE_MODE: ObscureMode = 'blackout'

/**
 * A gesture in progress.
 *
 * Kept in a ref rather than in state: it is written on every pointer move and
 * nothing renders from it directly, so putting it through `useState` would
 * schedule a second render per event for information only the next event
 * needs. What the user sees is the `draft` layer it produces.
 *
 * `origin` is the layer as it stood when the drag began, and every move of a
 * resize is measured from it. Deriving the anchor from the layer being resized
 * instead survives only as far as the pointer crossing the opposite edge; past
 * that the shape stops growing and starts sliding with the pointer, a defect
 * this project has already shipped and fixed twice.
 */
type GestureState =
  | { kind: 'draw'; tool: ToolName; id: string; start: Point; samples: Point[] }
  | { kind: 'move'; id: string; origin: Layer; start: Point }
  | { kind: 'resize'; id: string; origin: Layer; handle: Handle }
  | { kind: 'crop'; start: Point }

/** An open text box: where it was placed, and what has been typed into it. */
type TextSession = { origin: Point; content: string }

/**
 * A width-slider gesture in progress.
 *
 * `origin` is the selected layer as it stood when the slider was grabbed, and
 * every intermediate value is previewed against it rather than against the
 * layer as it now stands. A range input fires on every value it passes through,
 * so restyling the stored layer on each one would put a command on the undo
 * stack per pixel of travel and take twenty presses of Cmd+Z to reverse one
 * gesture. It is the same arrangement `GestureState` uses on the canvas, for
 * the same reason: one gesture, one command.
 *
 * `origin` is null when nothing was selected. The gesture is still tracked,
 * because the settings themselves move either way and only the commit is
 * conditional on there having been something to restyle.
 */
type StrokeGesture = { origin: Layer | null }

/** Which of the two things that can speak put a message in the status bar. */
type NoticeSource = 'paint' | 'deliver'
/**
 * Whether the line is a warning or a plain report of something that worked.
 *
 * The two share one line and must not look alike: a save that named the file it
 * wrote is information, and colouring it like the failures would train the user
 * to read past the failures.
 */
type NoticeTone = 'warning' | 'report'
type Notice = { source: NoticeSource; tone: NoticeTone; message: string }

export function Editor({ image, width, height, onExport, onCopy, onClose }: EditorProps): JSX.Element {
  const historyRef = useRef<History | null>(null)
  if (historyRef.current === null) historyRef.current = new History(createDocument(width, height))
  const history = historyRef.current

  const [doc, setDoc] = useState<EditorDocument>(history.document)
  const [tool, setTool] = useState<ToolName>('select')
  const [settings, setSettings] = useState<ToolSettings>({
    color: DEFAULT_COLOR,
    strokeWidth: DEFAULT_STROKE,
    obscureMode: DEFAULT_OBSCURE_MODE,
  })
  /** The format Save encodes in. Not a tool setting: it is about the file. */
  const [format, setFormat] = useState<ExportType>(DEFAULT_FORMAT)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  /** The layer a drag is building or transforming, shown in place of the stored one. */
  const [draft, setDraft] = useState<Layer | null>(null)
  /** The crop a drag is dragging out, before it is committed. */
  const [cropDraft, setCropDraft] = useState<Rect | null>(null)
  const [text, setText] = useState<TextSession | null>(null)
  /** The size of the area the canvas fills, in CSS pixels, once measured. */
  const [box, setBox] = useState({ width: 0, height: 0 })
  /**
   * The last failure worth telling the user about, or null.
   *
   * Export is the one operation here that can refuse: `getImageData` throws on
   * a tainted canvas, which is exactly the case where refusing is correct and
   * silence is not. A save that produced no file and no message would look
   * like a save.
   *
   * A message is worth clearing once the thing it warns about has stopped
   * being true, or one transient failure sits in the status bar for the rest
   * of the session, including behind later successful saves. It carries where
   * it came from because the two sources stop being true on different events
   * and share one line: painting succeeds on the very next render after a
   * failed save, so an untagged notice would clear the message about the file
   * that was never written before anybody could read it.
   */
  const [notice, setNotice] = useState<Notice | null>(null)

  /** Drops the notice if it came from `source`, and leaves any other alone. */
  const clearNotice = (source: NoticeSource): void => {
    setNotice((current) => (current?.source === source ? null : current))
  }

  const stageRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const textRef = useRef<HTMLTextAreaElement | null>(null)
  /**
   * The open text box, mirrored out of state.
   *
   * Two things end a text box and both can fire for one click: the press that
   * lands elsewhere, and the blur that press causes. Reading the session out
   * of a ref and clearing it in the same statement makes the second call a
   * no-op, where two reads of the same state value would add the layer twice.
   */
  const textSession = useRef<TextSession | null>(null)
  const gesture = useRef<GestureState | null>(null)
  /**
   * The width slider's gesture, in a ref for the same reason as the canvas's.
   *
   * It is written on the press and read on every intermediate value, and
   * nothing renders from it: what the user sees is the `draft` layer it
   * produces, exactly as during a drag on the picture.
   */
  const strokeGesture = useRef<StrokeGesture | null>(null)
  /**
   * Source of layer ids, unique within this document.
   *
   * A counter rather than a random id: it never goes backwards, so an undo
   * that puts a removed layer back can never collide with one made since, and
   * a document read in a test says which mark came first.
   */
  const nextId = useRef(0)

  // Adjusting state during render, rather than in an effect, because this is a
  // reset and not a reaction: the annotations were placed in the coordinate
  // space of the old capture, so on a new one there is nothing to keep. Doing
  // it in an effect would paint one frame of the old layers over the new image.
  const [source, setSource] = useState({ width, height })
  if (source.width !== width || source.height !== height) {
    const fresh = createDocument(width, height)
    historyRef.current = new History(fresh)
    setSource({ width, height })
    setDoc(fresh)
    setSelectedId(null)
    setDraft(null)
    setCropDraft(null)
    setText(null)
    // Both refs as well as the state they mirror: a gesture or a half-typed
    // caption from the previous capture would otherwise be committed into the
    // new one, at coordinates that mean nothing in it.
    textSession.current = null
    gesture.current = null
    strokeGesture.current = null
  }

  const view = viewOf(doc)
  const viewport = fitViewport(view, box.width, box.height)
  // The canvas element, in CSS pixels: the view at the fitted scale, placed in
  // the stage by the viewport's offsets. Everything drawn over it, the
  // selection chrome and the text box, is positioned in the same frame.
  const canvasSize = { width: view.width * viewport.scale, height: view.height * viewport.scale }
  // What the canvas shows: the document with the drag in progress standing in
  // for what is stored. Committing on every pointer move would put a hundred
  // entries on the undo stack for one drag.
  const preview = draft ? withDraft(doc, draft) : doc
  const selected = selectedId === null ? null : (preview.layers.find((layer) => layer.id === selectedId) ?? null)

  const run = (command: Command): void => {
    history.run(command)
    setDoc(history.document)
  }

  // The canvas is measured rather than given a fixed size: it fills whatever
  // the host window allows, and `fitViewport` scales the picture into it. An
  // observer rather than a resize listener, because the window is not the only
  // thing that can change this element's size.
  useLayoutEffect(() => {
    const stage = stageRef.current
    if (!stage) return
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (entry) setBox({ width: entry.contentRect.width, height: entry.contentRect.height })
    })
    observer.observe(stage)
    return () => observer.disconnect()
  }, [])

  // Paint. Before the browser lays out anything else, so the chrome positioned
  // over the canvas is never one frame ahead of the pixels it points at.
  useLayoutEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || box.width <= 0 || box.height <= 0) return
    const ctx = canvas.getContext('2d')
    if (!ctx) {
      setNotice({
        source: 'paint',
        tone: 'warning',
        message: 'This browser would not give the editor a 2D canvas.',
      })
      return
    }
    // The backing store is in device pixels and the element is in CSS pixels,
    // so the picture is drawn at the display's real resolution rather than
    // upscaled from a CSS-sized bitmap. Assigning either dimension clears the
    // canvas, so both are only written when they actually change.
    //
    // The canvas is exactly the view, never the whole stage. That is what
    // makes a crop show as a crop: everything outside it falls off the edge of
    // the bitmap, the same way `exportCanvas` gets its cropped file. The
    // alternative, a stage-sized canvas with a clip, is not open to this
    // renderer: `putImageData` ignores the clip, so an obscure layer would
    // write its redaction over the region the crop was meant to hide.
    const ratio = window.devicePixelRatio || 1
    const pixelWidth = Math.max(1, Math.round(canvasSize.width * ratio))
    const pixelHeight = Math.max(1, Math.round(canvasSize.height * ratio))
    if (canvas.width !== pixelWidth) canvas.width = pixelWidth
    if (canvas.height !== pixelHeight) canvas.height = pixelHeight

    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    // Source-image pixels in, device pixels out. `renderDocument` adds the
    // translation for the crop itself, so this is only the zoom.
    const scale = viewport.scale * ratio
    try {
      // Below one device pixel per source pixel a redaction cannot be drawn
      // faithfully in place. `render.ts` measures a mosaic block and a blur
      // radius in the device pixels of the target and rounds down, so at a
      // fitted zoom of, say, 0.4 a six-pixel block becomes two and a
      // three-pixel one becomes the identity: the file is redacted exactly as
      // asked and the preview shows the region very nearly as it was. That is
      // the safe direction, and it is still the wrong thing to put in front of
      // somebody who is about to send the file: they see an apparently
      // unredacted secret and read the tool as broken.
      //
      // So at that zoom the picture is rendered at source resolution and drawn
      // down, which is the only arrangement where what is on screen is what is
      // in the file. `exportCanvas` is the function that already does exactly
      // that, and using it rather than a second offscreen render is what keeps
      // the two from ever disagreeing.
      //
      // Only when a redaction is actually on the canvas. Every other layer is
      // vector work that the transform scales correctly, and a source-resolution
      // render costs a full-size canvas on every frame of a drag, which on a
      // retina capture in a small window is the difference between a smooth
      // gesture and a stuttering one. Redaction is the case where correctness
      // is worth those frames; nothing else is.
      if (scale < 1 && preview.layers.some((layer) => layer.kind === 'obscure')) {
        // `drawImage` downsamples with the browser's own filtering, so the
        // mosaic blocks arrive on screen as the average of themselves rather
        // than as one sampled pixel out of each.
        ctx.drawImage(exportCanvas(image, preview), 0, 0, canvas.width, canvas.height)
      } else {
        ctx.setTransform(scale, 0, 0, scale, 0, 0)
        renderDocument(ctx, image, preview)
      }
      // The warning below is about what is on screen now, so a paint that got
      // all the way here has made it untrue. Only the paint's own message is
      // dropped: a failed save is still a failed save.
      clearNotice('paint')
    } catch (error: unknown) {
      // A cross-origin image taints the canvas and `getImageData` throws from
      // inside an obscure layer. Reported rather than swallowed: the redaction
      // the user drew is not on screen, and they have to know that before they
      // send the file anywhere.
      console.error('editor: could not draw the document', error)
      setNotice({
        source: 'paint',
        tone: 'warning',
        message: 'Some layers could not be drawn. Do not treat this preview as redacted.',
      })
    }
  })

  const documentPointOf = (event: { clientX: number; clientY: number }): Point => {
    const canvas = canvasRef.current
    if (!canvas) return { x: 0, y: 0 }
    const rect = canvas.getBoundingClientRect()
    return toDocumentPoint(
      { x: event.clientX - rect.left, y: event.clientY - rect.top },
      view,
      viewport,
    )
  }

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>): void => {
    // A press on a canvas is also WebKit's gesture for selecting the document.
    // Without this the drag leaves a document selection over the picture and
    // the system selection colour is painted across it, which the capture
    // overlay learned the expensive way.
    event.preventDefault()
    if (event.button !== PRIMARY_BUTTON) return
    // A second pointer while one is already drawing would overwrite the first
    // gesture's ref: the first draft is orphaned, and the first release commits
    // whatever the second gesture was building. One pointer draws at a time.
    if (gesture.current) return
    // An open text box commits on the press that leaves it, before that press
    // is allowed to start anything else, so a click away finishes the sentence
    // rather than throwing it out.
    if (text) commitText()
    const point = documentPointOf(event)
    event.currentTarget.setPointerCapture(event.pointerId)

    if (tool === 'select') {
      // The handles first: they are drawn over the layer, so a press on one is
      // a resize and not a move, whichever is underneath.
      if (selected && hasResizeHandles(selected)) {
        const handle = handleAtPoint(selected, point, toDocumentLength(HANDLE_SIZE, viewport))
        if (handle) {
          gesture.current = { kind: 'resize', id: selected.id, origin: selected, handle }
          return
        }
      }
      const hit = layerAtPoint(preview.layers, point, toDocumentLength(HIT_TOLERANCE, viewport))
      setSelectedId(hit?.id ?? null)
      if (hit) gesture.current = { kind: 'move', id: hit.id, origin: hit, start: point }
      return
    }

    if (tool === 'crop') {
      gesture.current = { kind: 'crop', start: point }
      setCropDraft(null)
      return
    }

    if (tool === 'text') {
      openText({ origin: point, content: '' })
      return
    }

    const id = `layer-${(nextId.current += 1)}`
    // The click tools have no drag to wait for: the press is the whole gesture,
    // so the layer is committed here and the pointer is not tracked further.
    if (!isDragTool(tool)) {
      const layer = layerFor(tool, id, gestureOf(point, point, [point]), settings, nextStepIndex(doc))
      if (layer && isCommittable(layer)) run(addLayer(layer))
      return
    }
    gesture.current = { kind: 'draw', tool, id, start: point, samples: [point] }
    setDraft(layerFor(tool, id, gestureOf(point, point, [point]), settings, nextStepIndex(doc)))
  }

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>): void => {
    const active = gesture.current
    if (!active) return
    // No button held means the release was lost, which happens when the
    // pointer comes up outside the window. Acting on it would drag a shape
    // under a pointer that is only passing over.
    if (event.buttons === 0) {
      finishGesture()
      return
    }
    const point = documentPointOf(event)
    switch (active.kind) {
      case 'draw': {
        active.samples.push(point)
        setDraft(
          layerFor(
            active.tool,
            active.id,
            gestureOf(active.start, point, active.samples),
            settings,
            nextStepIndex(doc),
          ),
        )
        return
      }
      case 'move':
        setDraft(moveLayer(active.origin, point.x - active.start.x, point.y - active.start.y))
        return
      case 'resize':
        // `origin` twice: it is the anchor, and during a drag it is also the
        // layer, because nothing has been committed yet.
        setDraft(resizeLayer(active.origin, active.handle, point, active.origin))
        return
      case 'crop':
        setCropDraft(clampRectToBounds(normalizeRect(active.start, point), width, height))
        return
    }
  }

  const onPointerUp = (): void => finishGesture()

  /**
   * Ends whatever gesture was in progress and commits it, once.
   *
   * Every path that ends a drag goes through here, so no ref can outlive the
   * press that set it and steer the next pointer event. One command per
   * gesture: the draft has been the preview all along, and this is the first
   * and only time it reaches the history.
   */
  const finishGesture = (): void => {
    const active = gesture.current
    gesture.current = null
    if (!active) return
    switch (active.kind) {
      case 'draw':
        // A press that never moved leaves a zero-sized shape: invisible,
        // unhittable, and still on the undo stack, so the next Cmd+Z would
        // appear to do nothing.
        if (draft && isCommittable(draft)) run(addLayer(draft))
        break
      case 'move':
      case 'resize':
        if (draft) run(updateLayer(active.id, draft))
        break
      case 'crop':
        // A crop with no area is a stray click on the picture, not an
        // instruction to export nothing.
        if (cropDraft && cropDraft.width >= 1 && cropDraft.height >= 1) run(setCrop(cropDraft))
        break
    }
    setDraft(null)
    setCropDraft(null)
  }

  /** Opens, edits or drops the text box, keeping the ref and the state in step. */
  const openText = (session: TextSession | null): void => {
    textSession.current = session
    setText(session)
  }

  /** Measures one line of text in the font `render.ts` will paint it in. */
  const measureLine = (line: string, size: number): number => {
    const canvas = canvasRef.current
    const ctx = canvas?.getContext('2d')
    if (!ctx) return line.length * size * 0.6
    ctx.save()
    // The context carries the view transform, and `measureText` is unaffected
    // by it, so the width comes back in the font's own pixels either way. Saved
    // and restored all the same, because the font is not ours to leave behind.
    ctx.font = `${size}px ${TEXT_FONT_STACK}`
    const width = ctx.measureText(line).width
    ctx.restore()
    return width
  }

  /** Turns the open text box into a layer, or drops it if nothing was typed. */
  const commitText = (): void => {
    const session = textSession.current
    textSession.current = null
    setText(null)
    if (!session || session.content.trim() === '') return
    const size = textSizeFor(settings.strokeWidth)
    const rect = measureTextRect(session.origin, session.content, size, (line) => measureLine(line, size))
    const layer: Layer = {
      id: `layer-${(nextId.current += 1)}`,
      kind: 'text',
      rect,
      content: session.content,
      style: { color: settings.color, size, family: TEXT_FONT_STACK },
    }
    if (isCommittable(layer)) run(addLayer(layer))
  }

  /**
   * A layer restyled, with a text layer's box brought back around its ink.
   *
   * `restyleLayer` is pure and has no rasteriser, so it can move a text
   * layer's point size but not re-measure the box that size is painted in. Left
   * at the old measurement the two describe different things: `drawText` paints
   * from the rect's origin at the new size while `boundsOf` still reports the
   * old rect, so the selection outline covers a fraction of a caption the width
   * knob has just enlarged and `layerAtPoint` misses most of it. `render.ts`
   * puts the invariant as "the box follows the text, not the other way round",
   * and this is where it is kept: the measurer lives here, and the new rect
   * rides in the same command as the new size so one undo reverses both.
   */
  const restyled = (layer: Layer, next: ToolSettings): Layer => {
    const styled = restyleLayer(layer, next)
    if (styled.kind !== 'text') return styled
    const size = styled.style.size
    // The origin, not the whole rect: a restyle may resize the box but must
    // never move it, or the colour swatches become a second way to nudge things.
    const origin = { x: styled.rect.x, y: styled.rect.y }
    return {
      ...styled,
      rect: measureTextRect(origin, styled.content, size, (line) => measureLine(line, size)),
    }
  }

  /**
   * Applies the toolbar to the selection, as one reversible command.
   *
   * The settings move whether or not anything is selected, because they are
   * also what the next mark will be drawn with. With a selection they are the
   * same click: one command, one undo.
   */
  const changeSettings = (next: ToolSettings): void => {
    setSettings(next)
    if (selected) run(updateLayer(selected.id, restyled(selected, next)))
  }

  /**
   * Notes what the width slider was grabbed on, so the drag can be one command.
   *
   * Called from both the press and the key, because a range input is worked
   * with either and both stream intermediate values. Idempotent: the arrow keys
   * fire a `keydown` per press and only the first opens the gesture.
   */
  const beginStrokeChange = (): void => {
    if (strokeGesture.current === null) strokeGesture.current = { origin: selected }
  }

  /**
   * One intermediate value of the width slider.
   *
   * Inside a gesture this only moves the preview, exactly as a drag on the
   * canvas does; the command is written once, on the release. Outside one, the
   * value arrived without a press or a key behind it, so there is no gesture to
   * wait for and it commits immediately.
   */
  const changeStrokeWidth = (strokeWidth: number): void => {
    const next = { ...settings, strokeWidth }
    const active = strokeGesture.current
    if (!active) {
      changeSettings(next)
      return
    }
    setSettings(next)
    if (active.origin) setDraft(restyled(active.origin, next))
  }

  /**
   * Ends the width slider's gesture and commits it, once.
   *
   * Both the release and the blur come here, so a pointer that came up off the
   * control and a Tab out of it leave the same single entry on the undo stack.
   * A gesture that passed through no value has no draft and writes nothing.
   */
  const endStrokeChange = (): void => {
    const active = strokeGesture.current
    strokeGesture.current = null
    if (!active?.origin) return
    setDraft(null)
    if (draft && draft.id === active.origin.id) run(updateLayer(active.origin.id, draft))
  }

  const deliver = (
    send: (blob: Blob) => void | string | Promise<void | string>,
    what: string,
    type: ExportType,
  ): void => {
    // The history's document rather than the preview: a drag still in progress
    // has not been committed, and exporting it would ship a shape the user has
    // not let go of yet.
    toBlob(image, history.document, type, qualityFor(type))
      .then((blob) => {
        // Cleared only once the handover has happened, and only the delivery's
        // own message: a warning about a file that was never written has no
        // business sitting behind the save that finally worked.
        clearNotice('deliver')
        return send(blob)
      })
      .then((written) => {
        // Only when the host said where it put them. Saving as JPEG leaves a
        // second file beside the capture and saving as PNG overwrites it, and
        // the file name is the only thing that tells those two apart; a host
        // that names nothing simply leaves the line as it was.
        if (typeof written === 'string' && written.length > 0) {
          setNotice({ source: 'deliver', tone: 'report', message: `Saved ${written}` })
        }
      })
      .catch((error: unknown) => {
        console.error(`editor: could not ${what} the picture`, error)
        setNotice({
          source: 'deliver',
          tone: 'warning',
          message: `The picture could not be ${what === 'copy' ? 'copied' : 'saved'}.`,
        })
      })
  }

  /**
   * The two ways a picture leaves, named once.
   *
   * Save is reachable from the button, from Cmd+S and from Cmd+S inside an open
   * text box, and all three have to encode in the format the toolbar is showing
   * and tell the host which one that was. Writing that out three times is how
   * one of them ends up saving in yesterday's format.
   */
  const save = (): void => deliver((blob) => onExport(blob, format), 'save', format)
  const copy = (): void => deliver(onCopy, 'copy', COPY_TYPE)

  // No dependency array. The handler closes over the document, the selection
  // and the settings, all of which change on almost every render, so a
  // memoised listener would undo into a stale document or restyle a layer that
  // is no longer selected. It costs one add/removeEventListener pair per
  // render, on a window that exists for as long as one screenshot is open.
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      // Cmd on macOS, Ctrl everywhere else. The same key check the rest of the
      // app uses, so the editor answers the shortcut the platform's hand
      // reaches for.
      const meta = event.metaKey || event.ctrlKey
      const key = event.key.toLowerCase()

      // The text box owns the keyboard while it is open, or every letter typed
      // into it would also be a shortcut. Three keys still get through: Escape
      // throws the box away, Cmd+Enter finishes it, because Enter itself has to
      // stay available for a second line, and Cmd+S saves.
      //
      // Cmd+S is here because of what swallowing it would mean rather than
      // because a caption needs a save shortcut. This component is built to run
      // in a browser extension's host page, and every browser binds Cmd+S to
      // its own save-page dialog; returning early without calling
      // `preventDefault` opens that dialog over the editor, which is exactly
      // what the branch below exists to stop. Committing the box first rather
      // than dropping the key, so the file carries the sentence the user was in
      // the middle of instead of losing it to a save.
      //
      // Cmd+C is deliberately not in this list: the textarea's own copy is what
      // somebody selecting a word inside the box means by it.
      if (text) {
        if (event.key === 'Escape') {
          event.preventDefault()
          openText(null)
        } else if (meta && event.key === 'Enter') {
          event.preventDefault()
          commitText()
        } else if (meta && key === 's') {
          event.preventDefault()
          // `History.run` is synchronous, so the layer this adds is already in
          // `history.document` by the time `deliver` reads it.
          commitText()
          save()
        }
        return
      }

      if (meta && key === 'z') {
        event.preventDefault()
        if (event.shiftKey) history.redo()
        else history.undo()
        setDoc(history.document)
        // The selection may have been undone out of existence. Left alone it
        // would keep Delete and the colour swatches pointed at a layer that is
        // no longer in the document, where both quietly do nothing.
        setSelectedId((current) =>
          current !== null && history.document.layers.some((layer) => layer.id === current) ? current : null,
        )
        return
      }
      if (meta && key === 'c') {
        event.preventDefault()
        copy()
        return
      }
      if (meta && key === 's') {
        // Without this the browser's own save dialog opens over the editor.
        event.preventDefault()
        save()
        return
      }
      if (event.key === 'Delete' || event.key === 'Backspace') {
        if (!selectedId) return
        event.preventDefault()
        run(removeLayer(selectedId))
        setSelectedId(null)
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        // One press drops the selection, a second closes the editor. So the
        // key that gets out of a mistake is never the key that throws the
        // whole annotation away.
        if (selectedId !== null) setSelectedId(null)
        else onClose()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  // Focus follows the box being opened. Without it the caret stays wherever it
  // was and the first letter typed goes to the toolbar button that was clicked.
  useEffect(() => {
    if (text) textRef.current?.focus()
  }, [text !== null])

  const chrome = selected ? selectionBounds(selected, toDocumentLength(CHROME_PADDING, viewport)) : null
  const textSize = textSizeFor(settings.strokeWidth)

  return (
    <div
      data-testid="editor"
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        background: '#1c1c1e',
        color: '#f5f5f7',
        font: '13px system-ui, -apple-system, "Helvetica Neue", sans-serif',
        // The picture is the only thing on this surface worth selecting, and
        // it is a canvas, so a drag that selects the toolbar's labels is only
        // ever an accident of a drawing gesture that started too far left.
        userSelect: 'none',
      }}
    >
      <div
        role="toolbar"
        aria-label="Annotation tools"
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          flexWrap: 'wrap',
          padding: '8px 12px',
          borderBottom: '1px solid #3a3a3c',
          // `min-width` is a content-box measurement by default, so the toolbar
          // was demanding its minimum PLUS its 24 points of padding: at the
          // window's own minimum width, which is this same number, the last
          // buttons hung past the right edge and the window clipped them.
          // Close and the width readout were the two that went. With
          // `border-box` the number means the same thing on both sides of the
          // boundary, which is the only way one constant can serve both.
          boxSizing: 'border-box',
          // The one number the host also needs, so it is stated here, in the
          // component that owns the layout, and read from there by everything
          // else. Below this the right-hand group wraps onto a third row and
          // takes the room away from the picture.
          minWidth: TOOLBAR_MIN_WIDTH,
        }}
      >
        <div style={{ display: 'flex', gap: 2 }}>
          {TOOLS.map((entry) => (
            <button
              key={entry.name}
              type="button"
              data-testid={`tool-${entry.name}`}
              aria-label={entry.label}
              aria-pressed={tool === entry.name}
              title={entry.label}
              onClick={() => {
                if (text) commitText()
                setTool(entry.name)
                // The selection belongs to the select tool. Leaving it behind
                // would show handles that the tool now in hand cannot grab.
                if (entry.name !== 'select') setSelectedId(null)
              }}
              style={toolButtonStyle(tool === entry.name)}
            >
              <span aria-hidden="true">{entry.glyph}</span>
            </button>
          ))}
        </div>

        <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
          {PALETTE.map((color) => (
            <button
              key={color}
              type="button"
              data-testid={`color-${color.slice(1)}`}
              aria-label={`Colour ${color}`}
              aria-pressed={settings.color === color}
              onClick={() => changeSettings({ ...settings, color })}
              style={{
                width: 20,
                height: 20,
                borderRadius: '50%',
                background: color,
                border: settings.color === color ? '2px solid #f5f5f7' : '1px solid #48484a',
                padding: 0,
                cursor: 'pointer',
              }}
            />
          ))}
          <input
            type="color"
            data-testid="color-custom"
            aria-label="Custom colour"
            value={settings.color}
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              changeSettings({ ...settings, color: event.target.value })
            }
            style={{ width: 24, height: 24, padding: 0, background: 'none', border: 'none', cursor: 'pointer' }}
          />
        </div>

        <label style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          Width
          <input
            type="range"
            data-testid="stroke-width"
            min={MIN_STROKE}
            max={MAX_STROKE}
            step={1}
            value={settings.strokeWidth}
            aria-label="Stroke width"
            // A drag across this control is one edit and belongs on the undo
            // stack once, so the press and the key open a gesture, every value
            // in between only moves the preview, and the release and the blur
            // commit it. The swatches beside it need none of this: a colour is
            // picked in one click and fires once.
            onPointerDown={beginStrokeChange}
            onKeyDown={beginStrokeChange}
            onPointerUp={endStrokeChange}
            onBlur={endStrokeChange}
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              changeStrokeWidth(Number(event.target.value))
            }
          />
          <span data-testid="stroke-width-value" style={{ width: 20, textAlign: 'right' }}>
            {settings.strokeWidth}
          </span>
        </label>

        {/*
          Buttons rather than a `<select>`, and the reason is not taste.

          WKWebView draws a `<select>` as a real AppKit menu, and on the press
          that follows the menu's dismissal the webview delivers a `mousedown`
          to the page and no `pointerdown` at all. Measured on the packaged
          bundle with a counter on `window`: choosing a mode and then pressing
          on the picture took the page from `pd1 md1` to `pd2 md3`, the last
          event a bare `mousedown` at the press. The stage draws from
          `onPointerDown`, so that press does nothing and the user's first
          redaction after changing the mode is silently lost; the second one
          works, which is what makes it read as flakiness rather than as a bug.
          The toolbar's own buttons were unaffected throughout, because a
          `click` is built from the mousedown the page did get.

          Nothing in the page can put back an event the webview did not send,
          so the fix is to stop opening a native menu. These three buttons are
          the same control the tools and the palette already are.

          `aria-pressed` rather than a radio group, to match the tool buttons
          immediately to the left: the two are the same kind of choice and
          should be announced the same way. The visible word is hidden from
          assistive technology because the group already carries it as its
          name, and each button is named by its own label.
        */}
        <div role="group" aria-label="Obscure mode" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span aria-hidden="true">Obscure</span>
          <div style={{ display: 'flex', gap: 2 }}>
            {OBSCURE_MODES.map((mode) => (
              <button
                key={mode.value}
                type="button"
                data-testid={`obscure-${mode.value}`}
                aria-pressed={settings.obscureMode === mode.value}
                onClick={() => changeSettings({ ...settings, obscureMode: mode.value })}
                style={modeButtonStyle(settings.obscureMode === mode.value)}
              >
                {mode.label}
              </button>
            ))}
          </div>
        </div>

        <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 6 }}>
          {/*
            Buttons rather than a `<select>`, for the reason set out over the
            obscure modes above: a native menu costs the page the `pointerdown`
            of the next press on the canvas, and the canvas draws from
            `pointerdown`. A format menu sits one press away from the picture,
            so it would lose the same press in the same way.

            It carries no visible word of its own, where the obscure group
            carries "Obscure". The difference is that these two labels are the
            answer and the question at once: `PNG` and `JPEG` beside `Save` say
            what they do, while `Blur` on its own does not say what it blurs.
            The group is still named for assistive technology, by
            `aria-label`, and the toolbar keeps the width the word would have
            taken, which at the minimum window width is a row of the picture.
          */}
          <div role="group" aria-label="Save format" style={{ display: 'flex', gap: 2, marginRight: 6 }}>
            {FORMATS.map((entry) => (
              <button
                key={entry.type}
                type="button"
                data-testid={`format-${entry.label.toLowerCase()}`}
                aria-pressed={format === entry.type}
                title={`Save as ${entry.label}`}
                onClick={() => setFormat(entry.type)}
                style={modeButtonStyle(format === entry.type)}
              >
                {entry.label}
              </button>
            ))}
          </div>
          <button type="button" data-testid="copy" onClick={copy} style={actionButtonStyle}>
            Copy
          </button>
          <button
            type="button"
            data-testid="save"
            onClick={save}
            style={actionButtonStyle}
          >
            Save
          </button>
          <button type="button" data-testid="close" onClick={onClose} style={actionButtonStyle}>
            Close
          </button>
        </div>
      </div>

      <div
        ref={stageRef}
        data-testid="stage"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        style={{
          position: 'relative',
          flex: 1,
          minHeight: 0,
          overflow: 'hidden',
          // Otherwise a drag on a touch screen scrolls the page instead of
          // drawing, and the pointer events stop arriving halfway through.
          touchAction: 'none',
          cursor: tool === 'select' ? 'default' : 'crosshair',
        }}
      >
        <canvas
          ref={canvasRef}
          data-testid="canvas"
          style={{
            position: 'absolute',
            left: viewport.offsetX,
            top: viewport.offsetY,
            width: canvasSize.width,
            height: canvasSize.height,
            display: 'block',
          }}
        />

        {/*
          The chrome, in the canvas's own frame. Pinned to the canvas rather
          than to the stage so that a selection outline and a handle are placed
          by the same conversion that reads the pointer, and a crop moving the
          origin of the view moves both together.

          Never painted on the canvas. Chrome is not part of the picture, and
          `renderDocument` is the only thing allowed to write to the bitmap
          that `exportCanvas` will encode.
        */}
        <div
          data-testid="overlay"
          style={{
            position: 'absolute',
            left: viewport.offsetX,
            top: viewport.offsetY,
            width: canvasSize.width,
            height: canvasSize.height,
            // The stage below reads every press, including the ones that land
            // on a handle: `handleAtPoint` is the one answer to what the
            // pointer is over, and a second, parallel one drawn in the DOM
            // could disagree with it.
            pointerEvents: 'none',
          }}
        >
        {chrome && selected && (
          <>
            <div
              data-testid="selection"
              data-kind={selected.kind}
              style={{
                position: 'absolute',
                ...cssRect(toLocalRect(chrome, view, viewport)),
                border: '1px dashed #0a84ff',
                boxShadow: '0 0 0 1px rgba(0,0,0,0.5)',
                pointerEvents: 'none',
              }}
            />
            {/*
              Drawn where `handleAtPoint` looks for them, on the layer's own
              bounding box, and not on the inflated outline above: the target
              the eye aims at has to be the target the hit test answers, or a
              thick stroke makes every handle a near miss. The outline is
              inflated because it is the one that would otherwise disappear
              into the ink.
            */}
            {hasResizeHandles(selected) &&
              HANDLES.map((handle) => {
                const at = toLocalPoint(handlePosition(boundsOf(selected), handle), view, viewport)
                return (
                  <div
                    key={handle}
                    data-testid={`handle-${handle}`}
                    style={{
                      position: 'absolute',
                      left: at.x - HANDLE_SIZE / 2,
                      top: at.y - HANDLE_SIZE / 2,
                      width: HANDLE_SIZE,
                      height: HANDLE_SIZE,
                      background: '#fff',
                      border: '1px solid #0a84ff',
                      boxSizing: 'border-box',
                      cursor: `${handle}-resize`,
                      // The press is read by the stage's own handler, which
                      // asks `handleAtPoint` where the pointer is. A handle
                      // that swallowed its own press would need a second,
                      // parallel answer to the same question.
                      pointerEvents: 'none',
                    }}
                  />
                )
              })}
          </>
        )}

        {cropDraft && (
          <div
            data-testid="crop-draft"
            style={{
              position: 'absolute',
              ...cssRect(toLocalRect(cropDraft, view, viewport)),
              // A spread shadow rather than a second dimming sheet: the region
              // being kept has to stay true to what will be exported, and a
              // sheet over everything would dim that too.
              boxShadow: '0 0 0 9999px rgba(0,0,0,0.45)',
              outline: '1px solid #fff',
              pointerEvents: 'none',
            }}
          />
        )}

        {text && (
          <textarea
            ref={textRef}
            data-testid="text-input"
            aria-label="Annotation text"
            value={text.content}
            onChange={(event: ChangeEvent<HTMLTextAreaElement>) =>
              openText({ origin: text.origin, content: event.target.value })
            }
            onBlur={commitText}
            // The box must not be a drawing surface: a press inside it is a
            // caret placement, and letting it reach the stage would commit the
            // box and start a mark under the pointer.
            onPointerDown={(event) => event.stopPropagation()}
            style={{
              position: 'absolute',
              ...(() => {
                const at = toLocalPoint(text.origin, view, viewport)
                return { left: at.x, top: at.y }
              })(),
              // Shown at the size and colour it will be painted at, so the box
              // is a preview of the layer rather than a form field over it.
              font: `${textSize * viewport.scale}px ${TEXT_FONT_STACK}`,
              lineHeight: 1.25,
              color: settings.color,
              background: 'rgba(0,0,0,0.25)',
              border: '1px dashed #0a84ff',
              outline: 'none',
              padding: 0,
              margin: 0,
              minWidth: 120,
              minHeight: textSize * viewport.scale * 1.25,
              resize: 'none',
              overflow: 'hidden',
              userSelect: 'text',
              // The one part of the chrome that is not chrome: it takes the
              // caret and the keystrokes.
              pointerEvents: 'auto',
            }}
          />
        )}
        </div>
      </div>

      <div
        data-testid="status"
        style={{
          display: 'flex',
          gap: 12,
          padding: '4px 12px',
          borderTop: '1px solid #3a3a3c',
          color: '#98989d',
          font: '12px ui-monospace, monospace',
        }}
      >
        <span data-testid="layer-count">{doc.layers.length} annotations</span>
        <span data-testid="selected-kind">{selected ? `${selected.kind} selected` : 'nothing selected'}</span>
        <span data-testid="view-size">
          {Math.round(view.width)} × {Math.round(view.height)}
        </span>
        {/* The shortcuts are not discoverable any other way, and this editor
            has no menu bar of its own to put them in. */}
        <span style={{ marginLeft: 'auto' }}>⌘Z undo · ⌘C copy · ⌘S save · Esc close</span>
        {notice && (
          <span
            data-testid="notice"
            role="status"
            // The report is the surface's own foreground rather than another
            // grey readout, so a save that named its file is legible without
            // borrowing the colour that means something went wrong.
            style={{ color: notice.tone === 'warning' ? '#ff9f0a' : '#f5f5f7' }}
          >
            {notice.message}
          </span>
        )}
      </div>
    </div>
  )
}

/** A gesture literal, so the three call sites cannot disagree about its shape. */
function gestureOf(start: Point, current: Point, samples: Point[]): Gesture {
  return { start, current, samples }
}

/** A rect as absolute-position CSS. */
function cssRect(rect: Rect): { left: number; top: number; width: number; height: number } {
  return { left: rect.x, top: rect.y, width: rect.width, height: rect.height }
}

function toolButtonStyle(active: boolean): CSSProperties {
  return {
    width: 30,
    height: 26,
    borderRadius: 5,
    border: '1px solid transparent',
    background: active ? '#0a84ff' : '#2c2c2e',
    color: '#f5f5f7',
    font: '15px system-ui, -apple-system, sans-serif',
    cursor: 'pointer',
  }
}

/**
 * A toolbar button that carries a word: an obscure mode, or a save format.
 *
 * `toolButtonStyle`'s height and its active blue, because all three groups are
 * the same kind of choice and sit on the same row; the width is the label's
 * rather than fixed, because these carry words and the tools carry a glyph.
 */
function modeButtonStyle(active: boolean): CSSProperties {
  return {
    height: 26,
    padding: '0 8px',
    borderRadius: 5,
    border: '1px solid transparent',
    background: active ? '#0a84ff' : '#2c2c2e',
    color: '#f5f5f7',
    font: 'inherit',
    cursor: 'pointer',
  }
}

const actionButtonStyle: CSSProperties = {
  padding: '4px 10px',
  borderRadius: 5,
  border: '1px solid #48484a',
  background: '#2c2c2e',
  color: '#f5f5f7',
  font: 'inherit',
  cursor: 'pointer',
}
