import { describe, expect, it } from 'vitest'

import { tokenLooksValid } from './token'

/**
 * The `node` project. The check is a shape, and the shape is the whole point:
 * a token that is wrong by one character fails at the far end as
 * `unauthorized`, which reads exactly like an app that refuses to pair.
 */

/** 64 characters, written out rather than derived: this is the contract. */
const TOKEN = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'

describe('tokenLooksValid', () => {
  it('accepts the 64 lowercase hex characters the app mints', () => {
    // O1, the accepting half.
    expect(TOKEN).toHaveLength(64)
    expect(tokenLooksValid(TOKEN)).toBe(true)
  })

  it('rejects a token that is short, upper case or padded', () => {
    // O1, the rejecting half. A copy that dropped its last character, a token
    // put through something that upper-cased it, and the newline a paste from a
    // text field brings with it. The app compares bytes, so none of these is
    // the token even though all three look like it.
    expect(tokenLooksValid(TOKEN.slice(0, 63))).toBe(false)
    expect(tokenLooksValid(TOKEN.toUpperCase())).toBe(false)
    expect(tokenLooksValid(`${TOKEN} `)).toBe(false)
    expect(tokenLooksValid(`${TOKEN.slice(0, 32)} ${TOKEN.slice(33)}`)).toBe(false)
    expect(tokenLooksValid('')).toBe(false)
  })
})
