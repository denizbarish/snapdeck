import { describe, expect, it } from 'vitest'

import {
  BRIDGE_PORT,
  MAX_HANDSHAKE_MESSAGE_BYTES,
  MAX_IMAGE_PIXELS,
  MAX_MESSAGE_BYTES,
  MAX_PNG_BASE64_CHARS,
  MAX_PNG_BYTES,
  NONCE_HEX_LENGTH,
  PROOF_HEX_LENGTH,
} from './limits'
import { encode, helloSchema } from './messages'

describe('limits', () => {
  // L1
  it('carries the base64 of the largest transferable PNG plus the envelope', () => {
    expect(Math.ceil(MAX_PNG_BYTES / 3) * 4).toBeLessThan(MAX_MESSAGE_BYTES)
  })

  // L2
  it('keeps the canvas the extension builds inside what Chrome accepts', () => {
    expect(MAX_IMAGE_PIXELS).toBeLessThan(268_435_456)
  })

  // L3
  it('sits in the IANA dynamic port range', () => {
    expect(BRIDGE_PORT).toBeGreaterThanOrEqual(49_152)
    expect(BRIDGE_PORT).toBeLessThanOrEqual(65_535)
  })

  // L4. The two halves of the mutual proof, in the bytes they stand for.
  it('renders sixteen bytes of nonce and a whole sha256 digest of proof', () => {
    expect(NONCE_HEX_LENGTH).toBe(16 * 2)
    expect(PROOF_HEX_LENGTH).toBe(32 * 2)
  })

  // L5. Security. The handshake budget has to hold the largest `hello` the
  // schema will accept and nothing like a capture, or the two-stage limit is
  // either a refusal of real extensions or no limit at all.
  it('holds the largest hello the schema accepts and far less than a capture', () => {
    const largest = encode({
      type: 'hello',
      protocolVersion: PROTOCOL_VERSION_PLACE,
      token: 't'.repeat(256),
      nonce: 'a'.repeat(32),
      client: { name: 'n'.repeat(64), version: 'v'.repeat(32) },
    })
    expect(helloSchema.safeParse(JSON.parse(largest)).success).toBe(true)

    const bytes = new TextEncoder().encode(largest).length
    expect(bytes).toBeLessThan(MAX_HANDSHAKE_MESSAGE_BYTES)
    // 4096 is the contract: a few KiB, written out rather than imported.
    expect(MAX_HANDSHAKE_MESSAGE_BYTES).toBe(4096)
    expect(MAX_HANDSHAKE_MESSAGE_BYTES).toBeLessThan(MAX_MESSAGE_BYTES)
  })

  // L6. The base64 cap and the envelope cap describe the same PNG, so the
  // first has to fit inside the second with the JSON around it.
  it('keeps the longest base64 payload inside the envelope', () => {
    expect(MAX_PNG_BASE64_CHARS).toBe(Math.ceil(MAX_PNG_BYTES / 3) * 4)
    expect(MAX_PNG_BASE64_CHARS).toBeLessThan(MAX_MESSAGE_BYTES)
  })
})

/**
 * The version the largest-hello fixture announces. Any integer does: L5 is
 * about how much a `hello` may weigh, not about which version it names.
 */
const PROTOCOL_VERSION_PLACE = 1
