/**
 * Public surface of the editor model.
 *
 * Host-free by construction: React, Tauri and the DOM all stay on the far side
 * of this boundary, so the Tauri app and the browser extension can share it.
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
