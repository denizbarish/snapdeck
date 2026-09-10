import { z } from 'zod'

import { MAX_MESSAGE_BYTES } from './limits'

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
  token: z.string().min(1).max(256),
  client: z.strictObject({ name: z.string().min(1).max(64), version: z.string().min(1).max(32) }),
})

export const fullPageSchema = z.strictObject({
  type: z.literal('fullPage'),
  requestId: z.string().min(1).max(64),
  page: z.strictObject({ url: z.string().min(1).max(2048), title: z.string().max(1024) }),
  image: z.strictObject({
    pngBase64: z.string().min(1),
    width: z.number().int().positive(),
    height: z.number().int().positive(),
    devicePixelRatio: z.number().positive(),
  }),
  truncated: z.boolean(),
})

export const clientMessageSchema = z.discriminatedUnion('type', [helloSchema, fullPageSchema])
export type ClientMessage = z.infer<typeof clientMessageSchema>
export type Hello = z.infer<typeof helloSchema>
export type FullPage = z.infer<typeof fullPageSchema>

export const readySchema = z.strictObject({
  type: z.literal('ready'),
  protocolVersion: z.number().int(),
  app: z.strictObject({ name: z.string(), version: z.string() }),
})
export const acceptedSchema = z.strictObject({
  type: z.literal('accepted'),
  requestId: z.string(),
  savedPath: z.string().nullable(),
})
export const errorMessageSchema = z.strictObject({
  type: z.literal('error'),
  requestId: z.string().nullable(),
  code: errorCodeSchema,
  message: z.string(),
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
