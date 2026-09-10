/**
 * Public surface of the editor.
 *
 * Everything above the `Editor` line is host-free by construction: no React, no
 * DOM beyond the canvas 2D API, and no Tauri, so the model, the commands, the
 * hit testing and the renderer can be shared with a host that has none of them.
 *
 * `Editor` is the exception and the only one. It is a React component, so a
 * consumer that imports it pays for React; a consumer that imports only the
 * model does not, because nothing above reaches down to it. What `Editor` does
 * not do is know about its host: saving and copying leave through its props as
 * a `Blob`, which is what keeps the desktop shell and the planned browser
 * extension on the same component.
 */

/**
 * The toolbar's minimum width, so a host that decides how large a window may be
 * reads the toolbar's own number rather than keeping a copy of it.
 */
export { TOOLBAR_MIN_WIDTH } from './chrome'

export type {
  BadgeStyle,
  Color,
  EditorDocument,
  Layer,
  Point,
  Rect,
  ShapeStyle,
  StrokeStyle,
  TextStyle,
} from './model'
export { boundsOf, createDocument, nextStepIndex } from './model'

export type { Command } from './commands'
export { addLayer, History, removeLayer, setCrop, updateLayer } from './commands'

export type { Handle } from './hit'
export { handleAtPoint, layerAtPoint, moveLayer, resizeLayer } from './hit'

/**
 * `renderDocument` draws a document into a context the caller owns.
 *
 * Three things a consumer has to know before calling it, because none of them
 * is visible from the signature.
 *
 * It does not clear the target, so a surface drawing a document smaller than
 * its own canvas clears it first.
 *
 * It must not be scrolled by clip: an `obscure` layer writes raw device pixels
 * through `putImageData`, which ignores the clip, so a surface clipped to a
 * viewport would have its redactions land outside that clip. Pan and zoom by
 * setting a transform instead.
 *
 * And it must not be asked to draw an `obscure` layer below 1:1. A mosaic block
 * and a blur radius are measured in source pixels and converted into the device
 * pixels of the target, rounded down and floored at 1, so a transform under one
 * device pixel per source pixel weakens every redaction on the canvas; below
 * roughly 0.17 the block is 1 and the redaction is the identity function. The
 * file is still redacted exactly as asked, because an export renders at source
 * resolution, so the failure is not a leaked file: it is a preview that shows a
 * secret the user believes they have covered, which is the same mistake made a
 * step earlier. A surface that zooms out has to render the document at 1:1 and
 * scale the result down instead. `exportCanvas` is that render, and using it is
 * what keeps the two from disagreeing; Snapdeck's own editor takes that branch
 * whenever the view is under 1:1 and the document carries an `obscure` layer.
 */
export type { RenderTarget } from './render'
export { renderDocument } from './render'

export type { ExportType } from './export'
export { exportCanvas, toBlob } from './export'

export type { EditorProps } from './Editor'
export { Editor } from './Editor'
