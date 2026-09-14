/**
 * The editor page of the harness: the shipping `<Editor/>`, mounted over the
 * demo render.
 *
 * The component is taken from `@snapdeck/editor` untouched. That is the whole
 * point of the picture: it is deliberately free of Tauri, so a browser is a
 * host it already supports, and what a reader sees in the README is the real
 * toolbar drawing real layers rather than a drawing of one.
 *
 * `onExport`, `onCopy` and `onClose` are the props a host has to supply, and
 * nothing here presses the controls that reach them, so they are inert on
 * purpose: the harness draws and selects, and never saves.
 */

import { Editor } from '@snapdeck/editor'
import { createRoot } from 'react-dom/client'
import { sceneImage, scene } from './scene'

const root = document.getElementById('editor-root')
if (!root) throw new Error('screenshots: #editor-root is missing')

const { width, height } = scene()
const image = await sceneImage()

createRoot(root).render(
  // No `StrictMode`, unlike the app's own entry points. A double mount would
  // build the editor's history twice and leave the harness driving the second
  // one while the first is still on screen for a frame; the app wants that
  // check and a screenshot does not.
  <Editor
    image={image}
    width={width}
    height={height}
    onExport={() => undefined}
    onCopy={() => undefined}
    onClose={() => undefined}
  />,
)
