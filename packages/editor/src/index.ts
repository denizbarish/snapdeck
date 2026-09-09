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

export type { RenderTarget } from './render'
export { renderDocument } from './render'

export { exportCanvas, toBlob } from './export'

export type { EditorProps } from './Editor'
export { Editor } from './Editor'
