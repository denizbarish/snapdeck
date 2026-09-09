import React from 'react'
import { createRoot } from 'react-dom/client'
import { EditorWindow } from './EditorWindow'

const params = new URLSearchParams(window.location.search)

/**
 * Every parameter is written by Rust, so a missing one is a bug in the URL, not
 * user input. Throwing keeps it visible in the webview console instead of
 * opening an editor on `Number(null) === 0` pixels of nothing.
 */
function requiredParam(name: string): string {
  const value = params.get(name)
  if (value === null || value === '') {
    throw new Error(`editor: missing "${name}" query parameter in ${window.location.search}`)
  }
  return value
}

function requiredNumberParam(name: string): number {
  const raw = requiredParam(name)
  const value = Number(raw)
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error(`editor: "${name}" query parameter is not a size: ${raw}`)
  }
  return value
}

createRoot(document.getElementById('editor-root')!).render(
  <React.StrictMode>
    <EditorWindow
      path={requiredParam('path')}
      width={requiredNumberParam('width')}
      height={requiredNumberParam('height')}
    />
  </React.StrictMode>,
)
