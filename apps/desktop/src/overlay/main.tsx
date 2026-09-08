import React from 'react'
import { createRoot } from 'react-dom/client'
import { Overlay } from './Overlay'

const params = new URLSearchParams(window.location.search)

createRoot(document.getElementById('overlay-root')!).render(
  <React.StrictMode>
    <Overlay
      displayId={Number(params.get('display'))}
      mode={params.get('mode') ?? 'region'}
      scale={Number(params.get('scale') ?? 1)}
    />
  </React.StrictMode>,
)
