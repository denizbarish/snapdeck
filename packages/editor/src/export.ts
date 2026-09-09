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
 * Render a document to an offscreen canvas at full source resolution.
 *
 * No transform is set, so one source pixel is one output pixel and the canvas
 * is the size of the crop, or of the whole capture when there is no crop.
 * Dimensions are rounded and floored at one: a canvas cannot have a fractional
 * or zero side, and a drag that produced a sliver of a crop should still yield
 * a file rather than an exception.
 */
export function exportCanvas(image: CanvasImageSource, doc: EditorDocument): OffscreenCanvas {
  const view = viewOf(doc)
  const canvas = new OffscreenCanvas(
    Math.max(1, Math.round(view.width)),
    Math.max(1, Math.round(view.height)),
  )
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
 * `quality` is ignored by the PNG encoder and applies to JPEG and WebP, which
 * is the platform's rule rather than one imposed here; it is passed through
 * untouched so the caller's number means what the spec says it means.
 */
export async function toBlob(
  image: CanvasImageSource,
  doc: EditorDocument,
  type: 'image/png' | 'image/jpeg' | 'image/webp',
  quality?: number,
): Promise<Blob> {
  return exportCanvas(image, doc).convertToBlob({ type, quality })
}
