import React from 'react'
import { createRoot } from 'react-dom/client'
import { Overlay } from './Overlay'

const params = new URLSearchParams(window.location.search)

/**
 * Every parameter is written by Rust, so a missing one is a bug in the URL, not
 * user input. Throwing keeps it visible in the webview console instead of
 * rendering an overlay built on `Number(null) === 0`.
 */
function requiredParam(name: string): string {
  const value = params.get(name)
  if (value === null || value === '') {
    throw new Error(`overlay: missing "${name}" query parameter in ${window.location.search}`)
  }
  return value
}

function requiredNumberParam(name: string): number {
  const raw = requiredParam(name)
  const value = Number(raw)
  if (!Number.isFinite(value)) {
    throw new Error(`overlay: "${name}" query parameter is not a number: ${raw}`)
  }
  return value
}

createRoot(document.getElementById('overlay-root')!).render(
  <React.StrictMode>
    <Overlay
      displayId={requiredNumberParam('display')}
      mode={requiredParam('mode')}
      scale={requiredNumberParam('scale')}
      framePath={requiredParam('path')}
    />
  </React.StrictMode>,
)
