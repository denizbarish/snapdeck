/**
 * Turning a document into a file.
 *
 * Thin on purpose. Everything that decides what the picture looks like lives in
 * `renderDocument`, and export's whole job is to give it a canvas of the right
 * size and hand the result to the encoder. There is no export-only drawing code
 * here, which is why the exported file cannot disagree with the preview.
 *
 * What leaves this module is pixels. The document, the layers, their ids and
 * the coordinates of the region a user redacted are never written to the file:
 * a canvas encodes its bitmap and nothing else, so there is no metadata channel
 * for any of it to survive in.
 */

import type { EditorDocument } from './model'
import { renderDocument, viewOf } from './render'

/**
 * The formats a finished picture can leave in.
 *
 * Two, and WebP is deliberately not the third. A canvas is the only encoder
 * this package has, and WKWebView's canvas does not encode WebP: asking for it
 * returns PNG bytes, silently, which the guard in `toBlob` now refuses. A
 * format that can only ever fail is worse than one that is not offered, so it
 * is not offered.
 *
 * PNG is lossless and is what a capture already is, so it is the default and
 * the one Save re-writes in place. JPEG is here because a full-resolution
 * screenshot is a large file to send someone.
 */
export type ExportType = 'image/png' | 'image/jpeg'

/**
 * Render a document to an offscreen canvas at full source resolution.
 *
 * No transform is set, so one source pixel is one output pixel and the canvas
 * is the size of the crop, or of the whole capture when there is no crop.
 *
 * The size is taken from `viewOf` unrounded, because `viewOf` has already
 * rounded it: it is the same call the renderer translates by, so the canvas and
 * the origin drawn into it cannot disagree about where a fractional crop
 * starts. Rounding again here is how they used to.
 */
export function exportCanvas(image: CanvasImageSource, doc: EditorDocument): OffscreenCanvas {
  const view = viewOf(doc)
  const canvas = new OffscreenCanvas(view.width, view.height)
  const ctx = canvas.getContext('2d')
  // Only ever null if the context was already claimed by another mode or the
  // platform refused one. Nothing sane to fall back to, and falling back
  // silently would mean exporting an unannotated, unredacted screenshot.
  if (!ctx) throw new Error('exportCanvas: could not get a 2D context')
  renderDocument(ctx, image, doc)
  return canvas
}

/**
 * Encode a document to an image file.
 *
 * `quality` is ignored by the PNG encoder and applies to JPEG, which is the
 * platform's rule rather than one imposed here; it is passed through untouched
 * so the caller's number means what the spec says it means.
 *
 * A format the platform will not encode is refused rather than substituted.
 * `convertToBlob` is specified to fall back to PNG when it cannot honour the
 * type it was given, and it does so silently: measured on the packaged macOS
 * build, asking WKWebView for `image/webp` returns a blob of PNG bytes. The
 * caller names the file after the format it asked for, so without this check
 * the app writes PNG under `.webp` and hands the user a file whose extension
 * is a lie. WebP is no longer on `ExportType` for exactly that reason, and the
 * check stays anyway: it is the type system's word against the encoder's, and
 * the encoder is the one holding the bytes. Refusing surfaces in the editor's
 * own status bar, where a save that did not happen is visible; the substitution
 * was not visible anywhere.
 */
export async function toBlob(
  image: CanvasImageSource,
  doc: EditorDocument,
  type: ExportType,
  quality?: number,
): Promise<Blob> {
  const blob = await exportCanvas(image, doc).convertToBlob({ type, quality })
  if (blob.type !== type) {
    throw new Error(`toBlob: this platform encodes ${blob.type} rather than the requested ${type}`)
  }
  return blob
}
