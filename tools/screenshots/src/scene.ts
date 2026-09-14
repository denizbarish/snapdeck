/**
 * What `capture.mjs` hands each page before it runs.
 *
 * The three pages mount the app's own components and nothing else, so
 * everything that would come from Rust or from a capture has to arrive from
 * outside. It arrives on `window`, written by a Playwright init script before
 * any module of the page has evaluated, which is the only point early enough:
 * the overlay asks for its frozen frame in the first effect it runs.
 *
 * The picture is a data URL rather than a file. It is a render of
 * `docs/images/demo-page.html` made moments earlier in the same run, so there
 * is no PNG in the repository for it to go stale against, and a data URL is
 * same-origin, which is what keeps the overlay's magnifier able to read pixels
 * back out of a canvas.
 */

export type Scene = {
  /** The demo page, rendered, as a `data:image/png;base64,...` URL. */
  image: string
  /** Its size in pixels, which is also its size in CSS points here. */
  width: number
  height: number
  /**
   * `apps/extension/public/icons/128.png`, as a data URL.
   *
   * The file the store is given as the extension's icon, read rather than
   * redrawn, so the tile cannot come to show a mark the extension does not
   * ship. Only the promotional tile uses it.
   */
  icon: string
}

declare global {
  interface Window {
    __SNAPDECK_SCENE__?: Scene
  }
}

export function scene(): Scene {
  const value = window.__SNAPDECK_SCENE__
  if (!value) throw new Error('screenshots: no scene was installed on window')
  return value
}

/**
 * The scene's picture, decoded and ready to be drawn.
 *
 * `decode()` rather than an `onload` handler: the editor draws the image on its
 * first render, and an `HTMLImageElement` that has loaded but not decoded still
 * draws nothing on some paths. Waiting here means the page is only mounted once
 * there is something to mount it with.
 */
export async function sceneImage(): Promise<HTMLImageElement> {
  const current = scene()
  const image = new Image()
  image.src = current.image
  await image.decode()
  return image
}
