/**
 * Makes the pictures in `docs/images` again, from the real interfaces.
 *
 * `pnpm screenshots` from the repository root, and nothing appears on screen:
 * Chromium runs headless and no system capture tool is involved anywhere in
 * here. That is the point. A README picture taken by photographing a desktop is
 * a picture of whatever else was on that desktop, and a screenshot tool's README
 * is the worst possible place to find that out.
 *
 * The order is one dependency chain.
 *
 * `docs/images/demo-page.html` is rendered first, at a fixed size, into a PNG
 * that never reaches the disk. It is a fictional analytics page: fictional so
 * that the redaction has something to redact, and a page rather than a picture
 * so the thing under the annotations can be read and re-made.
 *
 * That render is then handed to three pages under `tools/screenshots`, each of
 * which mounts a component the application itself ships, and Chromium is driven
 * over them with real presses at real coordinates. The annotations in
 * `editor.png` are drawn by the editor, by the same code path a user's pointer
 * takes, because the alternative is a picture of something the product does not
 * do. `packages/editor` builds its own history inside the component and takes no
 * initial document, so there is no way in from the model or the command API
 * without adding a prop to the shipping component for a screenshot's benefit.
 *
 * Every coordinate an annotation is placed at is measured out of the demo page
 * rather than written down here, so the pictures survive the page being edited
 * and survive a font that lays out a hair differently. Nothing is timed: each
 * step waits for something the surface itself says, and the run fails loudly if
 * it does not say it.
 */

import { chromium } from 'playwright'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'

const HERE = dirname(fileURLToPath(import.meta.url))
const REPO = resolve(HERE, '../..')
const IMAGES = resolve(REPO, 'docs/images')
const DEMO_PAGE = resolve(IMAGES, 'demo-page.html')

/**
 * The demo render, in pixels.
 *
 * Also its size in CSS points, because it is rendered at one device pixel per
 * point: the editor then shows it at 1:1, which is the only scale at which a
 * redaction in the preview is the redaction in the file. `render.ts` says why.
 */
const SCENE = { width: 1100, height: 720 }

/**
 * How much taller than the picture the editor window is, in points.
 *
 * `TOOLBAR_ALLOWANCE` in `apps/desktop/src-tauri/src/editor.rs`, which is what
 * the real window adds for the toolbar and the status bar. It is a starting
 * guess here and nothing more: the harness measures the stage it actually got
 * and resizes until the canvas is exactly the picture, so a toolbar that grows
 * a row moves the window rather than silently scaling the screenshot down.
 */
const EDITOR_CHROME = 56

/** The density every picture is taken at. */
const DENSITY = 2

/** How long any one wait is given before the run is called failed. */
const TIMEOUT = 15_000

async function main() {
  await mkdir(IMAGES, { recursive: true })

  const server = await createServer({
    configFile: resolve(HERE, 'vite.config.ts'),
    logLevel: 'warn',
    server: { port: 0 },
  })
  await server.listen()
  const base = server.resolvedUrls?.local?.[0]
  if (!base) throw new Error('screenshots: the dev server reported no address')

  const browser = await chromium.launch({ headless: true })
  const written = []
  try {
    const demo = await renderDemoPage(browser)
    written.push(await captureEditor(browser, base, demo))
    written.push(await captureOverlay(browser, base, demo))
    written.push(await captureSettings(browser, base, demo))
  } finally {
    await browser.close()
    await server.close()
  }

  for (const { path, width, height, bytes } of written) {
    console.log(`${path}  ${width}x${height}  ${(bytes / 1024).toFixed(0)} KB`)
  }
}

/**
 * The demo page, rendered, plus the boxes the annotations are aimed at.
 *
 * The measurements are taken here, in the page itself, and travel as numbers.
 * They are in the render's own pixel space, which is the editor's document
 * space, so nothing downstream has to know how the page is laid out.
 */
async function renderDemoPage(browser) {
  const html = await readFile(DEMO_PAGE, 'utf8')
  const context = await browser.newContext({ viewport: SCENE, deviceScaleFactor: 1 })
  try {
    const page = await context.newPage()
    await page.setContent(html, { waitUntil: 'load', timeout: TIMEOUT })
    // Web fonts are not used, but the system stack still resolves
    // asynchronously, and a screenshot taken before it settles is a screenshot
    // of the fallback metrics the measurements below were not taken with.
    await page.evaluate(async () => {
      await document.fonts.ready
    })

    const marks = await page.evaluate(() => {
      const box = (element) => {
        if (!element) throw new Error('demo page: a measured element is missing')
        const rect = element.getBoundingClientRect()
        return { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
      }
      const union = (rects) => {
        const left = Math.min(...rects.map((r) => r.x))
        const top = Math.min(...rects.map((r) => r.y))
        const right = Math.max(...rects.map((r) => r.x + r.width))
        const bottom = Math.max(...rects.map((r) => r.y + r.height))
        return { x: left, y: top, width: right - left, height: bottom - top }
      }
      const cards = [...document.querySelectorAll('main > .card')]
      const keys = [...document.querySelectorAll('td code')].map(box)
      const firstKey = keys[0]
      if (!firstKey) throw new Error('demo page: no keys to redact')
      return {
        header: box(document.querySelector('header')),
        title: box(document.querySelector('h1')),
        nav: box(document.querySelector('nav')),
        // The number the arrow points at, and the card the rectangle frames.
        growth: box(cards[0]?.querySelector('.up')),
        churn: box(cards[2]),
        chart: box(cards[3]),
        keys: union(keys),
        firstKey,
      }
    })

    const image = await page.screenshot({ type: 'png' })
    return { image: `data:image/png;base64,${image.toString('base64')}`, marks }
  } finally {
    await context.close()
  }
}

/** A page with the scene installed before any of its own code runs. */
async function open(browser, base, file, viewport, scene) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: DENSITY })
  const page = await context.newPage()
  // Collected rather than thrown from the listener: a throw inside an event
  // handler becomes an unhandled rejection with no stack pointing here, where
  // this way `shoot` refuses to write a picture of a page that failed.
  const failures = []
  page.on('pageerror', (error) => failures.push(error.message))
  page.__failures = failures
  await page.addInitScript((value) => {
    window.__SNAPDECK_SCENE__ = value
  }, scene)
  await page.goto(`${base}${file}`, { waitUntil: 'load', timeout: TIMEOUT })
  return { context, page }
}

/**
 * The editor, with three annotations on it, one of them a redaction.
 *
 * Drawn rather than assembled. Each one is a press, a drag and a release on the
 * stage at coordinates measured out of the demo page, so what the picture shows
 * is the tool doing its job; the status bar is read after every one of them,
 * which is how a gesture that landed on nothing fails the run instead of
 * quietly producing a screenshot with an annotation missing.
 */
async function captureEditor(browser, base, demo) {
  const scene = { image: demo.image, width: SCENE.width, height: SCENE.height }
  const { context, page } = await open(
    browser,
    base,
    '/editor.html',
    { width: SCENE.width, height: SCENE.height + EDITOR_CHROME },
    scene,
  )
  try {
    await page.waitForSelector('[data-testid="canvas"]', { timeout: TIMEOUT })

    // The window is sized so that the stage is exactly the picture and the
    // editor's own fit lands on 1:1. `EDITOR_CHROME` is the app's guess at the
    // toolbar and status bar; this is the measurement that makes it true.
    const stage = await boxOf(page, 'stage')
    const missing = SCENE.height - stage.height
    if (missing !== 0) {
      await page.setViewportSize({
        width: SCENE.width,
        height: SCENE.height + EDITOR_CHROME + Math.ceil(missing),
      })
    }
    await page.waitForFunction(
      (expected) => {
        const canvas = document.querySelector('[data-testid="canvas"]')
        if (!canvas) return false
        const rect = canvas.getBoundingClientRect()
        return Math.abs(rect.width - expected.width) < 1 && Math.abs(rect.height - expected.height) < 1
      },
      SCENE,
      { timeout: TIMEOUT },
    )

    const canvas = await boxOf(page, 'canvas')
    const at = (point) => ({ x: canvas.x + point.x, y: canvas.y + point.y })
    const { marks } = demo

    // 1. The redaction, over the three fake keys. Black out, which is the
    //    tool's own default and the only one of the three that leaves no
    //    signal at all behind; the note over `DEFAULT_OBSCURE_MODE` in
    //    `Editor.tsx` has the measurements. It is also the only one that is
    //    still unmistakably a redaction at the size a README renders a picture
    //    at: a mosaic of small light text averages out to a pale band.
    const redaction = inflate(marks.keys, 4, 3)
    await page.click('[data-testid="tool-obscure"]')
    await page.click('[data-testid="obscure-blackout"]')
    await dragOnStage(page, at(redaction.topLeft), at(redaction.bottomRight))
    await expectLayers(page, 1)

    // 2. A frame around the one figure that is going the wrong way.
    const frame = inflate(marks.churn, 5, 5)
    await page.click('[data-testid="color-ffcc00"]')
    await page.click('[data-testid="tool-rect"]')
    await dragOnStage(page, at(frame.topLeft), at(frame.bottomRight))
    await expectLayers(page, 2)

    // 3. An arrow at the number worth looking at, starting in the empty middle
    //    of the page header so the shaft crosses nothing it has to be read over.
    await page.click('[data-testid="color-ff3b30"]')
    await page.click('[data-testid="tool-arrow"]')
    const tail = {
      x: (marks.title.x + marks.title.width + marks.nav.x) / 2,
      y: marks.header.y + marks.header.height / 2,
    }
    const head = { x: marks.growth.x + marks.growth.width / 2, y: marks.growth.y - 8 }
    await dragOnStage(page, at(tail), at(head))
    await expectLayers(page, 3)

    // 4. Select the rectangle, so the picture also shows what a selected layer
    //    looks like: the outline and the eight resize handles, which are the
    //    editor's own chrome and never reach the exported file. An unfilled
    //    shape is hit by its edge rather than by its box, so the press is on
    //    the middle of its top edge.
    await page.click('[data-testid="tool-select"]')
    const onFrameEdge = at({ x: (frame.topLeft.x + frame.bottomRight.x) / 2, y: frame.topLeft.y })
    await page.mouse.click(onFrameEdge.x, onFrameEdge.y)
    await page.waitForFunction(
      () => document.querySelector('[data-testid="selected-kind"]')?.textContent === 'rect selected',
      undefined,
      { timeout: TIMEOUT },
    )

    return await shoot(page, 'editor.png')
  } finally {
    await context.close()
  }
}

/**
 * The selection overlay, with a region drawn out and the magnifier up.
 *
 * The component is `apps/desktop/src/overlay/Overlay.tsx` unchanged. It does
 * reach for Tauri, so `src/tauri.ts` answers the three calls it makes before
 * its first paint; region mode issues no command of its own until `Enter`
 * confirms a selection, and nothing here presses `Enter`.
 */
async function captureOverlay(browser, base, demo) {
  const scene = { image: demo.image, width: SCENE.width, height: SCENE.height }
  const { context, page } = await open(browser, base, '/overlay.html', SCENE, scene)
  try {
    await page.waitForFunction(() => document.querySelector('img')?.complete === true, undefined, {
      timeout: TIMEOUT,
    })

    const rect = inflate(demo.marks.chart, 6, 6)
    await dragOnStage(page, rect.topLeft, rect.bottomRight)

    // The magnifier waits on a second decode of the frame, which the overlay
    // starts only once the window has shown itself, and it needs a pointer
    // move after that to have a colour to report. So the pointer is nudged
    // until the hex readout appears rather than after a fixed pause.
    // Parked on one of the keys rather than where the drag ended. The
    // magnifier reports the pixel under the pointer, and a pointer left on the
    // flat grey between two cards makes a feature that reads pixels look like
    // an empty box.
    const key = demo.marks.firstKey
    const resting = { x: key.x + key.width / 2, y: key.y + key.height / 2 }
    await nudgeUntil(page, resting, () =>
      page.evaluate(() => /#[0-9a-f]{6}/i.test(document.body.innerText)),
    )

    return await shoot(page, 'overlay.png')
  } finally {
    await context.close()
  }
}

/**
 * The settings window, at the width `settings_window.rs` builds it and as tall
 * as its own contents.
 *
 * The height is reached by growing the window rather than by asking for a
 * full-page screenshot. The form ends in a footer that is `position: sticky`,
 * and a full-page capture of a sticky element photographs it where it is pinned
 * in the viewport, which lands the Save button across the middle of the page.
 * A window tall enough to hold the form has no scroll for the footer to stick
 * against, so it is drawn where it belongs.
 */
async function captureSettings(browser, base, demo) {
  const scene = { image: demo.image, width: SCENE.width, height: SCENE.height }
  const width = 460
  const { context, page } = await open(
    browser,
    base,
    '/settings.html',
    { width, height: 620 },
    scene,
  )
  try {
    await page.waitForFunction(
      () => document.body.innerText.includes('Shortcuts'),
      undefined,
      { timeout: TIMEOUT },
    )
    const height = await page.evaluate(() => document.documentElement.scrollHeight)
    await page.setViewportSize({ width, height })
    await page.waitForFunction(
      (expected) => document.documentElement.scrollHeight <= expected,
      height,
      { timeout: TIMEOUT },
    )
    return await shoot(page, 'settings.png')
  } finally {
    await context.close()
  }
}

/** A press, a drag and a release, with moves in between so a drag is a drag. */
async function dragOnStage(page, from, to) {
  await page.mouse.move(from.x, from.y)
  await page.mouse.down()
  const steps = 12
  for (let step = 1; step <= steps; step += 1) {
    await page.mouse.move(
      from.x + ((to.x - from.x) * step) / steps,
      from.y + ((to.y - from.y) * step) / steps,
    )
  }
  await page.mouse.up()
}

/**
 * Moves the pointer a point back and forth around `point` until `ready` says
 * the surface has caught up.
 *
 * For the one thing here that cannot be waited on directly: a value that only
 * arrives on a pointer move, behind an asynchronous decode that has not
 * finished yet when the first move happens.
 */
async function nudgeUntil(page, point, ready) {
  for (let attempt = 0; attempt < 150; attempt += 1) {
    await page.mouse.move(point.x + (attempt % 2), point.y)
    if (await ready()) return
    await page.waitForTimeout(100)
  }
  throw new Error('screenshots: the overlay never reported a colour under the pointer')
}

/** The box of a `data-testid`, in CSS points from the top-left of the page. */
async function boxOf(page, testId) {
  const box = await page.locator(`[data-testid="${testId}"]`).boundingBox()
  if (!box) throw new Error(`screenshots: [data-testid="${testId}"] has no box`)
  return box
}

/** Fails the run unless the editor's own status bar counts `expected` layers. */
async function expectLayers(page, expected) {
  await page.waitForFunction(
    (count) => document.querySelector('[data-testid="layer-count"]')?.textContent === `${count} annotations`,
    expected,
    { timeout: TIMEOUT },
  )
}

/** A rect grown by a margin, as the two corners a drag is made of. */
function inflate(rect, x, y) {
  return {
    topLeft: { x: rect.x - x, y: rect.y - y },
    bottomRight: { x: rect.x + rect.width + x, y: rect.y + rect.height + y },
  }
}

/** Takes the picture and reports what was written. */
async function shoot(page, name, options = {}) {
  const failures = page.__failures ?? []
  if (failures.length > 0) {
    throw new Error(`screenshots: ${name} was not written, the page raised ${failures.join('; ')}`)
  }
  const path = resolve(IMAGES, name)
  const image = await page.screenshot({ type: 'png', ...options })
  await writeFile(path, image)
  const size = await page.evaluate(() => ({
    width: document.documentElement.scrollWidth,
    height: document.documentElement.scrollHeight,
  }))
  return {
    path: `docs/images/${name}`,
    width: size.width * DENSITY,
    height: size.height * DENSITY,
    bytes: image.byteLength,
  }
}

await main()
