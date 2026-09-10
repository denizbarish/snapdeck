import { describe, expect, it } from 'vitest'
import { bindingFromKeyPress, formatBinding, type KeyPress } from './shortcut'

function press(code: string, held: Partial<KeyPress> = {}): KeyPress {
  return {
    code,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...held,
  }
}

describe('bindingFromKeyPress', () => {
  it('records the combination in the spelling Rust registers', () => {
    expect(bindingFromKeyPress(press('Digit7', { metaKey: true, shiftKey: true }))).toBe(
      'CmdOrCtrl+Shift+Digit7',
    )
  })

  it('writes every modifier that is held, in a fixed order', () => {
    // Pressed in whatever order the user reached for them; stored in one, so
    // the same combination is always the same string and a rebind to the key
    // it already has is recognised as no change.
    expect(
      bindingFromKeyPress(
        press('KeyR', { shiftKey: true, altKey: true, ctrlKey: true, metaKey: true }),
      ),
    ).toBe('CmdOrCtrl+Control+Alt+Shift+KeyR')
  })

  /**
   * The user reaching for Shift on their way to a combination. Taking that as
   * the binding is what a recorder that does not know its modifiers does, and
   * it happens on every single recording.
   */
  it('ignores a modifier pressed on its own', () => {
    for (const code of ['ShiftLeft', 'MetaRight', 'AltLeft', 'ControlLeft', 'CapsLock']) {
      expect(bindingFromKeyPress(press(code, { shiftKey: true, metaKey: true }))).toBeNull()
    }
  })

  /**
   * A global shortcut is not scoped to a window. A bare letter would take that
   * letter away from every application on the machine, including the one the
   * user would need to type it back in.
   */
  it('refuses a key with no modifier at all', () => {
    expect(bindingFromKeyPress(press('KeyS'))).toBeNull()
    expect(bindingFromKeyPress(press('F5'))).toBeNull()
  })

  /**
   * The reason `code` is recorded rather than `key`. The physical key next to 6
   * is `Digit7` on every layout; what it produces with Shift held is not.
   */
  it('records the physical key, so the layout cannot change the binding', () => {
    expect(bindingFromKeyPress(press('Digit7', { metaKey: true, shiftKey: true }))).toBe(
      bindingFromKeyPress(press('Digit7', { metaKey: true, shiftKey: true })),
    )
    expect(bindingFromKeyPress(press('Digit7', { metaKey: true }))).toBe('CmdOrCtrl+Digit7')
  })
})

describe('formatBinding', () => {
  it('draws a recorded binding the way the Mac does', () => {
    expect(formatBinding('CmdOrCtrl+Shift+Digit7')).toBe('⌘⇧7')
    expect(formatBinding('CmdOrCtrl+Control+Alt+Shift+KeyR')).toBe('⌘⌃⌥⇧R')
  })

  /**
   * A settings file may hold a spelling this window never recorded, from a hand
   * edit or an older build. Rust registers those, so the window has to draw
   * them rather than fall back to raw text beside bindings drawn as symbols.
   */
  it('draws the other spellings Rust accepts', () => {
    expect(formatBinding('Cmd+Shift+7')).toBe('⌘⇧7')
    expect(formatBinding('Command+Option+KeyA')).toBe('⌘⌥A')
    expect(formatBinding('super+alt+F5')).toBe('⌘⌥F5')
  })

  it('shows an unrecognised key as it is stored', () => {
    expect(formatBinding('CmdOrCtrl+ArrowUp')).toBe('⌘ArrowUp')
    expect(formatBinding('')).toBe('')
  })
})
