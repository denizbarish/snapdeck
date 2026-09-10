/**
 * Turning a real key press into a binding Rust can register, and back into
 * something a person can read.
 *
 * There is no text field anywhere in the settings window, and that is the whole
 * point of this file. A typed binding is a string the user has to already know
 * the grammar of, it can name a key their keyboard does not have, and it cannot
 * tell `Cmd+Shift+7` from what their layout actually produces when they press
 * those keys. Recording the press answers all three at once.
 *
 * `KeyboardEvent.code` is what is recorded, not `key`. `code` names the
 * physical key and is the same on every layout, which is exactly what a global
 * shortcut is registered against; `key` is the character the layout produces,
 * so on a Turkish keyboard the top-row key next to 6 does not report "7" at all
 * once Shift is held. It is also the spelling `global-hotkey`'s own parser
 * accepts: "Digit7", "KeyA", "F5" are its key names.
 */

/**
 * The keys that are only ever part of a combination.
 *
 * A press of Shift on its own arrives here as an event like any other, and
 * without this the recorder would take "Shift" as the whole binding the instant
 * the user reached for the modifier they meant to hold.
 */
const MODIFIER_CODES = new Set([
  'MetaLeft',
  'MetaRight',
  'ShiftLeft',
  'ShiftRight',
  'AltLeft',
  'AltRight',
  'ControlLeft',
  'ControlRight',
  'CapsLock',
])

/**
 * The modifier names Rust's parser takes, in the order they are written.
 *
 * `CmdOrCtrl` rather than `Cmd`, because it is the spelling the rest of the
 * application already stores and the one that means the same thing on the
 * platform Snapdeck would be ported to next.
 */
const MODIFIER_TOKENS = ['CmdOrCtrl', 'Control', 'Alt', 'Shift'] as const

/** The subset of a keyboard event this needs, so a test does not need a DOM. */
export type KeyPress = {
  code: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
}

/**
 * The binding a key press stands for, or `null` when the press is not one.
 *
 * Two presses are refused rather than recorded. A modifier on its own is the
 * user on their way to a combination. A key with no modifier at all is a
 * binding that would fire everywhere: a global shortcut is not scoped to a
 * window, so a bare `S` would take the letter away from every application on
 * the machine, and there would be no way left to type it back.
 */
export function bindingFromKeyPress(press: KeyPress): string | null {
  if (MODIFIER_CODES.has(press.code)) return null
  const held = [press.metaKey, press.ctrlKey, press.altKey, press.shiftKey]
  const modifiers = MODIFIER_TOKENS.filter((_, index) => held[index])
  if (modifiers.length === 0) return null
  return [...modifiers, press.code].join('+')
}

/** How each modifier is drawn, keyed by the lower-cased token Rust accepts. */
const MODIFIER_SYMBOLS: Record<string, string> = {
  cmdorctrl: '⌘',
  cmdorcontrol: '⌘',
  commandorctrl: '⌘',
  commandorcontrol: '⌘',
  cmd: '⌘',
  command: '⌘',
  super: '⌘',
  ctrl: '⌃',
  control: '⌃',
  alt: '⌥',
  option: '⌥',
  shift: '⇧',
}

/**
 * A binding as the Mac draws it: `⌘⇧7`.
 *
 * Written for what is stored rather than for what this file records, because
 * the two are not the same set. A settings file may hold `Cmd+Shift+7` from a
 * hand edit or an older build, and Rust registers it happily; a formatter that
 * only understood its own output would render that as raw text next to bindings
 * drawn as symbols.
 *
 * An unrecognised final key is shown as it is. It cannot come from the recorder,
 * and printing the stored text is more use to somebody debugging their own file
 * than a question mark would be.
 */
export function formatBinding(binding: string): string {
  const tokens = binding
    .split('+')
    .map((token) => token.trim())
    .filter((token) => token.length > 0)
  const key = tokens.pop()
  if (key === undefined) return ''
  const modifiers = tokens.map((token) => MODIFIER_SYMBOLS[token.toLowerCase()] ?? token)
  return `${modifiers.join('')}${formatKey(key)}`
}

/**
 * The last token, as a key cap.
 *
 * `Digit7` and `KeyA` are the parser's names for keys whose caps read `7` and
 * `A`, and showing the parser's name to the user would be showing them our
 * plumbing.
 */
function formatKey(key: string): string {
  const digit = /^Digit(\d)$/.exec(key)?.[1]
  if (digit) return digit
  const letter = /^Key([A-Za-z])$/.exec(key)?.[1]
  if (letter) return letter.toUpperCase()
  return key
}
