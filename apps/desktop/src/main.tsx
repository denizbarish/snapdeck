import React from 'react'
import { createRoot } from 'react-dom/client'

function App() {
  return <main style={{ fontFamily: 'system-ui', padding: 24 }}>Snapdeck is running.</main>
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
