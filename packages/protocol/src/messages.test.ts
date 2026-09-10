import { describe, expect, it } from 'vitest'

import { MAX_MESSAGE_BYTES } from './limits'
import {
  encode,
  fullPageSchema,
  helloSchema,
  parseClientMessage,
  parseServerMessage,
  ProtocolError,
  readySchema,
  type FullPage,
} from './messages'

/** Sixteen bytes of hex, which is what a `hello` has to carry. */
const NONCE = '0f1e2d3c4b5a69788796a5b4c3d2e1f0'

/** Thirty-two bytes of hex, which is what a `ready` has to carry. */
const PROOF = `${NONCE}${NONCE}`

const hello = {
  type: 'hello',
  protocolVersion: 1,
  token: 'a-pairing-token',
  nonce: NONCE,
  client: { name: 'snapdeck-extension', version: '0.1.0' },
}

const ready = {
  type: 'ready',
  protocolVersion: 1,
  app: { name: 'snapdeck', version: '0.1.0' },
  proof: PROOF,
}

function fullPage(overrides: { pngBase64?: string; title?: string } = {}): FullPage {
  return {
    type: 'fullPage',
    requestId: 'req-1',
    page: { url: 'https://example.com/', title: overrides.title ?? 'Example' },
    image: {
      pngBase64: overrides.pngBase64 ?? 'iVBORw0KGgo=',
      width: 1280,
      height: 4200,
      devicePixelRatio: 2,
    },
    truncated: false,
  }
}

describe('parseClientMessage', () => {
  // M1
  it('parses a valid hello and carries the token', () => {
    const message = parseClientMessage(JSON.stringify(hello))

    expect(message.type).toBe('hello')
    expect(message).toMatchObject({ token: 'a-pairing-token' })
  })

  // M2
  it('rejects a hello that carries an unknown field', () => {
    const raw = JSON.stringify({ ...hello, extra: 'surprise' })

    expect(() => parseClientMessage(raw)).toThrow(
      expect.objectContaining({ code: 'malformedMessage' }),
    )
    expect(() => parseClientMessage(raw)).toThrow(ProtocolError)
  })

  // M3
  it('rejects JSON without a type field', () => {
    const raw = JSON.stringify({ protocolVersion: 1, token: 'a-pairing-token' })

    expect(() => parseClientMessage(raw)).toThrow(
      expect.objectContaining({ code: 'malformedMessage' }),
    )
  })

  // M4
  it('turns text that is not JSON into a ProtocolError, never a SyntaxError', () => {
    let thrown: unknown
    try {
      parseClientMessage('not json at all')
    } catch (error) {
      thrown = error
    }

    expect(thrown).toBeInstanceOf(ProtocolError)
    expect(thrown).not.toBeInstanceOf(SyntaxError)
    expect(thrown).toMatchObject({ code: 'malformedMessage' })
  })
})

describe('encode', () => {
  // M5
  it('rejects a message whose multi-byte title pushes it over the byte limit', () => {
    // Under the limit measured in UTF-16 code units, over it measured in UTF-8
    // bytes: every 'ğ' is one character and two bytes.
    const message = fullPage({
      pngBase64: 'a'.repeat(MAX_MESSAGE_BYTES - 100_000),
      title: 'ğ'.repeat(60_000),
    })
    const json = JSON.stringify(message)
    expect(json.length).toBeLessThan(MAX_MESSAGE_BYTES)

    let thrown: unknown
    try {
      encode(message)
    } catch (error) {
      thrown = error
    }

    expect(thrown).toBeInstanceOf(Error)
    const text = String((thrown as Error).message)
    expect(text).toContain(String(new TextEncoder().encode(json).length))
    expect(text).toContain(String(MAX_MESSAGE_BYTES))
  })
})

describe('parseServerMessage', () => {
  // M6
  it('rejects an error message carrying an unknown code', () => {
    const raw = JSON.stringify({
      type: 'error',
      requestId: 'req-1',
      code: 'somethingElseWentWrong',
      message: 'nope',
    })

    expect(() => parseServerMessage(raw)).toThrow(
      expect.objectContaining({ code: 'malformedMessage' }),
    )
  })

  // M8
  it('accepts an accepted message with a null savedPath', () => {
    const raw = JSON.stringify({ type: 'accepted', requestId: 'req-1', savedPath: null })

    const message = parseServerMessage(raw)

    expect(message).toEqual({ type: 'accepted', requestId: 'req-1', savedPath: null })
  })
})

describe('fullPageSchema', () => {
  // M7
  it('rejects a zero or negative width', () => {
    for (const width of [0, -1]) {
      const message = { ...fullPage(), image: { ...fullPage().image, width } }

      expect(fullPageSchema.safeParse(message).success).toBe(false)
    }
  })

  // M11. Security. Two numbers that are each acceptable multiply into an area
  // that is not, and the area is what gets allocated.
  it('rejects a picture whose area is past the pixel limit while each side is not', () => {
    const image = { ...fullPage().image, width: 100_000, height: 100_000 }

    expect(fullPageSchema.safeParse({ ...fullPage(), image }).success).toBe(false)
    expect(
      fullPageSchema.safeParse({ ...fullPage(), image: { ...image, height: 640 } }).success,
    ).toBe(true)
  })

  // M12. Security. The base64 is the whole weight of the message, and a cap
  // that lives only in the desktop's decoder is a cap the extension cannot see.
  it('rejects a png past the base64 length the protocol carries', () => {
    // 55_924_056 characters is the contract: four for every three bytes of the
    // 41_943_040 a PNG may weigh. Written out rather than imported, so a
    // changed constant fails here instead of following it.
    const message = fullPage({ pngBase64: 'A'.repeat(55_924_057) })

    expect(fullPageSchema.safeParse(message).success).toBe(false)
    expect(fullPageSchema.safeParse(fullPage({ pngBase64: 'A'.repeat(55_924_056) })).success).toBe(
      true,
    )
  })

  // M13. A request id comes back in answers and goes into log lines, so it is
  // an alphabet rather than a length: a quote or a newline in it is a way of
  // writing into somewhere it was only ever supposed to be quoted.
  it('rejects a request id outside its alphabet or past its length', () => {
    for (const requestId of ['req 1', 'req"1', 'req\n1', '', 'r'.repeat(65)]) {
      expect(fullPageSchema.safeParse({ ...fullPage(), requestId }).success).toBe(false)
    }
    expect(
      fullPageSchema.safeParse({ ...fullPage(), requestId: crypto.randomUUID() }).success,
    ).toBe(true)
  })

  // M14. A device pixel ratio is a multiplier on the canvas the extension
  // builds, so an unbounded one asks for an unbounded picture.
  it('rejects a device pixel ratio that is not a sane finite number', () => {
    for (const devicePixelRatio of [0, -1, 8.5, Infinity, -Infinity, NaN]) {
      const image = { ...fullPage().image, devicePixelRatio }

      expect(fullPageSchema.safeParse({ ...fullPage(), image }).success).toBe(false)
    }
    // 8 is the contract: the largest ratio a display reports, with room over it.
    expect(
      fullPageSchema.safeParse({ ...fullPage(), image: { ...fullPage().image, devicePixelRatio: 8 } })
        .success,
    ).toBe(true)
  })

  // M15. The two strings that come off a page the user did not write.
  it('rejects a url or a title past its cap', () => {
    // 2048 and 1024 are the contract, written out rather than imported.
    const page = fullPage().page

    expect(
      fullPageSchema.safeParse({ ...fullPage(), page: { ...page, url: `h${'a'.repeat(2048)}` } })
        .success,
    ).toBe(false)
    expect(
      fullPageSchema.safeParse({ ...fullPage(), page: { ...page, title: 'a'.repeat(1025) } })
        .success,
    ).toBe(false)
  })
})

describe('helloSchema', () => {
  // M9. Security. The nonce is what makes the bridge's answer a proof rather
  // than a sentence anybody can copy, so a `hello` without one is not a
  // handshake this protocol knows.
  it('refuses a hello that carries no nonce', () => {
    const { nonce: _nonce, ...withoutNonce } = hello

    expect(helloSchema.safeParse(withoutNonce).success).toBe(false)
    expect(helloSchema.safeParse(hello).success).toBe(true)
  })

  // M10. Security. Fixed at 32 hex characters, both ends. A nonce the caller
  // can shorten is a nonce the caller can exhaust.
  it('refuses a nonce that is short, long or not hex', () => {
    // 32 characters is the contract: 16 bytes rendered as hex.
    for (const nonce of [
      '0f1e2d3c4b5a69788796a5b4c3d2e1f',
      '0f1e2d3c4b5a69788796a5b4c3d2e1f00',
      '0f1e2d3c4b5a69788796a5b4c3d2e1fg',
      '',
    ]) {
      expect(helloSchema.safeParse({ ...hello, nonce }).success).toBe(false)
    }
    expect(
      helloSchema.safeParse({ ...hello, nonce: '0F1E2D3C4B5A69788796A5B4C3D2E1F0' }).success,
    ).toBe(true)
  })
})

describe('readySchema', () => {
  // M16. Security, and the whole of the mutual half of the handshake: an
  // answer with no proof in it is the answer a program that got to the port
  // first would give, and it has to be unable to satisfy the schema at all.
  it('refuses a ready that carries no proof, or one of the wrong shape', () => {
    const { proof: _proof, ...withoutProof } = ready

    expect(readySchema.safeParse(withoutProof).success).toBe(false)
    // 64 characters is the contract: the 32 bytes of a SHA-256 digest as hex.
    for (const proof of [PROOF.slice(1), `${PROOF}0`, `${PROOF.slice(1)}z`]) {
      expect(readySchema.safeParse({ ...ready, proof }).success).toBe(false)
    }
    expect(readySchema.safeParse(ready).success).toBe(true)
  })
})

describe('server strings', () => {
  // M17. LOW-2. A boundary that is bidirectional in name has to be
  // bidirectional in its limits too: what the bridge says is bounded exactly
  // as what it is told is.
  it('refuses a server message whose strings run past their caps', () => {
    // 1024 and 4096 are the contract, written out rather than imported.
    const tooLong = JSON.stringify({
      type: 'error',
      requestId: 'req-1',
      code: 'saveFailed',
      message: 'm'.repeat(1025),
    })
    expect(() => parseServerMessage(tooLong)).toThrow(
      expect.objectContaining({ code: 'malformedMessage' }),
    )

    const longPath = JSON.stringify({
      type: 'accepted',
      requestId: 'req-1',
      savedPath: `/${'p'.repeat(4096)}`,
    })
    expect(() => parseServerMessage(longPath)).toThrow(
      expect.objectContaining({ code: 'malformedMessage' }),
    )
  })
})
