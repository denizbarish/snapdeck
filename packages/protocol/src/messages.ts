import { z } from 'zod'

import {
  MAX_CLIENT_NAME_CHARS,
  MAX_CLIENT_VERSION_CHARS,
  MAX_DEVICE_PIXEL_RATIO,
  MAX_ERROR_MESSAGE_CHARS,
  MAX_IMAGE_PIXELS,
  MAX_MESSAGE_BYTES,
  MAX_PNG_BASE64_CHARS,
  MAX_REQUEST_ID_CHARS,
  MAX_SAVED_PATH_CHARS,
  MAX_TITLE_CHARS,
  MAX_TOKEN_CHARS,
  MAX_URL_CHARS,
  NONCE_HEX_LENGTH,
  PROOF_HEX_LENGTH,
  REQUEST_ID_PATTERN,
} from './limits'

/** Either case, because the proof is taken over the string exactly as sent. */
const HEX_PATTERN = /^[0-9a-fA-F]+$/

export const errorCodeSchema = z.enum([
  'unsupportedProtocolVersion',
  'unauthorized',
  'malformedMessage',
  'saveFailed',
])
export type ErrorCode = z.infer<typeof errorCodeSchema>

/**
 * `strictObject` everywhere, never `object`. This is a trust boundary, and
 * silently dropping an unknown field would hide the fact that the two sides are
 * saying different things. A field is added by raising `PROTOCOL_VERSION`.
 */
export const helloSchema = z.strictObject({
  type: z.literal('hello'),
  protocolVersion: z.number().int(),
  token: z.string().min(1).max(MAX_TOKEN_CHARS),
  /**
   * The extension's challenge to the bridge. Sixteen bytes from
   * `crypto.getRandomValues`, hex, and fresh for every session: a nonce that
   * repeats lets an impostor replay a proof it once watched go past.
   */
  nonce: z.string().length(NONCE_HEX_LENGTH).regex(HEX_PATTERN),
  client: z.strictObject({
    name: z.string().min(1).max(MAX_CLIENT_NAME_CHARS),
    version: z.string().min(1).max(MAX_CLIENT_VERSION_CHARS),
  }),
})

export const fullPageSchema = z.strictObject({
  type: z.literal('fullPage'),
  requestId: z.string().min(1).max(MAX_REQUEST_ID_CHARS).regex(REQUEST_ID_PATTERN),
  page: z.strictObject({
    url: z.string().min(1).max(MAX_URL_CHARS),
    title: z.string().max(MAX_TITLE_CHARS),
  }),
  image: z
    .strictObject({
      pngBase64: z.string().min(1).max(MAX_PNG_BASE64_CHARS),
      width: z.number().int().positive(),
      height: z.number().int().positive(),
      devicePixelRatio: z.number().positive().finite().max(MAX_DEVICE_PIXEL_RATIO),
    })
    // The limit that neither side can express field by field. A picture is
    // refused for the area it claims before anything allocates that area, and
    // two numbers that are each acceptable can multiply into one that is not.
    .refine((image) => image.width * image.height <= MAX_IMAGE_PIXELS, {
      error: `an image may not claim more than ${MAX_IMAGE_PIXELS} pixels`,
    }),
  truncated: z.boolean(),
})

export const clientMessageSchema = z.discriminatedUnion('type', [helloSchema, fullPageSchema])
export type ClientMessage = z.infer<typeof clientMessageSchema>
export type Hello = z.infer<typeof helloSchema>
export type FullPage = z.infer<typeof fullPageSchema>

/**
 * The bridge answering the challenge. `proof` is the half of the handshake that
 * runs the other way: it is what a server that holds the pairing token can
 * produce and a program that merely got to the port first cannot, and the
 * extension sends no page until it has recomputed it and found the same value.
 */
export const readySchema = z.strictObject({
  type: z.literal('ready'),
  protocolVersion: z.number().int(),
  app: z.strictObject({
    name: z.string().max(MAX_CLIENT_NAME_CHARS),
    version: z.string().max(MAX_CLIENT_VERSION_CHARS),
  }),
  proof: z.string().length(PROOF_HEX_LENGTH).regex(HEX_PATTERN),
})
export const acceptedSchema = z.strictObject({
  type: z.literal('accepted'),
  requestId: z.string().max(MAX_REQUEST_ID_CHARS),
  savedPath: z.string().max(MAX_SAVED_PATH_CHARS).nullable(),
})
export const errorMessageSchema = z.strictObject({
  type: z.literal('error'),
  requestId: z.string().max(MAX_REQUEST_ID_CHARS).nullable(),
  code: errorCodeSchema,
  message: z.string().max(MAX_ERROR_MESSAGE_CHARS),
})
export const serverMessageSchema = z.discriminatedUnion('type', [
  readySchema,
  acceptedSchema,
  errorMessageSchema,
])
export type ServerMessage = z.infer<typeof serverMessageSchema>

/**
 * The single failure type crossing the bridge. Neither `SyntaxError` nor
 * `ZodError` escapes this module, so a caller catches one thing and maps its
 * `code` straight onto the wire.
 */
export class ProtocolError extends Error {
  readonly code: ErrorCode

  constructor(code: ErrorCode, message: string) {
    super(message)
    this.name = 'ProtocolError'
    this.code = code
  }
}

function describe(error: z.ZodError): string {
  return error.issues
    .map((issue) => {
      const path = issue.path.join('.')
      return path.length > 0 ? `${path}: ${issue.message}` : issue.message
    })
    .join('; ')
}

function decode<T>(schema: z.ZodType<T>, raw: string, kind: string): T {
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch (cause) {
    const reason = cause instanceof Error ? cause.message : String(cause)
    throw new ProtocolError('malformedMessage', `${kind} is not valid JSON: ${reason}`)
  }

  const result = schema.safeParse(value)
  if (!result.success) {
    throw new ProtocolError('malformedMessage', `${kind} does not match the schema: ${describe(result.error)}`)
  }
  return result.data
}

export function parseClientMessage(raw: string): ClientMessage {
  return decode(clientMessageSchema, raw, 'client message')
}

export function parseServerMessage(raw: string): ServerMessage {
  return decode(serverMessageSchema, raw, 'server message')
}

/**
 * Measured in UTF-8 bytes, which is what the socket counts. `string.length`
 * counts UTF-16 code units, so a multi-byte page title would look safely under
 * the limit here and then be closed with 1009 at the far end.
 */
export function encode(message: ClientMessage | ServerMessage): string {
  const json = JSON.stringify(message)
  const bytes = new TextEncoder().encode(json).length
  if (bytes > MAX_MESSAGE_BYTES) {
    throw new ProtocolError(
      'malformedMessage',
      `message is ${bytes} bytes, over the ${MAX_MESSAGE_BYTES} byte limit`,
    )
  }
  return json
}
