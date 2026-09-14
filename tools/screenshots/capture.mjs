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
 * so the thing under the annotations can be read and re-made. It is rendered
 * twice, at two sizes, because the store's screenshots have a shape of their
 * own and a picture stretched into it would be a picture of a stretched page.
 *
 * Those renders are then handed to pages under `tools/screenshots`, each of
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
 *
 * `docs/images/store` is the exception to all of the above, and the one picture
 * here that is not a product surface: a brand tile the Chrome Web Store demands
 * at a fixed size. It draws no interface and invents no screen; it is the
 * extension's own icon, its own name and the description already in its own
 * manifest.
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
const EXTENSION_ICON = resolve(REPO, 'apps/extension/public/icons/128.png')

/**
 * The demo render, in pixels.
 *
 * Also its size in CSS points, because it is rendered at one device pixel per
 * point: the editor then shows it at 1:1, which is the only scale at which a
 * redaction in the preview is the redaction in the file. `render.ts` says why.
 */
const SCENE = { width: 1100, height: 720 }

/**
 * The size the Chrome Web Store takes a screenshot at, and the second size the
 * demo page is rendered at.
 *
 * Rendered rather than cropped. A centre crop of a picture of the editor cuts
 * the toolbar or the status bar off, which are the two things the picture is of;
 * rendering the interface into 1280x800 lets it lay itself out at that size, the
 * way it would in a window of that shape. The overlay needs the second demo
 * render for a plainer reason: its backdrop fills the window, so a frozen frame
 * of a different shape would be stretched.
 */
const STORE_SHOT = { width: 1280, height: 800 }

/** The small promotional tile the store requires, in pixels. */
const STORE_TILE = { width: 440, height: 280 }

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

/** The density the README's pictures are taken at. */
const DENSITY = 2

/**
 * The density the store's are taken at.
 *
 * One, not two. The store asks for 1280x800 and means pixels; a 2x capture is
 * 2560x1600 and is refused.
 */
const STORE_DENSITY = 1

/** How long any one wait is given before the run is called failed. */
const TIMEOUT = 15_000

async function main() {
  await mkdir(resolve(IMAGES, 'store'), { recursive: true })

  const server = await createServer({
    configFile: resolve(HERE, 'vite.config.ts'),
    logLevel: 'warn',
    server: { port: 0 },
  })
  await server.listen()
  const base = server.resolvedUrls?.local?.[0]
  if (!base) throw new Error('screenshots: the dev server reported no address')

  const icon = await dataUrl(EXTENSION_ICON)
  const browser = await chromium.launch({ headless: true })
  const written = []
  try {
    const demo = await renderDemoPage(browser, SCENE, icon)
    const wide = await renderDemoPage(browser, STORE_SHOT, icon)

    written.push(await captureEditor(browser, base, demo, { name: 'editor.png' }))
    written.push(await captureOverlay(browser, base, demo, { name: 'overlay.png' }))
    written.push(await captureSettings(browser, base, demo))

    // The same editor and the same three annotations, in a window of the
    // store's shape. The picture inside it is a third render of the demo page,
    // cut to the room the editor's own toolbar and status bar leave at that
    // width, so the capture fills the window and is still shown at 1:1. The
    // editor never enlarges a picture past its own resolution, so a scene of
    // any other size would either be centred in a band of empty window or shown
    // under 1:1, and a store screenshot of a screenshot tool should not be the
    // one picture on the page that is soft.
    const room = STORE_SHOT.height - (await measureEditorChrome(browser, base, demo, STORE_SHOT))
    const fitted = await renderDemoPage(browser, { width: STORE_SHOT.width, height: room }, icon)
    written.push(
      await captureEditor(browser, base, fitted, {
        name: 'store/screenshot-editor-1280x800.png',
        viewport: STORE_SHOT,
        density: STORE_DENSITY,
      }),
    )
    written.push(
      // No size is passed: the overlay's window is always its frozen frame's
      // own size, and `wide` is the frame rendered at the store's.
      await captureOverlay(browser, base, wide, {
        name: 'store/screenshot-overlay-1280x800.png',
        density: STORE_DENSITY,
      }),
    )
    written.push(await capturePromoTile(browser, base, wide))
  } finally {
    await browser.close()
    await server.close()
  }

  for (const { path, width, height, bytes } of written) {
    console.log(`${path}  ${width}x${height}  ${(bytes / 1024).toFixed(0)} KB`)
  }
}

/** A file on disk, as a `data:` URL a page can be handed. */
async function dataUrl(path) {
  const bytes = await readFile(path)
  return `data:image/png;base64,${bytes.toString('base64')}`
}

/**
 * The demo page, rendered at `size`, plus the boxes the annotations are aimed
 * at.
 *
 * The measurements are taken here, in the page itself, and travel as numbers.
 * They are in the render's own pixel space, which is the editor's document
 * space, so nothing downstream has to know how the page is laid out. They are
 * taken per render rather than once, because the page is responsive and a
 * second size lays its cards out somewhere else.
 */
async function renderDemoPage(browser, size, icon) {
  const html = await readFile(DEMO_PAGE, 'utf8')
  const context = await browser.newContext({ viewport: size, deviceScaleFactor: 1 })
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
    return {
      image: `data:image/png;base64,${image.toString('base64')}`,
      width: size.width,
      height: size.height,
      icon,
      marks,
    }
  } finally {
    await context.close()
  }
}

/** A page with the scene installed before any of its own code runs. */
async function open(browser, base, file, { viewport, density, demo }) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: density })
  const page = await context.newPage()
  // Collected rather than thrown from the listener: a throw inside an event
  // handler becomes an unhandled rejection with no stack pointing here, where
  // this way `shoot` refuses to write a picture of a page that failed.
  const failures = []
  page.on('pageerror', (error) => failures.push(error.message))
  page.__failures = failures
  page.__density = density
  await page.addInitScript(
    (value) => {
      window.__SNAPDECK_SCENE__ = value
    },
    { image: demo.image, width: demo.width, height: demo.height, icon: demo.icon },
  )
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
 *
 * With no `viewport` the window is sized to the picture, which is what the app
 * itself does. With one, the window is that size and the editor lays itself out
 * inside it; either way the canvas has to come out at 1:1, which is asserted
 * rather than assumed, because every coordinate below is a document coordinate.
 */
async function captureEditor(browser, base, demo, { name, viewport, density = DENSITY }) {
  const fitted = viewport ?? { width: demo.width, height: demo.height + EDITOR_CHROME }
  const { context, page } = await open(browser, base, '/editor.html', {
    viewport: fitted,
    density,
    demo,
  })
  try {
    await page.waitForSelector('[data-testid="canvas"]', { timeout: TIMEOUT })

    if (!viewport) {
      // `EDITOR_CHROME` is the app's own guess at the toolbar and the status
      // bar; this is the measurement that makes it true, so the stage is
      // exactly the picture and the editor's fit lands on 1:1 with no margin.
      const stage = await boxOf(page, 'stage')
      const missing = demo.height - stage.height
      if (missing !== 0) {
        await page.setViewportSize({
          width: fitted.width,
          height: fitted.height + Math.ceil(missing),
        })
      }
    }
    await page.waitForFunction(
      (expected) => {
        const canvas = document.querySelector('[data-testid="canvas"]')
        if (!canvas) return false
        const rect = canvas.getBoundingClientRect()
        return Math.abs(rect.width - expected.width) < 1 && Math.abs(rect.height - expected.height) < 1
      },
      { width: demo.width, height: demo.height },
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

    return await shoot(page, name)
  } finally {
    await context.close()
  }
}

/**
 * How many points of a window of this size the editor spends on its own
 * toolbar and status bar.
 *
 * Measured rather than written down, because it is not one number: the toolbar
 * wraps, so the answer depends on how wide the window is, and it changes again
 * the day a control is added to it. Everything that needs a picture cut to the
 * editor's stage asks here first.
 */
async function measureEditorChrome(browser, base, demo, viewport) {
  const { context, page } = await open(browser, base, '/editor.html', {
    viewport,
    density: STORE_DENSITY,
    demo,
  })
  try {
    await page.waitForSelector('[data-testid="stage"]', { timeout: TIMEOUT })
    const stage = await boxOf(page, 'stage')
    return Math.ceil(viewport.height - stage.height)
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
 *
 * The window is the frozen frame's own size, always. The backdrop is an `<img>`
 * filling the window, so a window of any other shape would stretch the frame,
 * and the overlay's coordinates would stop meaning what the measurements say.
 */
async function captureOverlay(browser, base, demo, { name, density = DENSITY }) {
  const { context, page } = await open(browser, base, '/overlay.html', {
    viewport: { width: demo.width, height: demo.height },
    density,
    demo,
  })
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
    //
    // Parked on one of the keys rather than where the drag ended. The
    // magnifier reports the pixel under the pointer, and a pointer left on the
    // flat grey between two cards makes a feature that reads pixels look like
    // an empty box.
    const key = demo.marks.firstKey
    const resting = { x: key.x + key.width / 2, y: key.y + key.height / 2 }
    await nudgeUntil(page, resting, () =>
      page.evaluate(() => /#[0-9a-f]{6}/i.test(document.body.innerText)),
    )

    return await shoot(page, name)
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
  const width = 460
  const { context, page } = await open(browser, base, '/settings.html', {
    viewport: { width, height: 620 },
    density: DENSITY,
    demo,
  })
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

/**
 * The small promotional tile, 440x280, which the Chrome Web Store requires and
 * which nothing else in this repository is the size of.
 *
 * The one picture here that is not a picture of the product running. It draws
 * no interface: a store tile that shows a mocked-up screen is a drawing of
 * software rather than the software, and this listing has real screenshots for
 * that. What is on it is the extension's own icon file, its own name, and the
 * description already written in its own `manifest.json`, on the accent colour
 * the editor and the settings window use for a pressed control. Full bleed,
 * because the store asks for no padding and no white border.
 */
async function capturePromoTile(browser, base, demo) {
  const { context, page } = await open(browser, base, '/promo.html', {
    viewport: STORE_TILE,
    density: STORE_DENSITY,
    demo,
  })
  try {
    await page.waitForFunction(
      () => document.querySelector('img')?.complete === true,
      undefined,
      { timeout: TIMEOUT },
    )
    await page.evaluate(async () => {
      await document.fonts.ready
    })
    return await shoot(page, 'store/promo-440x280.png')
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
async function shoot(page, name) {
  const failures = page.__failures ?? []
  if (failures.length > 0) {
    throw new Error(`screenshots: ${name} was not written, the page raised ${failures.join('; ')}`)
  }
  const path = resolve(IMAGES, name)
  const image = await page.screenshot({ type: 'png' })
  await writeFile(path, image)
  const density = page.__density ?? 1
  const size = await page.evaluate(() => ({
    width: document.documentElement.scrollWidth,
    height: document.documentElement.scrollHeight,
  }))
  return {
    path: `docs/images/${name}`,
    width: size.width * density,
    height: size.height * density,
    bytes: image.byteLength,
  }
}

await main()
