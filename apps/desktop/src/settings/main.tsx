import React from 'react'
import { createRoot } from 'react-dom/client'
import { SettingsWindow } from './SettingsWindow'

const root = document.getElementById('settings-root')
if (!root) throw new Error('settings: #settings-root is missing')

createRoot(root).render(
  <React.StrictMode>
    <SettingsWindow />
  </React.StrictMode>,
)
