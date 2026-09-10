/**
 * The editing surface, driven in a real browser by real input.
 *
 * The rest of the package is arithmetic and can be tested by calling it. This
 * file cannot: every defect it exists to catch lives in the path from a control
 * to a layer, and that path is made of events the DOM produces rather than of
 * values a test can pass in. A range input fires on every value it travels
 * through, a browser binds Cmd+S to its own dialog, and a caption's box is a
 * measurement only a rasteriser can make. A programmatic `fill()` reproduces
 * none of those, which is exactly how three of them reached review.
 *
 * So the component is mounted into the page and worked with the provider's
 * pointer and keyboard: real presses at real coordinates, real key repeats.
 *
 * Assertions are over things the user can see. The document is not reachable
 * from here and should not be: what matters is that the status bar says one
 * annotation, that the selection outline covers the ink, that two presses of
 * Cmd+Z empty the picture and that the file that leaves through `onExport` is
 * the size the crop asked for. `layer-count`, `selected-kind`, `view-size` and
 * the selection chrome are the surface's own readouts, and the canvas is read
 * back as pixels where a readout would not settle it.
 *
 * The source image is a transparent canvas, so every non-transparent pixel on
 * the preview is ink an annotation put there and `inkBounds` can measure it.
 */

import { userEvent } from '@vitest/browser/context'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { Editor } from './Editor'
import type { Point, Rect } from './model'
import { textSizeFor } from './tools'

/**
 * The capture, and the box the editor is given to fit it in.
 *
 * Twice the picture in both axes, so `fitViewport` clamps the scale at 1 and
 * one source pixel is one CSS pixel. Every coordinate below is therefore a
 * source coordinate, and `mountEditor` asserts the scale rather than assuming
 * it, so a change to the toolbar's height says so instead of quietly moving
 * every press half a pixel.
 */
const SOURCE = { width: 400, height: 300 }
const CONTAINER = { width: 800, height: 600 }

/** The stroke width the toolbar opens on, and the slider's ends. */
const DEFAULT_STROKE = 4
const MAX_STROKE = 24

/** How far a measured box may sit from where it is expected, in CSS pixels. */
const PIXEL_SLACK = 2

/**
 * What left through the props, and the switches that steer a handover.
 *
 * `failNext` is how a host that cannot take the picture is stood up: the only
 * operation in this component that can fail in a way the user has to be told
 * about is the handover, and the status bar's behaviour around that failure is
 * not reachable any other way.
 *
 * `failCopy` is the same switch for the clipboard, and it is separate because
 * the case worth standing up is a save that worked and a clipboard that did
 * not. That pair is what decides whether the status bar reports a failed save,
 * which would be untrue, or a stale clipboard, which is the thing the user has
 * to know. It stays set, because unlike `failNext` nothing retries it.
 *
 * `savedAs` is the file name a host reports back, and it is null by default
 * because a host that names nothing is the case every other test in this file
 * mounts: naming one would put a message in the status bar that those tests
 * assert the absence of.
 *
 * `types` is what `onExport` was told the blob is, kept beside the blobs
 * because the two are separate claims. The type argument is what picks the
 * extension on the host's side, and the blob's own type is what the encoder
 * actually produced; a format control that changed one without the other would
 * be the exact defect that put PNG bytes in a `.webp` file.
 */
type Delivered = {
  saved: Blob[]
  types: string[]
  copied: Blob[]
  closed: number
  failNext: boolean
  failCopy: boolean
  savedAs: string | null
}

let root: Root | null = null
let container: HTMLDivElement | null = null
/**
 * What the editor currently under test handed out.
 *
 * Each mount gets its own record and the props close over that record rather
 * than over this variable. `toBlob` is asynchronous, so an editor unmounted
 * mid-export still resolves, and a shared record would let the last test's
 * picture arrive in the next test's list and be measured there.
 */
let delivered: Delivered = {
  saved: [],
  types: [],
  copied: [],
  closed: 0,
  failNext: false,
  failCopy: false,
  savedAs: null,
}

async function mountEditor(): Promise<void> {
  container = document.createElement('div')
  container.style.position = 'fixed'
  container.style.left = '0px'
  container.style.top = '0px'
  container.style.width = `${CONTAINER.width}px`
  container.style.height = `${CONTAINER.height}px`
  document.body.appendChild(container)

  const own: Delivered = {
    saved: [],
    types: [],
    copied: [],
    closed: 0,
    failNext: false,
    failCopy: false,
    savedAs: null,
  }
  delivered = own
  // Transparent, not noise: this file measures ink, and a picture under it
  // would make every pixel opaque and `inkBounds` the whole canvas.
  const image = new OffscreenCanvas(SOURCE.width, SOURCE.height)

  root = createRoot(container)
  root.render(
    <Editor
      image={image}
      width={SOURCE.width}
      height={SOURCE.height}
      onExport={(blob, type) => {
        if (own.failNext) {
          own.failNext = false
          throw new Error('the host would not take the picture')
        }
        own.saved.push(blob)
        own.types.push(type)
        return own.savedAs ?? undefined
      }}
      onCopy={(blob) => {
        if (own.failCopy) throw new Error('the host would not take the clipboard')
        own.copied.push(blob)
      }}
      onClose={() => {
        own.closed += 1
      }}
    />,
  )

  // The canvas is sized by a ResizeObserver, so nothing can be aimed at it
  // before that has fired.
  await vi.waitFor(() => {
    expect(canvasBox().width).toBe(SOURCE.width)
    expect(canvasBox().height).toBe(SOURCE.height)
  })
}

beforeEach(async () => {
  await mountEditor()
})

afterEach(() => {
  root?.unmount()
  container?.remove()
  root = null
  container = null
})

function byTestId(id: string): HTMLElement {
  const element = document.querySelector<HTMLElement>(`[data-testid="${id}"]`)
  if (!element) throw new Error(`no element with data-testid="${id}"`)
  return element
}

function maybeTestId(id: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-testid="${id}"]`)
}

function canvasBox(): DOMRect {
  return byTestId('canvas').getBoundingClientRect()
}

/** How many annotations the status bar says the document holds. */
function layerCount(): number {
  return Number((byTestId('layer-count').textContent ?? '').split(' ')[0])
}

/** What the status bar says is selected: a layer kind, or "nothing". */
function selectedKind(): string {
  return (byTestId('selected-kind').textContent ?? '').split(' ')[0] ?? ''
}

/** The size of the picture that would be exported, as the status bar reads it. */
function viewSize(): string {
  return (byTestId('view-size').textContent ?? '').replace(/\s+/g, ' ').trim()
}

/** The width the toolbar's slider currently stands at. */
function strokeWidth(): number {
  return Number((byTestId('stroke-width') as HTMLInputElement).value)
}

/**
 * The selection outline, in source-image pixels, or null with no selection.
 *
 * The scale is 1 and the view starts at the origin unless a crop has moved it,
 * so the canvas's own corner is the only conversion this needs.
 */
function selectionBox(): Rect | null {
  const outline = maybeTestId('selection')
  if (!outline) return null
  const box = outline.getBoundingClientRect()
  const canvas = canvasBox()
  return { x: box.left - canvas.left, y: box.top - canvas.top, width: box.width, height: box.height }
}

/** How many resize handles the selection chrome is offering. */
function handleCount(): number {
  return document.querySelectorAll('[data-testid^="handle-"]').length
}

/**
 * The box the painted pixels occupy, in source-image pixels, or null if the
 * canvas is empty.
 *
 * The source image is transparent, so anything with alpha is a mark. This is
 * the only honest way to ask whether a caption's box still describes the
 * caption: the box is a number the tool stored, and the glyphs are what a
 * rasteriser actually drew, and a test that compares one stored number with
 * another cannot tell you they have come apart.
 */
function inkBounds(): Rect | null {
  const canvas = byTestId('canvas') as HTMLCanvasElement
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('no 2d context')
  // The backing store is in device pixels and the element is in CSS pixels.
  const ratio = canvas.width / canvas.getBoundingClientRect().width
  const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height)
  let left = Number.POSITIVE_INFINITY
  let top = Number.POSITIVE_INFINITY
  let right = Number.NEGATIVE_INFINITY
  let bottom = Number.NEGATIVE_INFINITY
  for (let y = 0; y < pixels.height; y += 1) {
    for (let x = 0; x < pixels.width; x += 1) {
      if ((pixels.data[(y * pixels.width + x) * 4 + 3] ?? 0) === 0) continue
      if (x < left) left = x
      if (x > right) right = x
      if (y < top) top = y
      if (y > bottom) bottom = y
    }
  }
  if (right < left) return null
  return {
    x: left / ratio,
    y: top / ratio,
    width: (right - left + 1) / ratio,
    height: (bottom - top + 1) / ratio,
  }
}

/** Whether `inner` sits inside `outer`, allowing for antialiasing at the edges. */
function contains(outer: Rect, inner: Rect, slack = PIXEL_SLACK): boolean {
  return (
    inner.x >= outer.x - slack &&
    inner.y >= outer.y - slack &&
    inner.x + inner.width <= outer.x + outer.width + slack &&
    inner.y + inner.height <= outer.y + outer.height + slack
  )
}

/** A source-image point as a position inside the stage, for the provider. */
function stagePosition(point: Point): { x: number; y: number } {
  const stage = byTestId('stage').getBoundingClientRect()
  const canvas = canvasBox()
  return { x: canvas.left - stage.left + point.x, y: canvas.top - stage.top + point.y }
}

async function chooseTool(name: string): Promise<void> {
  await userEvent.click(byTestId(`tool-${name}`))
}

/** A real press and release at one point of the picture. */
async function pressAt(point: Point): Promise<void> {
  await userEvent.click(byTestId('stage'), { position: stagePosition(point) })
}

/** A real press, drag and release across the picture. */
async function dragAcross(from: Point, to: Point): Promise<void> {
  const stage = byTestId('stage')
  await userEvent.dragAndDrop(stage, stage, {
    sourcePosition: stagePosition(from),
    targetPosition: stagePosition(to),
  })
}

/**
 * Control, not Meta.
 *
 * The editor answers `metaKey || ctrlKey`, so either exercises the same branch,
 * and Control is the one that means the same thing on the machine CI runs on.
 */
async function undo(): Promise<void> {
  await userEvent.keyboard('{Control>}z{/Control}')
}

async function redo(): Promise<void> {
  await userEvent.keyboard('{Control>}{Shift>}z{/Shift}{/Control}')
}

/** Waits for React to have committed everything the last input asked for. */
async function settle(): Promise<void> {
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
  })
}

/**
 * How long an assertion that cannot pass until a picture has been encoded is
 * given.
 *
 * `vi.waitFor` allows one second, and one second is the wrong budget to hold a
 * `convertToBlob` to. Encoding this canvas was measured at 9-25 ms on every
 * save in this file across about a hundred and forty instrumented runs, with
 * one exception: the first encode a page performs sometimes returns after
 * roughly 1005 ms instead. Six of those were caught, at 1004, 1005, 1005,
 * 1042, 1048 and 1053 ms, and nothing was ever measured in between: no encode
 * in any run took between 25 ms and a second. A gap like that is a fallback
 * timer inside the browser and not a machine under load, which a spread of
 * intermediate times would have looked like. The component is not part of it
 * either: `deliver` calls `toBlob` synchronously while handling the keydown,
 * and from there it only awaits the promise.
 *
 * So a browser that stalls for 1005 ms was being measured against a deadline of
 * 1000, and whichever test saved first lost that coin toss about one run in
 * twenty. It was `does not let Cmd+S reach the host`, the first save in the
 * file, and it would have moved to whatever test took that place next.
 *
 * Five seconds sits well clear of the stall and well inside the thirty this
 * project allows a browser test, so a delivery that never arrives still fails,
 * and still fails long before the suite gives up on the test.
 */
const DELIVERY_DEADLINE = 5_000

/**
 * `vi.waitFor` for the things a delivery produces: the blobs a host was handed,
 * and the status bar lines that only appear once one has been.
 *
 * Everything else in this file waits on a React commit, which is a frame away,
 * and keeps the shorter default.
 */
async function waitForDelivery<T>(assertion: () => T | Promise<T>): Promise<T> {
  return vi.waitFor(assertion, { timeout: DELIVERY_DEADLINE })
}

describe('drawing', () => {
  // One case per tool, because "the toolbar works" is not a thing a test can
  // fail on and "the rectangle tool produced a rectangle" is. The hit point is
  // per tool as well: an unfilled shape is a frame, so the click that finds it
  // has to land on its edge and not in the middle of what it was drawn around.
  const marks: { tool: string; kind: string; draw: () => Promise<void>; hit: Point }[] = [
    {
      tool: 'arrow',
      kind: 'arrow',
      draw: () => dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 }),
      hit: { x: 130, y: 100 },
    },
    {
      tool: 'rect',
      kind: 'rect',
      draw: () => dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 }),
      hit: { x: 130, y: 60 },
    },
    {
      tool: 'ellipse',
      kind: 'ellipse',
      draw: () => dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 }),
      hit: { x: 60, y: 100 },
    },
    {
      tool: 'line',
      kind: 'line',
      draw: () => dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 }),
      hit: { x: 130, y: 100 },
    },
    {
      tool: 'highlight',
      kind: 'highlight',
      draw: () => dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 }),
      hit: { x: 130, y: 100 },
    },
    {
      tool: 'obscure',
      kind: 'obscure',
      draw: () => dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 }),
      hit: { x: 130, y: 100 },
    },
    {
      tool: 'step',
      kind: 'step',
      draw: () => pressAt({ x: 150, y: 150 }),
      hit: { x: 150, y: 150 },
    },
  ]

  it.each(marks)('the $tool tool adds a $kind layer', async ({ tool, kind, draw, hit }) => {
    await chooseTool(tool)
    await draw()
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    await chooseTool('select')
    await pressAt(hit)
    await vi.waitFor(() => expect(selectedKind()).toBe(kind))
    expect(maybeTestId('selection')?.dataset.kind).toBe(kind)
  })

  it('adds a text layer from a typed caption', async () => {
    await chooseTool('text')
    await pressAt({ x: 60, y: 100 })
    const box = await vi.waitFor(() => byTestId('text-input') as HTMLTextAreaElement)
    await userEvent.type(box, 'Hello')
    await userEvent.keyboard('{Control>}{Enter}{/Control}')
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    await chooseTool('select')
    await pressAt({ x: 65, y: 110 })
    await vi.waitFor(() => expect(selectedKind()).toBe('text'))
  })

  // A press that never moved is a stray click on the picture, not an
  // invisible, unhittable layer that the next Cmd+Z appears not to remove.
  it('leaves nothing behind for a press that never moved', async () => {
    await chooseTool('rect')
    await pressAt({ x: 120, y: 120 })
    await settle()
    expect(layerCount()).toBe(0)
  })
})

/**
 * The redaction mode the toolbar opens in, and the control that changes it.
 *
 * Both assertions are made through the picture rather than through the
 * toolbar's own state, and the transparent source is what lets them be. Blur
 * and pixelate average what is under them, and what is under them has alpha 0,
 * so neither leaves a mark; blackout replaces the region with opaque black
 * whatever was there. So "there is ink where the drag was" means blackout and
 * nothing else, and a default quietly moved back to blur fails here rather
 * than shipping a redaction that can be read through.
 */
describe('the obscure tool', () => {
  const band = { x: 60, y: 60, width: 140, height: 80 }

  it('opens in blackout and blacks out on the first drag', async () => {
    expect(byTestId('obscure-blackout').getAttribute('aria-pressed')).toBe('true')
    expect(byTestId('obscure-blur').getAttribute('aria-pressed')).toBe('false')
    expect(byTestId('obscure-pixelate').getAttribute('aria-pressed')).toBe('false')

    await chooseTool('obscure')
    await dragAcross({ x: band.x, y: band.y }, { x: band.x + band.width, y: band.y + band.height })
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    const ink = inkBounds()
    expect(ink).not.toBeNull()
    // Both directions: the ink covers the band and nothing outside it.
    expect(contains(band, ink as Rect)).toBe(true)
    expect(contains(ink as Rect, band)).toBe(true)
  })

  it('changes mode from the toolbar', async () => {
    await userEvent.click(byTestId('obscure-blur'))
    expect(byTestId('obscure-blur').getAttribute('aria-pressed')).toBe('true')
    expect(byTestId('obscure-blackout').getAttribute('aria-pressed')).toBe('false')

    await chooseTool('obscure')
    await dragAcross({ x: band.x, y: band.y }, { x: band.x + band.width, y: band.y + band.height })
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    // A blur of nothing is nothing. The layer is in the document and the
    // picture is still empty, which is the mode having really changed.
    expect(inkBounds()).toBeNull()
  })
})

describe('undo and redo', () => {
  it('move the layer count in both directions', async () => {
    await chooseTool('rect')
    await dragAcross({ x: 40, y: 40 }, { x: 140, y: 120 })
    await dragAcross({ x: 200, y: 40 }, { x: 300, y: 120 })
    await vi.waitFor(() => expect(layerCount()).toBe(2))

    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(0))

    await redo()
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await redo()
    await vi.waitFor(() => expect(layerCount()).toBe(2))
  })
})

describe('the width slider', () => {
  /** Draws a rectangle and selects it, which is what the slider then restyles. */
  async function drawAndSelectRect(): Promise<void> {
    await chooseTool('rect')
    await dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 })
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await chooseTool('select')
    await pressAt({ x: 130, y: 60 })
    await vi.waitFor(() => expect(selectedKind()).toBe('rect'))
  }

  // The defect this is here for: `onChange` fires on every value a range input
  // travels through, so a restyle per value put up to twenty entries on the
  // undo stack for one gesture and reversing it took twenty presses of Cmd+Z.
  // The count is not readable from outside, so it is measured the way a user
  // would find it: undo once and the layer is still there with its original
  // stroke, undo twice and the picture is empty. Against the unfixed component
  // the second press is still walking back the drag and the count stays at 1.
  it('spends one undo entry on a whole keyboard gesture', async () => {
    await drawAndSelectRect()
    const before = selectionBox()
    expect(before).not.toBeNull()

    const slider = byTestId('stroke-width') as HTMLInputElement
    slider.focus()
    // Sixteen real key presses, each one a `keydown` and an `input`.
    for (let step = 0; step < 16; step += 1) await userEvent.keyboard('{ArrowRight}')
    await vi.waitFor(() => expect(strokeWidth()).toBe(DEFAULT_STROKE + 16))

    // The preview has to stay live through the gesture, or the width knob
    // would be a control with no feedback until the user let go of it. The
    // outline grows with the stroke, because it is drawn clear of the ink.
    const during = selectionBox()
    expect(during?.width).toBeGreaterThan((before?.width ?? 0) + 8)

    // Tab out: the blur is one of the two events that commit the gesture.
    await userEvent.tab()
    await settle()

    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    expect(selectionBox()?.width).toBeCloseTo(before?.width ?? 0, 0)

    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(0))
  })

  it('spends one undo entry on a whole pointer drag', async () => {
    await drawAndSelectRect()

    const slider = byTestId('stroke-width')
    const track = slider.getBoundingClientRect()
    await userEvent.dragAndDrop(slider, slider, {
      sourcePosition: { x: 2, y: track.height / 2 },
      targetPosition: { x: track.width - 2, y: track.height / 2 },
    })
    await vi.waitFor(() => expect(strokeWidth()).toBe(MAX_STROKE))
    await settle()

    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(0))
  })

  // The swatches were never part of the defect and must not become part of the
  // fix: a colour is picked in one click, so it is one command with no gesture
  // to wait for.
  it('leaves the colour swatches at one command each', async () => {
    await drawAndSelectRect()
    await userEvent.click(byTestId('color-0a84ff'))
    await settle()

    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(0))
  })

  // Restyling a text layer changes the size the glyphs are painted at, so the
  // box that size was measured in has to be measured again in the same command.
  // Left alone, the box describes the old measurement: the outline covers a
  // fraction of the caption and `layerAtPoint` misses most of it. The ink is
  // read off the canvas rather than taken from a stored number, because a
  // stored number is exactly what has come apart from the picture.
  it('keeps a restyled caption inside its own box', async () => {
    await chooseTool('text')
    await pressAt({ x: 40, y: 100 })
    const box = await vi.waitFor(() => byTestId('text-input') as HTMLTextAreaElement)
    await userEvent.type(box, 'Hello')
    await userEvent.keyboard('{Control>}{Enter}{/Control}')
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    await chooseTool('select')
    await pressAt({ x: 45, y: 110 })
    await vi.waitFor(() => expect(selectedKind()).toBe('text'))
    const small = selectionBox()
    expect(small).not.toBeNull()
    expect(contains(small as Rect, inkBounds() as Rect)).toBe(true)

    // Six presses: width 4 to 10, which is a text size of 24 to 60.
    const slider = byTestId('stroke-width') as HTMLInputElement
    slider.focus()
    for (let step = 0; step < 6; step += 1) await userEvent.keyboard('{ArrowRight}')
    await userEvent.tab()
    await vi.waitFor(() => expect(strokeWidth()).toBe(10))
    await settle()

    const grown = selectionBox()
    const ink = inkBounds()
    expect(grown).not.toBeNull()
    expect(ink).not.toBeNull()

    // The size really did move, so the box really does have to have moved with
    // it: 24 to 60 is two and a half times, and a box the restyle left alone
    // would still be the height of the smaller one.
    const ratio = textSizeFor(10) / textSizeFor(DEFAULT_STROKE)
    expect((grown as Rect).height).toBeGreaterThan((small as Rect).height * (ratio - 0.5))
    expect(contains(grown as Rect, ink as Rect)).toBe(true)

    // And the hit test follows the box. A press near the far end of the grown
    // caption is outside the box the old measurement described, so before the
    // fix it selected nothing.
    const far = { x: (ink as Rect).x + (ink as Rect).width - 4, y: (ink as Rect).y + 4 }
    expect(far.x).toBeGreaterThan((small as Rect).x + (small as Rect).width)
    await pressAt({ x: 320, y: 260 })
    await vi.waitFor(() => expect(selectedKind()).toBe('nothing'))
    await pressAt(far)
    await vi.waitFor(() => expect(selectedKind()).toBe('text'))
  })
})

describe('the keyboard while a text box is open', () => {
  /**
   * Catches a key event after the editor's own window listener has seen it.
   *
   * Registered last, so it runs last: listeners on the same target fire in the
   * order they were added, and the editor re-adds its own on every render. The
   * caller settles the component first and presses the key without touching
   * state in between, which is what keeps this one behind it.
   */
  function keyProbe(): { seen: KeyboardEvent[]; stop: () => void } {
    const seen: KeyboardEvent[] = []
    const listen = (event: KeyboardEvent): void => {
      seen.push(event)
    }
    window.addEventListener('keydown', listen)
    return { seen, stop: () => window.removeEventListener('keydown', listen) }
  }

  // The branch that handles a text box returned before reaching the
  // `preventDefault` that exists to stop the browser's own save dialog, so in
  // the browser-extension host this component is built for, Cmd+S while typing
  // a caption opened the host's Save-page dialog over the editor.
  it('does not let Cmd+S reach the host', async () => {
    await chooseTool('text')
    await pressAt({ x: 60, y: 100 })
    const box = await vi.waitFor(() => byTestId('text-input') as HTMLTextAreaElement)
    await userEvent.type(box, 'Draft')
    await vi.waitFor(() => expect(box.value).toBe('Draft'))
    await settle()

    const probe = keyProbe()
    try {
      await userEvent.keyboard('{Control>}s{/Control}')
      // The finding, first: an unprevented Cmd+S is the browser's Save-page
      // dialog opening over the editor.
      const save = probe.seen.find((event) => event.key.toLowerCase() === 's')
      expect(save).toBeDefined()
      expect(save?.defaultPrevented).toBe(true)
      await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))
    } finally {
      probe.stop()
    }

    // Committed rather than dropped, so the file carries the sentence that was
    // being typed when the shortcut was pressed.
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    expect(maybeTestId('text-input')).toBeNull()
    // Once, not twice: the box closing must not deliver a second picture on
    // its way out. There is no event to await for something that should not
    // happen, so this waits out a window several times the length of the
    // encode that did happen.
    await new Promise((resolve) => setTimeout(resolve, 300))
    expect(delivered.saved).toHaveLength(1)
  })

  // The textarea's own copy is what selecting a word inside the box means by
  // Cmd+C, so the editor must not take it and export the picture instead.
  it('leaves Cmd+C to the text box', async () => {
    await chooseTool('text')
    await pressAt({ x: 60, y: 100 })
    const box = await vi.waitFor(() => byTestId('text-input') as HTMLTextAreaElement)
    await userEvent.type(box, 'Draft')
    await settle()

    await userEvent.keyboard('{Control>}c{/Control}')
    await settle()
    expect(delivered.copied).toHaveLength(0)
    expect(maybeTestId('text-input')).not.toBeNull()
  })
})

describe('the selection chrome', () => {
  // A step badge keeps its size through a resize by design, so a handle on one
  // would only re-centre it and appear to lag the hand. There is nothing to
  // resize, so there is nothing to grab.
  it('offers no resize handles on a step badge and eight on a rectangle', async () => {
    await chooseTool('step')
    await pressAt({ x: 150, y: 150 })
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await chooseTool('select')
    await pressAt({ x: 150, y: 150 })
    await vi.waitFor(() => expect(selectedKind()).toBe('step'))
    expect(handleCount()).toBe(0)

    await chooseTool('rect')
    await dragAcross({ x: 40, y: 40 }, { x: 120, y: 100 })
    await vi.waitFor(() => expect(layerCount()).toBe(2))
    await chooseTool('select')
    await pressAt({ x: 80, y: 40 })
    await vi.waitFor(() => expect(selectedKind()).toBe('rect'))
    expect(handleCount()).toBe(8)
  })

  // A caption's box is measured from its glyphs at its point size, and a
  // resize returns a new rect without re-measuring anything. Eight handles on
  // one were a way to drag the box away from the ink: the renderer would keep
  // painting the old size from the new origin while the outline and the hit
  // test described the box the pointer made. Text is resized by the width
  // slider, which re-measures, so the handles are simply not offered.
  it('offers no resize handles on a caption', async () => {
    await chooseTool('text')
    await pressAt({ x: 60, y: 100 })
    const box = await vi.waitFor(() => byTestId('text-input') as HTMLTextAreaElement)
    await userEvent.type(box, 'Hello')
    await userEvent.keyboard('{Control>}{Enter}{/Control}')
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    await chooseTool('select')
    await pressAt({ x: 65, y: 110 })
    await vi.waitFor(() => expect(selectedKind()).toBe('text'))
    expect(selectionBox()).not.toBeNull()
    expect(handleCount()).toBe(0)
  })
})

/**
 * The two keys the README documents that nothing was driving.
 *
 * Both are the kind of shortcut that is easy to break without noticing,
 * because the visible effect of breaking them is that a key does nothing.
 */
describe('the keyboard with no text box open', () => {
  it('removes the selected annotation on Delete', async () => {
    await chooseTool('rect')
    await dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 })
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await chooseTool('select')
    await pressAt({ x: 130, y: 60 })
    await vi.waitFor(() => expect(selectedKind()).toBe('rect'))

    await userEvent.keyboard('{Delete}')

    await vi.waitFor(() => expect(layerCount()).toBe(0))
    expect(selectedKind()).toBe('nothing')
    expect(selectionBox()).toBeNull()
    // Removal is an ordinary command, so the key that got out of a mistake is
    // itself undoable.
    await undo()
    await vi.waitFor(() => expect(layerCount()).toBe(1))
  })

  // One press drops the selection and a second closes the window, so the key
  // somebody reaches for to get out of a mis-click is never the key that
  // throws the whole annotation away.
  it('drops the selection on the first Escape and closes on the second', async () => {
    await chooseTool('rect')
    await dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 })
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await chooseTool('select')
    await pressAt({ x: 130, y: 60 })
    await vi.waitFor(() => expect(selectedKind()).toBe('rect'))

    await userEvent.keyboard('{Escape}')

    await vi.waitFor(() => expect(selectedKind()).toBe('nothing'))
    // The annotation is still there: this key drops the selection, not the work.
    expect(layerCount()).toBe(1)
    expect(delivered.closed).toBe(0)

    await userEvent.keyboard('{Escape}')

    await vi.waitFor(() => expect(delivered.closed).toBe(1))
    expect(layerCount()).toBe(1)
  })
})

/**
 * The clipboard after a save.
 *
 * A host that copies the capture when it takes it, which is what Snapdeck
 * does, holds the untouched original the whole time the editor is open. Black
 * out a password, press Cmd+S, paste: without this the unredacted capture is
 * what arrives, which is the exact failure the redaction feature exists to
 * prevent, on the shortest path a user takes to it.
 */
describe('saving and the clipboard', () => {
  it('replaces the clipboard with the picture it just saved', async () => {
    await chooseTool('obscure')
    await dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 })
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    await userEvent.click(byTestId('save'))

    await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))
    await waitForDelivery(() => expect(delivered.copied).toHaveLength(1))
    // The bytes just encoded, not a second export: the file and the clipboard
    // cannot then be different pictures.
    expect(delivered.copied[0]).toBe(delivered.saved[0])
  })

  // The clipboard takes pixels, not a file, so the reason to pick JPEG does
  // not reach it, and a host is entitled to accept less here than on disk:
  // Snapdeck's own decodes PNG only, so handing it the JPEG it had just
  // written would fail the clipboard write and leave the unredacted capture
  // sitting there, which is the whole leak, reopened on the JPEG path.
  it('puts a lossless copy on the clipboard after a JPEG save', async () => {
    await chooseTool('rect')
    await dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 })
    await vi.waitFor(() => expect(layerCount()).toBe(1))
    await userEvent.click(byTestId('format-jpeg'))

    await userEvent.click(byTestId('save'))

    await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))
    await waitForDelivery(() => expect(delivered.copied).toHaveLength(1))
    expect((delivered.saved[0] as Blob).type).toBe('image/jpeg')
    expect((delivered.copied[0] as Blob).type).toBe('image/png')
  })

  // A crop hides by removal rather than by covering, so it is an edit for this
  // purpose exactly as a layer is.
  it('replaces the clipboard after a save that only cropped', async () => {
    await chooseTool('crop')
    await dragAcross({ x: 100, y: 80 }, { x: 300, y: 220 })
    await vi.waitFor(() => expect(viewSize()).toBe('200 × 140'))
    expect(layerCount()).toBe(0)

    await userEvent.click(byTestId('save'))

    await waitForDelivery(() => expect(delivered.copied).toHaveLength(1))
    const bitmap = await createImageBitmap(delivered.copied[0] as Blob)
    try {
      expect(bitmap.width).toBe(200)
      expect(bitmap.height).toBe(140)
    } finally {
      bitmap.close()
    }
  })

  // The other half of the rule. An untouched document is the picture the host
  // already has, so a save of one has no business touching a clipboard the
  // user may have put something else on.
  it('leaves the clipboard alone when nothing was edited', async () => {
    await userEvent.click(byTestId('save'))

    await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))
    await settle()
    expect(delivered.copied).toHaveLength(0)
  })

  // A save that worked followed by a clipboard that refused is not a failed
  // save, and saying so would send the user back to press Save again while the
  // stale capture stays where the leak is.
  it('says the clipboard is stale rather than that the save failed', async () => {
    delivered.failCopy = true
    await chooseTool('rect')
    await dragAcross({ x: 60, y: 60 }, { x: 200, y: 140 })
    await vi.waitFor(() => expect(layerCount()).toBe(1))

    await userEvent.click(byTestId('save'))

    await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))
    await waitForDelivery(() => expect(byTestId('notice').textContent ?? '').toContain('clipboard'))
    expect(byTestId('notice').textContent ?? '').not.toContain('could not be saved')
  })
})

describe('the status bar', () => {
  // A warning is worth showing for as long as it is true and no longer. One
  // export that refused used to leave its message in the status bar for the
  // rest of the session, sitting behind every later save that worked, which
  // turns the one line the editor has for telling the user something serious
  // into a line they learn to read past.
  it('clears a delivery warning once a later save works', async () => {
    delivered.failNext = true
    await userEvent.click(byTestId('save'))
    await waitForDelivery(() => expect(maybeTestId('notice')?.textContent ?? '').toContain('could not be saved'))
    expect(delivered.saved).toHaveLength(0)

    await userEvent.click(byTestId('save'))
    await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))
    await waitForDelivery(() => expect(maybeTestId('notice')).toBeNull())
  })
})

describe('the save format', () => {
  // The control exists because the format decides the file: PNG replaces the
  // capture, JPEG lands beside it. That only holds if pressing JPEG reaches
  // both the encoder and the type the host names the file after, so both are
  // asserted rather than the button's own colour.
  it('opens on PNG and encodes JPEG once JPEG is chosen', async () => {
    expect(byTestId('format-png').getAttribute('aria-pressed')).toBe('true')
    expect(byTestId('format-jpeg').getAttribute('aria-pressed')).toBe('false')

    await userEvent.click(byTestId('save'))
    await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))
    expect(delivered.types[0]).toBe('image/png')
    expect((delivered.saved[0] as Blob).type).toBe('image/png')

    await userEvent.click(byTestId('format-jpeg'))
    expect(byTestId('format-jpeg').getAttribute('aria-pressed')).toBe('true')
    expect(byTestId('format-png').getAttribute('aria-pressed')).toBe('false')

    await userEvent.click(byTestId('save'))
    await waitForDelivery(() => expect(delivered.saved).toHaveLength(2))
    expect(delivered.types[1]).toBe('image/jpeg')
    expect((delivered.saved[1] as Blob).type).toBe('image/jpeg')
  })

  // A clipboard image is pixels handed to the next application, not a file, so
  // there is nothing for a lossy encode to buy and detail for it to cost.
  it('copies losslessly even with JPEG chosen', async () => {
    await userEvent.click(byTestId('format-jpeg'))
    await userEvent.click(byTestId('copy'))
    await waitForDelivery(() => expect(delivered.copied).toHaveLength(1))
    expect((delivered.copied[0] as Blob).type).toBe('image/png')
  })

  // Saving as PNG overwrites the capture and saving as JPEG leaves a second
  // file beside it. The name is the only thing that tells those two apart, and
  // the editor has no path of its own, so the host's answer has to reach the
  // status bar unchanged.
  it('shows the name the host says it wrote', async () => {
    delivered.savedAs = 'Snapdeck 2026-09-09 at 12.00.01.jpg'
    await userEvent.click(byTestId('save'))
    await waitForDelivery(() =>
      expect(byTestId('notice').textContent).toBe('Saved Snapdeck 2026-09-09 at 12.00.01.jpg'),
    )
  })
})

describe('cropping', () => {
  it('changes the size of the file that leaves through onExport', async () => {
    await chooseTool('crop')
    await dragAcross({ x: 100, y: 80 }, { x: 300, y: 220 })
    await vi.waitFor(() => expect(viewSize()).toBe('200 × 140'))

    await userEvent.click(byTestId('save'))
    await waitForDelivery(() => expect(delivered.saved).toHaveLength(1))

    const bitmap = await createImageBitmap(delivered.saved[0] as Blob)
    try {
      expect(bitmap.width).toBe(200)
      expect(bitmap.height).toBe(140)
    } finally {
      bitmap.close()
    }
  })
})
