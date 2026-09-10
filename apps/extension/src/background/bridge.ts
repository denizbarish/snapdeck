import {
  BRIDGE_PORT,
  encode,
  fullPageSchema,
  MAX_HANDSHAKE_MESSAGE_BYTES,
  NONCE_HEX_LENGTH,
  parseServerMessage,
  PROTOCOL_VERSION,
  ProtocolError,
  type ErrorCode,
  type FullPage,
  type ServerMessage,
} from '@snapdeck/protocol'

import { version as EXTENSION_VERSION } from '../../package.json'

/**
 * The client half of the bridge: one session, one page, then hang up.
 *
 * The socket is an argument rather than something this module builds, for the
 * reason the capture loop takes `deps`: what is worth testing here is an order
 * of frames and a mapping from what came back to what the user is told, and a
 * real server that answers as fast as it can is exactly what would hide the
 * order. `connectToBridge` is the one implementation that touches `WebSocket`.
 *
 * The trust boundary runs both ways. Anything on this machine can bind a
 * loopback port, so every frame that arrives goes through the protocol package's
 * schema before it is believed, and a frame that does not parse ends the
 * session rather than being skipped.
 *
 * Parsing is not enough on its own, because a `ready` carries no secret and
 * anything can write one. So the handshake runs in both directions: the `hello`
 * carries a challenge, the `ready` has to answer it with a proof only something
 * holding the pairing token can compute, and no page leaves this module until
 * that proof has been recomputed here and found to match.
 */

export type BridgeSocket = {
  send(text: string): void
  close(): void
  onMessage(handler: (text: string) => void): void
  onClose(handler: () => void): void
}

/**
 * `impostor` is deliberately not `unreachable`. Something did answer, and it
 * failed to prove it was Snapdeck: telling the user their app is not running
 * would send them to open a window while the program that wants their pages
 * keeps the port.
 */
export type SendOutcome =
  | { ok: true; savedPath: string | null }
  | { ok: false; code: ErrorCode | 'unreachable' | 'impostor'; message: string }

type Failure = Extract<SendOutcome, { ok: false }>

/**
 * Loopback by address rather than by name: `localhost` can resolve to `::1`
 * first, and the bridge binds the IPv4 address the protocol package names.
 */
const BRIDGE_HOST = '127.0.0.1'

/**
 * Who is calling, as the far end's logs and its settings window will show it.
 * The version comes from the package rather than a string here, so the two
 * cannot drift.
 */
const CLIENT = { name: 'snapdeck-extension', version: EXTENSION_VERSION }

/**
 * What each refusal means for the person who pressed the button. The far end
 * sends a code and a sentence written for a developer; these are the sentences
 * that say what to do about it, and the difference between them matters: an app
 * that is not running is opened, a token that was refused is pasted again, and
 * a version mismatch is fixed by neither.
 */
const EXPLANATIONS: Record<ErrorCode, string> = {
  unsupportedProtocolVersion:
    'Snapdeck and this extension were built for different bridge protocol versions. Update whichever one is older; pairing again will not help.',
  unauthorized:
    'Snapdeck did not accept the pairing token. Copy it again in Snapdeck settings and paste it into this extension’s options.',
  malformedMessage: 'Snapdeck could not read what the extension sent it.',
  saveFailed: 'Snapdeck could not save the capture.',
}

/** Said whenever nothing that speaks the protocol is on the other end. */
const NOT_RUNNING = 'Snapdeck is not running, or it is not listening for the extension.'

/** Said when something answered on the port but was not Snapdeck. */
const UNREADABLE_ANSWER =
  'Something answered on Snapdeck’s bridge port without speaking its protocol, so the connection was closed.'

/**
 * Said when something holds the port and cannot prove it is Snapdeck.
 *
 * Its own sentence rather than `NOT_RUNNING`, because the two ask opposite
 * things of the user. This one names the port, since that is the thing they
 * have to act on, and it says where the capture went so that a refusal never
 * reads like a capture that was lost.
 */
const IMPOSTOR = `Something is listening on Snapdeck’s bridge port (${BRIDGE_PORT}) but could not prove it is Snapdeck, so the capture was not sent to it and went to your downloads instead. Quit whatever else is using that port, then try again.`

/**
 * Said when a frame this side built would be refused by the far end's own
 * schema. It never leaves the machine, so nothing is at risk; what the user
 * needs to know is that the capture is in their downloads folder.
 */
const UNSENDABLE =
  'This capture is outside the limits Snapdeck’s bridge accepts, so it went to your downloads instead.'

function unreachable(detail: string): Failure {
  return { ok: false, code: 'unreachable', message: `${NOT_RUNNING} (${detail})` }
}

function refused(code: ErrorCode, detail: string): Failure {
  const explanation = EXPLANATIONS[code]
  return { ok: false, code, message: detail.length > 0 ? `${explanation} (${detail})` : explanation }
}

function unreadable(detail: string): Failure {
  return { ok: false, code: 'malformedMessage', message: `${UNREADABLE_ANSWER} (${detail})` }
}

function impostor(detail: string): Failure {
  return { ok: false, code: 'impostor', message: `${IMPOSTOR} (${detail})` }
}

function unsendable(detail: string): Failure {
  return { ok: false, code: 'malformedMessage', message: `${UNSENDABLE} (${detail})` }
}

function reasonOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause)
}

/** Measured the way the socket counts, which is not the way `length` does. */
function byteLengthOf(text: string): number {
  return new TextEncoder().encode(text).length
}

/** The base a nonce, a proof and the pairing token are all written in. */
const HEX_RADIX = 16

/** Every byte gets both its digits, so that a small one keeps its leading zero. */
const HEX_DIGITS_PER_BYTE = 2

/**
 * Bytes as lower-case hex, which is how the token is written and therefore how
 * both halves of the handshake are written.
 */
function toHex(bytes: Uint8Array): string {
  let hex = ''
  for (const byte of bytes) {
    hex += byte.toString(HEX_RADIX).padStart(HEX_DIGITS_PER_BYTE, '0')
  }
  return hex
}

/**
 * The challenge this side puts in its `hello`: sixteen bytes of the browser's
 * own randomness, as hex.
 *
 * Fresh for every session and from `crypto.getRandomValues` rather than
 * anything cheaper, because a nonce that repeats is a proof that can be
 * replayed: a program holding the port watches one answer go past, keeps it,
 * and passes the next challenge with it without ever knowing the token.
 *
 * The byte count comes from `NONCE_HEX_LENGTH` rather than being written again
 * here, so the two cannot say different things about how long a nonce is.
 */
function freshNonce(): string {
  const bytes = new Uint8Array(NONCE_HEX_LENGTH / HEX_DIGITS_PER_BYTE)
  crypto.getRandomValues(bytes)
  return toHex(bytes)
}

/**
 * The bridge's half of the handshake, as this side computes it:
 * `HMAC-SHA256(key = the token, message = the nonce)`, lowercase hex.
 *
 * The key is the token *as a string*. Both the token and the nonce are hex, so
 * a key could mean the characters or the 32 bytes they spell, and the two
 * answer different proofs; the desktop's `bridge::session::proof_for` makes the
 * same reading, and `bridge.test.ts` pins both against a vector neither program
 * computed.
 *
 * Exported because it is the whole of the rule and worth a test of its own, the
 * way `proof_for` is on the other side.
 */
export async function proofFor(token: string, nonce: string): Promise<string> {
  const utf8 = new TextEncoder()
  const key = await crypto.subtle.importKey(
    'raw',
    utf8.encode(token),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign'],
  )
  return toHex(new Uint8Array(await crypto.subtle.sign('HMAC', key, utf8.encode(nonce))))
}

/**
 * Whether two proofs are the same, in time that does not depend on where they
 * first differ.
 *
 * A comparison that stops at the first wrong character tells whoever holds the
 * port how much of a proof it had right, and a proof that answers that question
 * is one that can be guessed a character at a time. The length is not a secret,
 * the schema fixes it at `PROOF_HEX_LENGTH`, so a difference there returns at
 * once; past that every character is read whatever the ones before it were.
 */
function proofsMatch(offered: string, expected: string): boolean {
  if (offered.length !== expected.length) return false

  let difference = 0
  for (let index = 0; index < offered.length; index += 1) {
    difference |= offered.charCodeAt(index) ^ expected.charCodeAt(index)
  }
  return difference === 0
}

/** One thing that arrived, or one reason nothing did. */
type Incoming =
  | { state: 'frame'; message: ServerMessage }
  | { state: 'unreadable'; reason: string }
  | { state: 'closed' }
  | { state: 'timeout' }

/**
 * Turns the socket's two handlers into something that can be awaited one frame
 * at a time. Frames are queued rather than dropped: the far end is free to
 * answer before this side gets round to asking.
 */
function listen(socket: BridgeSocket): (timeoutMs: number) => Promise<Incoming> {
  const queue: Incoming[] = []
  let closed = false
  let wake: (() => void) | null = null

  socket.onMessage((text) => {
    try {
      queue.push({ state: 'frame', message: parseServerMessage(text) })
    } catch (cause) {
      queue.push({ state: 'unreadable', reason: reasonOf(cause) })
    }
    wake?.()
  })
  socket.onClose(() => {
    // A flag rather than a queued entry, because every wait after a hang-up has
    // the same answer and there is only ever one hang-up to queue.
    closed = true
    wake?.()
  })

  return (timeoutMs: number) =>
    new Promise<Incoming>((resolve) => {
      const settle = (incoming: Incoming): void => {
        clearTimeout(timer)
        wake = null
        resolve(incoming)
      }
      const take = (): void => {
        const next = queue.shift()
        if (next !== undefined) {
          settle(next)
          return
        }
        if (closed) settle({ state: 'closed' })
      }

      const timer = setTimeout(() => {
        settle({ state: 'timeout' })
      }, timeoutMs)
      wake = take
      // Whatever arrived while the caller was busy is already in the queue.
      take()
    })
}

type Session = {
  send(text: string): void
  receive(): Promise<Incoming>
}

/** A frame of the type that was asked for, or the reason there is none. */
type Received<T extends ServerMessage['type']> =
  | { frame: Extract<ServerMessage, { type: T }>; failure?: undefined }
  | { frame?: undefined; failure: Failure }

async function receive<T extends ServerMessage['type']>(
  session: Session,
  type: T,
  /**
   * What a frame that will not parse, or that is not the one asked for, means
   * at this point in the session. Waiting for `ready` it means the far end has
   * not proved it is Snapdeck and never will, which is `impostor`; after that
   * the session is one this side already trusts, and a frame it cannot read is
   * `unreadable`.
   */
  onUnreadable: (detail: string) => Failure,
): Promise<Received<T>> {
  const incoming = await session.receive()
  switch (incoming.state) {
    case 'timeout':
      return { failure: unreachable(`no ${type} within the time allowed`) }
    case 'closed':
      return { failure: unreachable(`the connection was closed before the ${type}`) }
    case 'unreadable':
      return { failure: onUnreadable(incoming.reason) }
    case 'frame': {
      const { message } = incoming
      if (message.type === 'error') return { failure: refused(message.code, message.message) }
      if (message.type !== type) {
        return { failure: onUnreadable(`expected a ${type}, got a ${message.type}`) }
      }
      // The comparison above is the narrowing; a generic `T` is not something
      // the compiler can carry through it.
      return { frame: message as Extract<ServerMessage, { type: T }> }
    }
  }
}

/**
 * Opens, shakes hands, runs `handOver`, and closes on every path out.
 *
 * The close is in a `finally` rather than at each ending because there are six
 * of them, and the one that gets forgotten is a socket held open by a service
 * worker Chrome then refuses to shut down.
 */
async function runSession(
  open: () => Promise<BridgeSocket>,
  token: string,
  timeoutMs: number,
  handOver: (session: Session) => Promise<SendOutcome>,
): Promise<SendOutcome> {
  let socket: BridgeSocket
  try {
    socket = await open()
  } catch (cause) {
    return unreachable(reasonOf(cause))
  }

  const next = listen(socket)
  const session: Session = {
    send: (text: string) => {
      socket.send(text)
    },
    receive: () => next(timeoutMs),
  }

  try {
    const nonce = freshNonce()
    const hello = encode({
      type: 'hello',
      protocolVersion: PROTOCOL_VERSION,
      token,
      nonce,
      client: CLIENT,
    })

    // Until a token has been checked the bridge reads at most this much per
    // frame, because a caller that has proved nothing must not be able to make
    // it hold a buffer of that caller's choosing. Every field of a `hello` is
    // short by schema, so going over means the stored token is not one Snapdeck
    // ever issued, and the frame is dropped here rather than closed there.
    const weight = byteLengthOf(hello)
    if (weight > MAX_HANDSHAKE_MESSAGE_BYTES) {
      return refused(
        'unauthorized',
        `the handshake would weigh ${weight} bytes, over the ${MAX_HANDSHAKE_MESSAGE_BYTES} the bridge reads before a token is checked`,
      )
    }
    session.send(hello)

    const ready = await receive(session, 'ready', impostor)
    if (ready.failure) return ready.failure

    // The half of the handshake that runs the other way. A `ready` carries no
    // secret, so anything that took the port can write one; what it cannot
    // write is this, which needs the token it has only just been handed a copy
    // of. Nothing is sent to a far end that fails here.
    if (!proofsMatch(ready.frame.proof, await proofFor(token, nonce))) {
      return impostor('the proof it answered is not the one this token and this challenge make')
    }

    return await handOver(session)
  } catch (cause) {
    // `encode` refuses a frame over the size limit and a socket that died
    // between the check and the write throws. Neither is worth a different
    // answer from the caller: whatever happened, the page did not arrive.
    if (cause instanceof ProtocolError) return refused(cause.code, cause.message)
    return unreachable(reasonOf(cause))
  } finally {
    socket.close()
  }
}

/**
 * Opens a session, hands over one page, and closes.
 *
 * Never throws: the caller's answer to every failure is the same, fall back to
 * a download, and a thrown error would make that a `catch` in three places.
 */
export function sendFullPage(
  open: () => Promise<BridgeSocket>,
  token: string,
  page: Omit<FullPage, 'type'>,
  timeoutMs: number,
): Promise<SendOutcome> {
  return runSession(open, token, timeoutMs, async (session) => {
    const frame: FullPage = { type: 'fullPage', ...page }

    // The far end's own schema, run against the frame before it goes rather
    // than after 55 MB of it has crossed the socket and been closed with 1008.
    //
    // Refused rather than trimmed to fit. A URL is metadata and is clipped by
    // the caller, but the numbers of a picture are not: a width cut down to the
    // limit does not describe a smaller picture, it describes the same one
    // wrongly, and an app that believed it would place every pixel in the wrong
    // row. So an oversized capture is not sent at all, and the caller's fallback
    // puts it in the downloads folder whole.
    const checked = fullPageSchema.safeParse(frame)
    if (!checked.success) {
      return unsendable(checked.error.issues.map((issue) => issue.message).join('; '))
    }

    session.send(encode(frame))
    const accepted = await receive(session, 'accepted', unreadable)
    if (accepted.failure) return accepted.failure
    // `null` is what the app answers when the user's settings say clipboard
    // only. Nothing was saved, and nothing went wrong.
    return { ok: true, savedPath: accepted.frame.savedPath }
  })
}

/**
 * The handshake on its own, for the options page's `Test connection` button.
 *
 * Shares `runSession` rather than repeating it: the button exists to answer
 * "is this token the right one", and a second copy of the handshake is a second
 * place for that answer to be wrong. It stops at `ready`, because a button
 * pressed to check a token should not upload a page.
 */
export function testBridgeConnection(
  open: () => Promise<BridgeSocket>,
  token: string,
  timeoutMs: number,
): Promise<SendOutcome> {
  return runSession(open, token, timeoutMs, () => Promise.resolve({ ok: true, savedPath: null }))
}

/**
 * The one implementation that touches `WebSocket`.
 *
 * `ws://` rather than `wss://` and loopback rather than a name: the bridge is a
 * plain socket on this machine, and a loopback address is a trustworthy origin,
 * so the browser does not treat this as mixed content.
 */
export function connectToBridge(port: number = BRIDGE_PORT): Promise<BridgeSocket> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://${BRIDGE_HOST}:${port}`)

    socket.addEventListener(
      'open',
      () => {
        resolve(adopt(socket))
      },
      { once: true },
    )
    // A refused connection surfaces as an error event with nothing in it; the
    // reason a browser will not say is always the same one.
    socket.addEventListener(
      'error',
      () => {
        reject(new Error(`nothing accepted a connection on ${BRIDGE_HOST}:${port}`))
      },
      { once: true },
    )
  })
}

function adopt(socket: WebSocket): BridgeSocket {
  return {
    send: (text: string) => {
      socket.send(text)
    },
    close: () => {
      socket.close()
    },
    onMessage: (handler: (text: string) => void) => {
      socket.addEventListener('message', (event: MessageEvent<unknown>) => {
        // The protocol is JSON text. A binary frame is not something this side
        // has any way to read, so it is left to the schema check to reject.
        if (typeof event.data === 'string') handler(event.data)
      })
    },
    onClose: (handler: () => void) => {
      socket.addEventListener('close', () => {
        handler()
      }, { once: true })
    },
  }
}

/** Chunked so a 40 MB capture does not become one argument list. */
const BASE64_CHUNK_BYTES = 0x8000

/**
 * The bytes of a blob as base64.
 *
 * It lives here because base64 exists in this extension for one reason, which
 * is that the bridge's JSON envelope cannot carry bytes. The download fallback
 * borrows it rather than growing a second encoder.
 */
export async function toBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer())
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += BASE64_CHUNK_BYTES) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + BASE64_CHUNK_BYTES))
  }
  return btoa(binary)
}
