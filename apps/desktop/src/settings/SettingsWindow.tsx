/**
 * The settings window.
 *
 * Rust owns every value in here. The window renders nothing until
 * `get_settings` has answered, and it never invents a starting value of its
 * own: a field initialised to `''` or to `'png'` would be a second place a
 * default is written, which is the one thing `settings.rs` exists to prevent.
 * The empty first frame is the honest one.
 *
 * A save is one transaction. Everything on the form goes to `save_settings`
 * together, and what comes back is what is actually in force, which is not
 * always what was asked for: a rebind the platform refuses leaves the previous
 * bindings and says so. Rendering the answer rather than the request is what
 * keeps the window from showing a shortcut that does not work.
 */

import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useCallback, useEffect, useState } from 'react'
import type { CSSProperties, JSX, ReactNode } from 'react'
import { bindingFromKeyPress, formatBinding } from './shortcut'

/** Mirrors `settings::SaveFormat`, which serialises in camel case. */
export type SaveFormat = 'png' | 'jpeg'

/** Mirrors `shortcuts::Shortcuts`. */
export type Shortcuts = {
  captureRegion: string
  captureWindow: string
  captureDisplay: string
}

/** Mirrors `settings::Settings`. */
export type Settings = {
  saveDirectory: string | null
  filenameTemplate: string
  defaultFormat: SaveFormat
  shortcuts: Shortcuts
  launchAtLogin: boolean
  openEditorAfterCapture: boolean
}

/** The capture modes, in the order the tray menu lists them. */
const SHORTCUT_ROWS: { key: keyof Shortcuts; label: string }[] = [
  { key: 'captureRegion', label: 'Capture region' },
  { key: 'captureWindow', label: 'Capture window' },
  { key: 'captureDisplay', label: 'Capture full screen' },
]

const FORMAT_ROWS: { value: SaveFormat; label: string; note: string }[] = [
  { value: 'png', label: 'PNG', note: 'Lossless, and what text stays readable in.' },
  { value: 'jpeg', label: 'JPEG', note: 'Much smaller, at some cost around text.' },
]

/** What a message under the form is: something that worked, or something that did not. */
type Notice = { kind: 'ok' | 'error'; text: string }

export function SettingsWindow(): JSX.Element {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [notice, setNotice] = useState<Notice | null>(null)
  const [saving, setSaving] = useState(false)
  const [recording, setRecording] = useState<keyof Shortcuts | null>(null)

  // Snapdeck is a menu bar agent and is never the active application, so
  // without this the window opens behind whatever the user was looking at and
  // `Settings…` appears to have done nothing. Asked for here rather than by the
  // window builder because AppKit ignores a focus request made before the
  // window has been composited; the editor measured the same thing.
  useEffect(() => {
    getCurrentWindow()
      .setFocus()
      .catch((error: unknown) => {
        console.error('settings: the window could not take focus', error)
      })
  }, [])

  useEffect(() => {
    let abandoned = false
    invoke<Settings>('get_settings')
      .then((loaded) => {
        if (!abandoned) setSettings(loaded)
      })
      .catch((error: unknown) => {
        if (!abandoned) {
          setNotice({ kind: 'error', text: `Snapdeck could not read its settings. ${String(error)}` })
        }
      })
    return () => {
      abandoned = true
    }
  }, [])

  // The recorder. Bound to the window rather than to the button, because the
  // combinations worth binding are the ones macOS would otherwise route
  // somewhere else, and `preventDefault` on a captured keydown is what stops
  // the webview acting on them while the user is choosing.
  useEffect(() => {
    if (!recording) return
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault()
      if (event.code === 'Escape') {
        setRecording(null)
        return
      }
      const binding = bindingFromKeyPress(event)
      if (!binding) return
      setSettings((current) =>
        current ? { ...current, shortcuts: { ...current.shortcuts, [recording]: binding } } : current,
      )
      setRecording(null)
      setNotice(null)
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [recording])

  const update = useCallback((change: Partial<Settings>) => {
    setNotice(null)
    setSettings((current) => (current ? { ...current, ...change } : current))
  }, [])

  const chooseFolder = useCallback(async () => {
    setNotice(null)
    try {
      const chosen = await invoke<string | null>('choose_save_directory')
      // `null` is a cancelled picker, which is not a failure and must not look
      // like one.
      if (chosen !== null) update({ saveDirectory: chosen })
    } catch (error: unknown) {
      setNotice({ kind: 'error', text: String(error) })
    }
  }, [update])

  const save = useCallback(async () => {
    if (!settings) return
    setSaving(true)
    setNotice(null)
    try {
      // What comes back is what is in force, which is the thing worth
      // rendering: a refused rebind answers with the bindings that survived.
      const inForce = await invoke<Settings>('save_settings', { settings })
      setSettings(inForce)
      setNotice({ kind: 'ok', text: 'Saved. The new settings are in force now.' })
    } catch (error: unknown) {
      setNotice({ kind: 'error', text: String(error) })
      // Nothing was changed, so the form has to go back to showing what is
      // actually in force rather than the request that was refused.
      try {
        setSettings(await invoke<Settings>('get_settings'))
      } catch {
        // Leave the form as it is: the error above is the one that matters,
        // and replacing it with a second one would only bury it.
      }
    } finally {
      setSaving(false)
    }
  }, [settings])

  if (!settings) {
    return (
      <main style={pageStyle}>
        <p style={statusStyle} role="status">
          {notice?.text ?? 'Reading your settings…'}
        </p>
      </main>
    )
  }

  return (
    <main style={pageStyle}>
      <Section title="Shortcuts" hint="Click a shortcut, then press the combination you want. Escape cancels.">
        {SHORTCUT_ROWS.map(({ key, label }) => (
          <Row key={key} label={label}>
            <button
              type="button"
              style={recording === key ? recordingButtonStyle : shortcutButtonStyle}
              aria-pressed={recording === key}
              onClick={() => setRecording(recording === key ? null : key)}
            >
              {recording === key ? 'Press a combination…' : formatBinding(settings.shortcuts[key])}
            </button>
          </Row>
        ))}
      </Section>

      <Section title="Saving">
        <Row label="Folder">
          <div style={folderStyle}>
            <span style={pathStyle} title={settings.saveDirectory ?? undefined}>
              {settings.saveDirectory ?? 'Your Pictures folder'}
            </span>
            <span style={buttonRowStyle}>
              <button type="button" style={buttonStyle} onClick={chooseFolder}>
                Choose…
              </button>
              {settings.saveDirectory !== null && (
                <button
                  type="button"
                  style={buttonStyle}
                  onClick={() => update({ saveDirectory: null })}
                >
                  Reset
                </button>
              )}
            </span>
          </div>
        </Row>
        <Row label="File name" hint="{date}, {time}, {width} and {height} are replaced.">
          <input
            type="text"
            aria-label="File name template"
            style={inputStyle}
            value={settings.filenameTemplate}
            spellCheck={false}
            onChange={(event) => update({ filenameTemplate: event.target.value })}
          />
        </Row>
        <Row label="Format">
          <span style={buttonRowStyle}>
            {FORMAT_ROWS.map(({ value, label, note }) => (
              <button
                key={value}
                type="button"
                title={note}
                aria-pressed={settings.defaultFormat === value}
                style={settings.defaultFormat === value ? selectedButtonStyle : buttonStyle}
                onClick={() => update({ defaultFormat: value })}
              >
                {label}
              </button>
            ))}
          </span>
        </Row>
      </Section>

      <Section title="Behaviour">
        <Check
          label="Open the editor after a capture"
          checked={settings.openEditorAfterCapture}
          onChange={(openEditorAfterCapture) => update({ openEditorAfterCapture })}
        />
        <Check
          label="Launch Snapdeck at login"
          checked={settings.launchAtLogin}
          onChange={(launchAtLogin) => update({ launchAtLogin })}
        />
      </Section>

      <footer style={footerStyle}>
        <p style={notice?.kind === 'error' ? errorStyle : okStyle} role="status">
          {notice?.text ?? ''}
        </p>
        <button type="button" style={primaryButtonStyle} disabled={saving} onClick={save}>
          {saving ? 'Saving…' : 'Save'}
        </button>
      </footer>
    </main>
  )
}

function Section({
  title,
  hint,
  children,
}: {
  title: string
  hint?: string
  children: ReactNode
}): JSX.Element {
  return (
    <section style={sectionStyle}>
      <h2 style={headingStyle}>{title}</h2>
      {hint && <p style={hintStyle}>{hint}</p>}
      {children}
    </section>
  )
}

/**
 * A labelled row.
 *
 * A `div` and a `span`, not a `label`. A row holds two buttons as often as it
 * holds one control, and a `label` wrapping them makes a click anywhere on the
 * row press the first of the two: clicking the words "Folder" would open the
 * folder picker. The one control that needs a name for a screen reader carries
 * its own `aria-label`.
 */
function Row({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: ReactNode
}): JSX.Element {
  return (
    <div style={rowStyle}>
      <span style={labelStyle}>
        {label}
        {hint && <span style={rowHintStyle}>{hint}</span>}
      </span>
      {children}
    </div>
  )
}

function Check({
  label,
  checked,
  onChange,
}: {
  label: string
  checked: boolean
  onChange: (checked: boolean) => void
}): JSX.Element {
  return (
    <label style={checkStyle}>
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      {label}
    </label>
  )
}

const FONT = '13px -apple-system, system-ui, "Helvetica Neue", sans-serif'

const pageStyle: CSSProperties = {
  font: FONT,
  color: '#1d1d1f',
  background: '#f5f5f7',
  minHeight: '100%',
  boxSizing: 'border-box',
  padding: '18px 22px 0',
  display: 'flex',
  flexDirection: 'column',
  gap: 18,
}

const statusStyle: CSSProperties = { color: '#6e6e73', margin: 0 }

const sectionStyle: CSSProperties = {
  background: '#ffffff',
  border: '1px solid #e0e0e5',
  borderRadius: 10,
  padding: '12px 14px',
  display: 'flex',
  flexDirection: 'column',
  gap: 10,
}

const headingStyle: CSSProperties = { font: FONT, fontWeight: 600, margin: 0 }
const hintStyle: CSSProperties = { color: '#6e6e73', margin: 0, lineHeight: 1.4 }
const rowHintStyle: CSSProperties = { color: '#6e6e73', display: 'block', fontSize: 11 }

const rowStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '132px 1fr',
  alignItems: 'center',
  gap: 12,
}

const labelStyle: CSSProperties = { color: '#3c3c43' }

const checkStyle: CSSProperties = { display: 'flex', alignItems: 'center', gap: 8 }

const buttonStyle: CSSProperties = {
  font: FONT,
  padding: '4px 10px',
  borderRadius: 6,
  border: '1px solid #c6c6cc',
  background: '#ffffff',
  color: '#1d1d1f',
  cursor: 'pointer',
}

const selectedButtonStyle: CSSProperties = {
  ...buttonStyle,
  background: '#0071e3',
  borderColor: '#0071e3',
  color: '#ffffff',
}

const shortcutButtonStyle: CSSProperties = {
  ...buttonStyle,
  fontSize: 14,
  minWidth: 96,
  justifySelf: 'start',
}

const recordingButtonStyle: CSSProperties = {
  ...shortcutButtonStyle,
  borderColor: '#0071e3',
  color: '#0071e3',
}

const primaryButtonStyle: CSSProperties = { ...selectedButtonStyle, padding: '5px 16px' }

const inputStyle: CSSProperties = {
  font: FONT,
  padding: '4px 8px',
  borderRadius: 6,
  border: '1px solid #c6c6cc',
  background: '#ffffff',
  color: '#1d1d1f',
}

const folderStyle: CSSProperties = { display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }

// Truncated at the end rather than at the start, and left to the `title`
// attribute to show in full. Starting the ellipsis at the front would need
// `direction: rtl`, which reorders the punctuation in the path it is drawing.
const pathStyle: CSSProperties = {
  flex: '1 1 auto',
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  color: '#3c3c43',
}

const buttonRowStyle: CSSProperties = { display: 'flex', gap: 6, flex: '0 0 auto' }

const footerStyle: CSSProperties = {
  position: 'sticky',
  bottom: 0,
  background: '#f5f5f7',
  padding: '12px 0 16px',
  display: 'flex',
  alignItems: 'center',
  gap: 12,
  marginTop: 'auto',
}

const okStyle: CSSProperties = { margin: 0, flex: '1 1 auto', color: '#6e6e73', lineHeight: 1.4 }
const errorStyle: CSSProperties = { ...okStyle, color: '#c1121f' }
