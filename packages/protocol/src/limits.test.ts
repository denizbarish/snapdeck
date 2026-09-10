import { describe, expect, it } from 'vitest'

import { BRIDGE_PORT, MAX_IMAGE_PIXELS, MAX_MESSAGE_BYTES, MAX_PNG_BYTES } from './limits'

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
})
