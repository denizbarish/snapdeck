import { describe, expect, it } from 'vitest'

import { encode, PROTOCOL_VERSION } from '@snapdeck/protocol'

import {
  sendFullPage,
  testBridgeConnection,
  type BridgeSocket,
  type SendOutcome,
} from './bridge'

/**
 * The `node` project. Everything the bridge client decides is an order of
 * frames and a mapping from what came back to what the caller is told, and both
 * are visible through a socket that only records.
 *
 * The socket is a fake rather than a real `WebSocket` on purpose: what is worth
 * testing here is that the client refuses to hand a page over before the far
 * end has said `ready`, and a real server that answers as fast as it can is the
 * one thing that would hide that.
 */

const TOKEN = 'a'.repeat(64)

/** Long enough that no test in here can time out by being slow. */
const TIMEOUT_MS = 2000

/** For the two tests whose subject is what happens when nothing comes back. */
const SHORT_TIMEOUT_MS = 10

const PAGE = {
  requestId: 'request-1',
  page: { url: 'https://example.test/a', title: 'A page' },
  image: { pngBase64: 'iVBORw0KGgo=', width: 800, height: 2400, devicePixelRatio: 2 },
  truncated: false,
}

const READY = encode({
  type: 'ready',
  protocolVersion: PROTOCOL_VERSION,
  app: { name: 'Snapdeck', version: '0.1.0' },
})

function accepted(savedPath: string | null): string {
  return encode({ type: 'accepted', requestId: PAGE.requestId, savedPath })
}

type Frame = Record<string, unknown>

type FakeSocket = BridgeSocket & {
  /** Every frame the client sent, parsed. */
  readonly sent: Frame[]
  closes(): number
  /** Hands the client a frame, the way a server would. */
  deliver(text: string): void
}

function fakeSocket(): FakeSocket {
  const sent: Frame[] = []
  let closes = 0
  let message: ((text: string) => void) | null = null

  return {
    sent,
    closes: () => closes,
    send(text: string) {
      sent.push(JSON.parse(text) as Frame)
    },
    close() {
      closes += 1
    },
    onMessage(handler: (text: string) => void) {
      message = handler
    },
    onClose() {
      // No test here needs the far end to hang up; G4 covers the socket that
      // never opens and G5 the one that never answers.
    },
    deliver(text: string) {
      message?.(text)
    },
  }
}

function typesOf(socket: FakeSocket): unknown[] {
  return socket.sent.map((frame) => frame.type)
}

/** Lets everything already queued run, without waiting out a timeout. */
function flush(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0)
  })
}

function expectFailure(outcome: SendOutcome): Extract<SendOutcome, { ok: false }> {
  if (outcome.ok) {
    throw new Error(`expected a failure, got a success saved at ${String(outcome.savedPath)}`)
  }
  return outcome
}

describe('sendFullPage', () => {
  it('hands the page over only after the app has said ready', async () => {
    // G1. The handshake is what proves the app on the other end is Snapdeck and
    // that it accepted the token. A page sent before `ready` is a 40 MB frame
    // pushed at something that may not even be listening for it.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, TIMEOUT_MS)

    await flush()
    expect(typesOf(socket)).toEqual(['hello'])
    expect(socket.sent[0]?.token).toBe(TOKEN)

    socket.deliver(READY)
    await flush()
    expect(typesOf(socket)).toEqual(['hello', 'fullPage'])

    socket.deliver(accepted('/Users/someone/Snapdeck/a.png'))
    await expect(outcome).resolves.toEqual({
      ok: true,
      savedPath: '/Users/someone/Snapdeck/a.png',
    })
  })

  it('takes the protocol version from the protocol package', async () => {
    // G2. The version is what the far end's first gate reads. Written out as a
    // number rather than compared against the import alone: a test that only
    // says "whatever the constant says" moves with the constant, and a bumped
    // version would then be reported as agreement by both sides at once.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, SHORT_TIMEOUT_MS)

    await flush()

    expect(socket.sent[0]?.protocolVersion).toBe(1)
    expect(socket.sent[0]?.protocolVersion).toBe(PROTOCOL_VERSION)
    await outcome
  })

  it('reports a version mismatch as itself, not as an app that is not there', async () => {
    // G3. The two failures need different answers from the user: an app that is
    // not running is opened, and a version mismatch is updated. Pasting the
    // token again fixes neither, and is what "unreachable" invites.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, TIMEOUT_MS)

    await flush()
    socket.deliver(
      encode({
        type: 'error',
        requestId: null,
        // Deliberately unhelpful, so the sentence the user reads has to be the
        // client's own rather than whatever the far end happened to say.
        message: 'no',
        code: 'unsupportedProtocolVersion',
      }),
    )

    const failure = expectFailure(await outcome)
    expect(failure.code).toBe('unsupportedProtocolVersion')
    expect(failure.message).toMatch(/version/i)
  })

  it('answers unreachable rather than throwing when the socket will not open', async () => {
    // G4. Every failure here ends the same way, in a download, and a throw
    // would make that a `catch` at each of the three places that call this.
    const outcome = await sendFullPage(
      () => Promise.reject(new Error('connection refused')),
      TOKEN,
      PAGE,
      TIMEOUT_MS,
    )

    const failure = expectFailure(outcome)
    expect(failure.code).toBe('unreachable')
    expect(failure.message).toMatch(/Snapdeck/)
  })

  it('closes the socket and gives up when ready never comes', async () => {
    // G5. Something is listening on the port but it is not answering. Without
    // the close this leaks a socket per capture, held open by a service worker
    // that Chrome will not shut down while it has one.
    const socket = fakeSocket()

    const outcome = await sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, SHORT_TIMEOUT_MS)

    expect(expectFailure(outcome).code).toBe('unreachable')
    expect(socket.closes()).toBeGreaterThan(0)
    expect(typesOf(socket)).toEqual(['hello'])
  })

  it('closes the socket on a frame that does not match the schema', async () => {
    // G6. The trust boundary runs both ways. The desktop does not believe the
    // extension, and the extension does not believe whatever answered on a
    // loopback port that anything on this machine can bind.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, SHORT_TIMEOUT_MS)

    await flush()
    socket.deliver('{"type":"ready"}')

    const failure = expectFailure(await outcome)
    expect(failure.code).toBe('malformedMessage')
    expect(socket.closes()).toBeGreaterThan(0)
    // The page is never handed to something that cannot even say `ready`.
    expect(typesOf(socket)).toEqual(['hello'])
  })

  it('counts a capture that was only copied, never saved, as a success', async () => {
    // G7. `savedPath: null` is what the app answers when the user's settings
    // say clipboard only. Treating it as a failure would download a second copy
    // of a capture that arrived perfectly well.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, TIMEOUT_MS)

    await flush()
    socket.deliver(READY)
    await flush()
    socket.deliver(accepted(null))

    await expect(outcome).resolves.toEqual({ ok: true, savedPath: null })
  })
})

describe('testBridgeConnection', () => {
  it('shakes hands and stops there', async () => {
    // The options page's `Test connection` button. It is pressed while the user
    // is looking at a token they may have pasted wrong, and a button that
    // uploads a page to find that out would be a surprise.
    const socket = fakeSocket()
    const outcome = testBridgeConnection(() => Promise.resolve(socket), TOKEN, TIMEOUT_MS)

    await flush()
    socket.deliver(READY)

    await expect(outcome).resolves.toEqual({ ok: true, savedPath: null })
    expect(typesOf(socket)).toEqual(['hello'])
  })
})
