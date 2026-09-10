/**
 * `shortcutNotice` is exported to be tested, and this is that test.
 *
 * It is the sentence that stands between the user and the failure this whole
 * task exists to prevent: an application running with some capture shortcuts
 * working, or none, and no way to tell from the window that claims to show
 * them. The window renders what the settings file says, because that is what
 * the next save writes back; this is what stops it implying that pressing those
 * keys does anything.
 */

import { describe, expect, it } from 'vitest'
import { shortcutNotice, type Shortcuts } from './SettingsWindow'

const BOUND: Shortcuts = {
  captureRegion: 'CmdOrCtrl+Shift+Digit7',
  captureWindow: 'CmdOrCtrl+Shift+Digit8',
  captureDisplay: 'CmdOrCtrl+Shift+Digit9',
}

const STORED: Shortcuts = { ...BOUND, captureRegion: 'CmdOrCtrl+Alt+Shift+KeyR' }

/** The reason a save comes back with when the platform refused the rebind. */
const REFUSED =
  'the region shortcut (⌘⌥⇧R) could not be registered: HotKey already registered. Your previous shortcuts are still in force.'

describe('shortcutNotice', () => {
  // The ordinary case, and the only one that says nothing: what is on the form
  // is what the platform accepted.
  it('says nothing when the shortcuts shown are the ones bound', () => {
    expect(shortcutNotice(BOUND, BOUND, null)).toBeNull()
  })

  // The launch-path case. A stored combination another application had taken
  // stays in the settings, the built-in ones take over the keyboard, and the
  // window may not render the stored ones as though they worked.
  it('names what is bound when the shortcuts shown are not', () => {
    const notice = shortcutNotice(STORED, BOUND, null)
    expect(notice).not.toBeNull()
    // The bindings the user can actually press, as key caps rather than as the
    // parser's spelling.
    expect(notice).toContain('⌘⇧7')
    expect(notice).toContain('⌘⇧8')
    expect(notice).toContain('⌘⇧9')
  })

  // The false "in force" case: nothing registered at all. The window has to say
  // the keyboard is empty and where to go instead, not fall silent because
  // there is no set to compare against.
  it('says the keyboard is empty when nothing is bound', () => {
    const notice = shortcutNotice(STORED, null, null)
    expect(notice).toContain('No capture shortcut is bound')
    expect(notice).toContain('menu bar')
  })

  // A recorded but unsaved combination is not in force either, and the same
  // sentence is the honest thing to say about it.
  it('treats an unsaved change as not in force', () => {
    expect(
      shortcutNotice({ ...BOUND, captureDisplay: 'CmdOrCtrl+Alt+KeyD' }, BOUND, null),
    ).not.toBeNull()
  })

  // The case a partly-completed save leaves behind, and the whole reason the
  // reason travels. The user is owed three things: that the shortcut is not
  // bound, why, and that the rest of what they pressed Save for did happen.
  it('gives the platform reason and says the rest of the save went through', () => {
    const notice = shortcutNotice(STORED, BOUND, REFUSED)
    expect(notice).toContain('already registered')
    expect(notice).toContain('Your other settings were saved')
    expect(notice).toContain('still the one in your settings')
  })

  // The reason takes over rather than being appended to a sentence that says
  // the same thing less precisely: two warnings about one shortcut is how the
  // user stops reading either.
  it('replaces the derived sentence rather than repeating it', () => {
    const notice = shortcutNotice(STORED, null, REFUSED)
    expect(notice).toContain('already registered')
    expect(notice).not.toContain('No capture shortcut is bound right now')
  })
})
