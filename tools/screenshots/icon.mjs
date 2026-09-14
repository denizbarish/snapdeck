/**
 * Rasterises `docs/images/icon-source.svg` into `docs/images/icon-source.png`.
 *
 * `pnpm screenshots` runs this along with everything else, and
 * `pnpm --filter @snapdeck/screenshots icon` runs only this. It lives in its own
 * file rather than inside `capture.mjs` because it is not a picture of the
 * product running: it draws no component, needs no dev server and no demo page,
 * and a change to it cannot break a screenshot or be broken by one. What it
 * shares with `capture.mjs` is the thing worth sharing, which is that nothing on
 * screen is involved: a headless Chromium draws the SVG and the PNG comes out
 * of the browser, so this repository's icon is a text file anyone can edit and
 * re-make rather than a binary handed over once.
 *
 * The PNG is a source, not a shipped asset. The application's `.icns`, its menu
 * bar icon and the extension's PNGs are all generated from it by their own
 * tools, which is why the only size made here is the largest one.
 */

import { chromium } from 'playwright'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const REPO = resolve(HERE, '../..')
const IMAGES = resolve(REPO, 'docs/images')
const SOURCE = resolve(IMAGES, 'icon-source.svg')
const OUTPUT = resolve(IMAGES, 'icon-source.png')

/**
 * The side of the rendered PNG, in pixels.
 *
 * 1024 because that is the largest square macOS asks an icon for, and because
 * the SVG's own grid is 1024: at this size one user unit is one pixel and
 * nothing in the drawing has to land on a fraction.
 */
const SIDE = 1024

/**
 * Renders the icon. Pass a browser to reuse one; it is left open if you do.
 *
 * `omitBackground` is what makes the corners transparent rather than white. The
 * tile does not fill the canvas, and an icon with white shoulders is an icon
 * with a white square behind it on every dark surface it is put on.
 */
export async function renderIcon(browser) {
  const owned = browser ?? (await chromium.launch({ headless: true }))
  const svg = await readFile(SOURCE, 'utf8')
  const context = await owned.newContext({
    viewport: { width: SIDE, height: SIDE },
    deviceScaleFactor: 1,
  })
  try {
    const page = await context.newPage()
    const failures = []
    page.on('pageerror', (error) => failures.push(error.message))
    // The SVG is inlined rather than loaded through an `<img>`, so the file on
    // disk is the only input and there is no second decode to wait on. The page
    // is sized to the render and the SVG is sized to the page, which leaves the
    // drawing at exactly its own scale.
    await page.setContent(
      `<!doctype html><meta charset="utf-8"><style>` +
        `html,body{margin:0;padding:0;width:${SIDE}px;height:${SIDE}px;background:transparent}` +
        `svg{display:block;width:${SIDE}px;height:${SIDE}px}` +
        `</style>${svg}`,
      { waitUntil: 'load' },
    )
    if (failures.length > 0) {
      throw new Error(`icon: nothing was written, the page raised ${failures.join('; ')}`)
    }

    await mkdir(IMAGES, { recursive: true })
    const image = await page.screenshot({ type: 'png', omitBackground: true })
    await writeFile(OUTPUT, image)
    return {
      path: 'docs/images/icon-source.png',
      width: SIDE,
      height: SIDE,
      bytes: image.byteLength,
    }
  } finally {
    await context.close()
    if (!browser) await owned.close()
  }
}

// Run directly: `node icon.mjs`.
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { path, width, height, bytes } = await renderIcon()
  console.log(`${path}  ${width}x${height}  ${(bytes / 1024).toFixed(0)} KB`)
}
