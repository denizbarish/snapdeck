import { describe, expect, it } from 'vitest'

import { MAX_MESSAGE_BYTES } from './limits'
import {
  encode,
  fullPageSchema,
  parseClientMessage,
  parseServerMessage,
  ProtocolError,
  type FullPage,
} from './messages'

const hello = {
  type: 'hello',
  protocolVersion: 1,
  token: 'a-pairing-token',
  client: { name: 'snapdeck-extension', version: '0.1.0' },
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
})
