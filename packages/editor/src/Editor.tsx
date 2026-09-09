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
 * The canvas is painted by `renderDocument` and by nothing else. There is no
 * second painter for the preview, so what the user approves and what
 * `exportCanvas` encodes cannot drift apart; that is the property the whole
 * package is arranged around. Selection outlines, resize handles and the crop
 * preview are therefore DOM elements laid over the canvas rather than extra
 * strokes on it: chrome is not part of the picture and must never be able to
 * reach the exported file.
 *
 * Zoom is a transform on the context, never a clip. `render.ts` redacts through
 * `getImageData`/`putImageData`, which address raw device pixels and ignore the
 * clip; under a clipped view an obscure layer would write its redaction outside
 * the visible region.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { CSSProperties, ChangeEvent, JSX, PointerEvent as ReactPointerEvent } from 'react'
import { addLayer, History, removeLayer, setCrop, updateLayer, type Command } from './commands'
import { toBlob } from './export'
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
  onExport(blob: Blob, type: string): void | Promise<void>
  onCopy(blob: Blob): void | Promise<void>
  onClose(): void
}

/** What the two callbacks are handed. The only format this editor writes. */
const EXPORT_TYPE = 'image/png'

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

export function Editor({ image, width, height, onExport, onCopy, onClose }: EditorProps): JSX.Element {
  const historyRef = useRef<History | null>(null)
  if (historyRef.current === null) historyRef.current = new History(createDocument(width, height))
  const history = historyRef.current

  const [doc, setDoc] = useState<EditorDocument>(history.document)
  const [tool, setTool] = useState<ToolName>('select')
  const [settings, setSettings] = useState<ToolSettings>({
    color: DEFAULT_COLOR,
    strokeWidth: DEFAULT_STROKE,
    obscureMode: 'blur',
  })
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
   */
  const [notice, setNotice] = useState<string | null>(null)

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
      setNotice('This browser would not give the editor a 2D canvas.')
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
    ctx.setTransform(scale, 0, 0, scale, 0, 0)
    try {
      renderDocument(ctx, image, preview)
    } catch (error: unknown) {
      // A cross-origin image taints the canvas and `getImageData` throws from
      // inside an obscure layer. Reported rather than swallowed: the redaction
      // the user drew is not on screen, and they have to know that before they
      // send the file anywhere.
      console.error('editor: could not draw the document', error)
      setNotice('Some layers could not be drawn. Do not treat this preview as redacted.')
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
   * Applies the toolbar to the selection, as one reversible command.
   *
   * The settings move whether or not anything is selected, because they are
   * also what the next mark will be drawn with. With a selection they are the
   * same click: one command, one undo.
   */
  const changeSettings = (next: ToolSettings): void => {
    setSettings(next)
    if (selected) run(updateLayer(selected.id, restyleLayer(selected, next)))
  }

  const deliver = (send: (blob: Blob) => void | Promise<void>, what: string): void => {
    // The history's document rather than the preview: a drag still in progress
    // has not been committed, and exporting it would ship a shape the user has
    // not let go of yet.
    toBlob(image, history.document, EXPORT_TYPE)
      .then((blob) => send(blob))
      .catch((error: unknown) => {
        console.error(`editor: could not ${what} the picture`, error)
        setNotice(`The picture could not be ${what === 'copy' ? 'copied' : 'saved'}.`)
      })
  }

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
      // into it would also be a shortcut. Two keys still get through: Escape
      // throws the box away, and Cmd+Enter finishes it, because Enter itself
      // has to stay available for a second line.
      if (text) {
        if (event.key === 'Escape') {
          event.preventDefault()
          openText(null)
        } else if (meta && event.key === 'Enter') {
          event.preventDefault()
          commitText()
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
        deliver(onCopy, 'copy')
        return
      }
      if (meta && key === 's') {
        // Without this the browser's own save dialog opens over the editor.
        event.preventDefault()
        deliver((blob) => onExport(blob, EXPORT_TYPE), 'save')
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
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              changeSettings({ ...settings, strokeWidth: Number(event.target.value) })
            }
          />
          <span data-testid="stroke-width-value" style={{ width: 20, textAlign: 'right' }}>
            {settings.strokeWidth}
          </span>
        </label>

        <label style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          Obscure
          <select
            data-testid="obscure-mode"
            aria-label="Obscure mode"
            value={settings.obscureMode}
            onChange={(event: ChangeEvent<HTMLSelectElement>) =>
              changeSettings({ ...settings, obscureMode: event.target.value as ObscureMode })
            }
            style={{ background: '#2c2c2e', color: 'inherit', border: '1px solid #48484a', borderRadius: 4 }}
          >
            {OBSCURE_MODES.map((mode) => (
              <option key={mode.value} value={mode.value}>
                {mode.label}
              </option>
            ))}
          </select>
        </label>

        <div style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
          <button type="button" data-testid="copy" onClick={() => deliver(onCopy, 'copy')} style={actionButtonStyle}>
            Copy
          </button>
          <button
            type="button"
            data-testid="save"
            onClick={() => deliver((blob) => onExport(blob, EXPORT_TYPE), 'save')}
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
          <span data-testid="notice" role="status" style={{ color: '#ff9f0a' }}>
            {notice}
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

const actionButtonStyle: CSSProperties = {
  padding: '4px 10px',
  borderRadius: 5,
  border: '1px solid #48484a',
  background: '#2c2c2e',
  color: '#f5f5f7',
  font: 'inherit',
  cursor: 'pointer',
}
