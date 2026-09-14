/**
 * The settings page of the harness: the shipping `<SettingsWindow/>`, over a
 * stand-in for the one command it needs before it will render anything.
 *
 * The window renders a status line until `get_settings` answers, on purpose:
 * Rust owns every value on the form and the window invents none of its own. So
 * the picture needs an answer, and this file is it. The values are the defaults
 * `settings.rs` writes on a first launch, which is what a reader would see
 * themselves, with one exception.
 *
 * The exception is the pairing token. A real one is 64 hex characters of system
 * randomness and is the whole of what authorises a browser extension to hand
 * this machine a picture. Sixty-four zeros is not a token, cannot be mistaken
 * for one, and is the right shape for the field.
 */

import type { SettingsView } from '@snapdeck/desktop/src/settings/SettingsWindow'
import { SettingsWindow } from '@snapdeck/desktop/src/settings/SettingsWindow'
import { createRoot } from 'react-dom/client'
import { installTauriStub } from './tauri'

/** Not a token. See the note above; it is here to be obviously not one. */
const FAKE_TOKEN = '0'.repeat(64)

const VIEW: SettingsView = {
  settings: {
    // What `settings.rs` means by "no directory chosen": the window prints the
    // Pictures folder for it rather than a path this machine happens to have.
    saveDirectory: null,
    filenameTemplate: 'Snapdeck {date} at {time}',
    defaultFormat: 'png',
    shortcuts: {
      captureRegion: 'CmdOrCtrl+Shift+Digit7',
      captureWindow: 'CmdOrCtrl+Shift+Digit8',
      captureDisplay: 'CmdOrCtrl+Shift+Digit9',
    },
    launchAtLogin: false,
    openEditorAfterCapture: true,
    checkForUpdatesAtLaunch: false,
    bridgeToken: FAKE_TOKEN,
  },
  // The same three the form holds, so the window shows no warning about a
  // keyboard that disagrees with it. A picture of a warning would be a picture
  // of a machine with a conflict on it, which is not what this window is.
  boundShortcuts: {
    captureRegion: 'CmdOrCtrl+Shift+Digit7',
    captureWindow: 'CmdOrCtrl+Shift+Digit8',
    captureDisplay: 'CmdOrCtrl+Shift+Digit9',
  },
  shortcutProblem: null,
  bridgeToken: FAKE_TOKEN,
  bridgeStatus: { state: 'listening', detail: { port: 51837 } },
}

const root = document.getElementById('settings-root')
if (!root) throw new Error('screenshots: #settings-root is missing')

installTauriStub({ get_settings: VIEW }, '')

createRoot(root).render(<SettingsWindow />)
