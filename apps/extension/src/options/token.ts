/**
 * The pairing token: what shape it has, and where it is kept.
 *
 * It lives beside the options page because that is where a token is entered,
 * and it is imported by the service worker because that is where one is used.
 * One module knows the storage key, so the page and the worker cannot disagree
 * about which key that is.
 *
 * The shape check is the one thing the options page can decide on its own.
 *
 * Pairing fails at the far end as `unauthorized`, and `unauthorized` reads the
 * same whether the token is wrong by one character or the app was never paired
 * at all. Checking the shape here turns the most common mistakes - a paste that
 * lost a character, one that brought a newline along, a token that went through
 * something which upper-cased it - into a sentence next to the field rather
 * than a refusal from a program that is running on another window.
 *
 * A shape, not a secret: this says the string could be a token, never that it
 * is the right one. Only the app knows that, and only by comparing bytes.
 */

/**
 * `bridge::token` mints 32 bytes of system randomness as lowercase hex, so a
 * pairing token is exactly 64 characters of `0-9a-f` and nothing else. The
 * anchors are what make it 64 rather than "at least 64", and the app compares
 * the bytes it was given, so an upper-case `A` is not an `a`.
 */
const PAIRING_TOKEN = /^[0-9a-f]{64}$/

export function tokenLooksValid(token: string): boolean {
  return PAIRING_TOKEN.test(token)
}

/**
 * Where the token is kept. `local` rather than `sync`: it pairs this browser
 * with the app on this machine, and syncing it would push a secret to every
 * other machine the user is signed in on, where it pairs nothing.
 */
const TOKEN_STORAGE_KEY = 'bridgeToken'

/**
 * The stored token, or an empty string before the extension has been paired.
 *
 * The two readers of this are the options page and the service worker, and this
 * is the one place that knows where it lives.
 */
export async function readPairingToken(): Promise<string> {
  const stored = await chrome.storage.local.get(TOKEN_STORAGE_KEY)
  const token: unknown = stored[TOKEN_STORAGE_KEY]
  return typeof token === 'string' ? token : ''
}

export async function writePairingToken(token: string): Promise<void> {
  await chrome.storage.local.set({ [TOKEN_STORAGE_KEY]: token })
}
