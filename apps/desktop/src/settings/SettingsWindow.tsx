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
 * together, and it either happens or it does not: a rebind the platform refuses
 * comes back as an `Err`, nothing is written, and the `catch` asks
 * `get_settings` what is actually in force rather than leaving the refused
 * request on screen.
 *
 * What is *in force* is two answers, not one. `boundShortcuts` is what the
 * platform has actually registered, which is `null` when nothing is and a
 * different set when a stored combination had been taken by another
 * application; `settings.shortcuts` is what the file says and what the next
 * save writes back. This window shows the second and tells the truth about the
 * first, because the failure worth preventing here is a page that renders three
 * shortcuts, reports them saved, and leaves the user pressing keys that do
 * nothing.
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
  checkForUpdatesAtLaunch: boolean
}

/**
 * Mirrors `commands::SettingsView`: the settings, what the keyboard actually
 * has, and why those two disagree when they do.
 *
 * `boundShortcuts` is `null` when nothing is registered at all, which is a
 * state the window has to be able to render rather than one it can treat as
 * "no answer yet".
 *
 * `shortcutProblem` is the platform's own reason for refusing a rebind, and
 * only a save produces one. The window can see that the form and the keyboard
 * disagree; it cannot see *why*, and "why" is the half that tells the user
 * whether this is theirs to fix.
 */
export type SettingsView = {
  settings: Settings
  boundShortcuts: Shortcuts | null
  shortcutProblem: string | null
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

/**
 * What to say about the gap between the shortcuts on the form and the ones the
 * platform has actually registered, or `null` when there is no gap.
 *
 * The one thing the window may not do is stay quiet about it. A stored
 * combination another application has taken is still in the settings file,
 * waiting to be changed, while the built-in ones do the work; without this the
 * page renders the stored ones as though pressing them did something.
 *
 * The same sentence covers a shortcut the user has just recorded and not yet
 * saved, which is also not in force, and is also worth saying.
 *
 * `problem` is the reason the last save gave, and it takes over when there is
 * one. It is more specific than anything this function can work out on its own,
 * it already names what is bound instead, and it answers the question the
 * derived sentence cannot: whether the combination is refused because another
 * application holds it, which is not something the user can fix from here. The
 * rest of that save went through, so it says so, because a warning over a form
 * that has just been saved otherwise reads as a save that failed. It is written
 * about the save that produced it rather than about the form as it stands, so
 * that editing something else afterwards cannot make it untrue; the recorder
 * clears it, because a reason for one combination has no business sitting over
 * another.
 *
 * Compared as strings, deliberately. `boundShortcuts` is the very value Rust
 * registered, so anything different came from this form; the question here is
 * "is this the set I was handed", not "is this the same key", which only the
 * shortcut parser in Rust can answer.
 *
 * Exported to be tested: it is the sentence that decides whether the user finds
 * out their keyboard is empty.
 */
export function shortcutNotice(
  shown: Shortcuts,
  bound: Shortcuts | null,
  problem: string | null,
): string | null {
  if (problem !== null) {
    return `${problem} Your other settings were saved, and this shortcut is still the one in your settings: pick another combination here, or quit whatever is holding this one and save again.`
  }
  if (bound === null) {
    return 'No capture shortcut is bound right now. Save to put these into force, or use the menu bar item to take a capture.'
  }
  if (SHORTCUT_ROWS.every(({ key }) => bound[key] === shown[key])) return null
  const inForce = SHORTCUT_ROWS.map(({ key }) => formatBinding(bound[key])).join(', ')
  return `These are not what is bound right now: ${inForce} are. Save to put them into force.`
}

export function SettingsWindow(): JSX.Element {
  const [settings, setSettings] = useState<Settings | null>(null)
  // What the platform has registered, which is a different question from what
  // the form holds; `null` is a real answer and means nothing is bound.
  const [boundShortcuts, setBoundShortcuts] = useState<Shortcuts | null>(null)
  // Why the last save did not change the shortcuts, which is a thing only Rust
  // can say and only a save can produce.
  const [shortcutProblem, setShortcutProblem] = useState<string | null>(null)
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

  const applyView = useCallback((view: SettingsView) => {
    setSettings(view.settings)
    setBoundShortcuts(view.boundShortcuts)
    setShortcutProblem(view.shortcutProblem)
  }, [])

  useEffect(() => {
    let abandoned = false
    invoke<SettingsView>('get_settings')
      .then((loaded) => {
        if (!abandoned) {
          setSettings(loaded.settings)
          setBoundShortcuts(loaded.boundShortcuts)
        }
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
      // The reason belonged to the combination that was just replaced. Leaving
      // it up would put an explanation of one shortcut over another one.
      setShortcutProblem(null)
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
      // rendering: the settings that were written, and the bindings the
      // platform actually accepted.
      const inForce = await invoke<SettingsView>('save_settings', { settings })
      applyView(inForce)
      setNotice({
        kind: 'ok',
        // A save whose shortcuts were refused is still a save, and saying only
        // "Saved" over a warning the user has to scroll up to read would be the
        // half of the truth that costs them nothing to miss.
        text:
          inForce.shortcutProblem === null
            ? 'Saved. The new settings are in force now.'
            : 'Saved, apart from the shortcuts. The reason is above.',
      })
    } catch (error: unknown) {
      setNotice({ kind: 'error', text: String(error) })
      // Nothing was changed, so the form has to go back to showing what is
      // actually in force rather than the request that was refused.
      try {
        applyView(await invoke<SettingsView>('get_settings'))
      } catch {
        // Leave the form as it is: the error above is the one that matters,
        // and replacing it with a second one would only bury it.
      }
    } finally {
      setSaving(false)
    }
  }, [applyView, settings])

  const unbound = settings ? shortcutNotice(settings.shortcuts, boundShortcuts, shortcutProblem) : null

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
        {unbound && (
          <p style={warningStyle} role="status">
            {unbound}
          </p>
        )}
        {SHORTCUT_ROWS.map(({ key, label }) => (
          <Row key={key} label={label}>
            <button
              type="button"
              style={recording === key ? recordingButtonStyle : shortcutButtonStyle}
              // The row's label is a `span`, not a `label`, for the reason
              // `Row` gives, so the button has to carry its own name: without
              // this a screen reader reads "⌘⇧7 button" and never says which
              // capture mode it belongs to.
              aria-label={`${label} shortcut`}
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
              {/*
                Named for a screen reader, which reads the button and not the
                row: "Choose… button" and "Reset button" say nothing about what
                is being chosen or reset.
              */}
              <button
                type="button"
                style={buttonStyle}
                aria-label="Choose the folder captures are saved in"
                onClick={chooseFolder}
              >
                Choose…
              </button>
              {settings.saveDirectory !== null && (
                <button
                  type="button"
                  style={buttonStyle}
                  aria-label="Save captures in your Pictures folder again"
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

      {/*
        A section of its own, and the hint is the reason. This is the only
        switch in the window that decides whether Snapdeck talks to the network
        at all, so it does not belong among preferences about where files go.
        It is off until it is turned on: Check for Updates… in the menu bar is
        always there, so nobody has to leave a connection switched on to get an
        update.
      */}
      <Section
        title="Updates"
        hint="Snapdeck makes no other network connection. You can always check by hand from the menu bar."
      >
        <Check
          label="Check for updates when Snapdeck starts"
          checked={settings.checkForUpdatesAtLaunch}
          onChange={(checkForUpdatesAtLaunch) => update({ checkForUpdatesAtLaunch })}
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

/**
 * The window's background, declared once in `settings.html` and read here.
 *
 * It has to exist in the document as well as in this component: the window is
 * built visible, so the first frame is painted before React has mounted, and
 * without a background on `body` the user sees a white rectangle flash where a
 * grey sheet is about to be. Writing the colour down twice is how the two
 * halves drift apart, so the stylesheet owns the value and this reads it.
 */
const BACKGROUND = 'var(--settings-background)'

const pageStyle: CSSProperties = {
  font: FONT,
  color: '#1d1d1f',
  background: BACKGROUND,
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

// Not the error red the footer uses: nothing has gone wrong with what the user
// just did, and this sits above the controls whether or not they have touched
// anything.
const warningStyle: CSSProperties = {
  color: '#8a5a00',
  background: '#fff6e5',
  border: '1px solid #f0dcb4',
  borderRadius: 6,
  padding: '6px 8px',
  margin: 0,
  lineHeight: 1.4,
}

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
  background: BACKGROUND,
  padding: '12px 0 16px',
  display: 'flex',
  alignItems: 'center',
  gap: 12,
  marginTop: 'auto',
}

const okStyle: CSSProperties = { margin: 0, flex: '1 1 auto', color: '#6e6e73', lineHeight: 1.4 }
const errorStyle: CSSProperties = { ...okStyle, color: '#c1121f' }
