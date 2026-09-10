import { createHmac } from 'node:crypto'

import { describe, expect, it, vi } from 'vitest'

import { encode, PROTOCOL_VERSION } from '@snapdeck/protocol'

import {
  proofFor,
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
 * end has said `ready` and proved it holds the pairing token, and a real server
 * that answers as fast as it can is the one thing that would hide that.
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

/**
 * The proof vector, computed outside this program.
 *
 * `HMAC-SHA256` over these two strings is what OpenSSL and Node both answer,
 * and the desktop's `bridge::session` tests pin the same three values. They are
 * written out here rather than computed from the code under test, because an
 * implementation that agrees only with itself is exactly the one the other side
 * of this bridge cannot pair with.
 *
 * The last two are the misreadings. Both the token and the nonce are hex, so a
 * key or a message could mean the characters or the bytes they spell, and each
 * reading answers a different proof. The contract is the characters; the values
 * the other readings produce are here so that a change to that fails loudly
 * rather than drifting.
 */
const VECTOR_TOKEN = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
const VECTOR_NONCE = '0f1e2d3c4b5a69788796a5b4c3d2e1f0'
const VECTOR_PROOF = 'bd0b0ea0ed26cb9208586f1c5b039df3c2c157d46ade2476e82a127a6ea8636b'
const PROOF_IF_THE_KEY_WERE_DECODED =
  '0b7524f58c62339f46c8886db927c2edb6a39b64704ed9924857771e7ca30139'
const PROOF_IF_THE_MESSAGE_WERE_DECODED =
  'c9559e2119354251630bdd2b9b27f79d01da0f5a6c1c954aa08f1a36f016343b'

/** A nonce is 32 hex characters, lower case, and nothing else. */
const NONCE_SHAPE = /^[0-9a-f]{32}$/

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

/**
 * What a server holding the pairing token can answer, computed with Node's own
 * HMAC rather than with the function under test.
 *
 * A fake server that asked `bridge.ts` for the proof would agree with a broken
 * `bridge.ts`, and every claim below rests on it not doing that.
 */
function honestProof(token: string, nonce: string): string {
  return createHmac('sha256', Buffer.from(token, 'utf8'))
    .update(Buffer.from(nonce, 'utf8'))
    .digest('hex')
}

function readyWith(proof: string): string {
  return encode({
    type: 'ready',
    protocolVersion: PROTOCOL_VERSION,
    app: { name: 'Snapdeck', version: '0.1.0' },
    proof,
  })
}

/** The challenge this session put in its `hello`. */
function nonceOf(socket: FakeSocket): string {
  const nonce = socket.sent[0]?.nonce
  if (typeof nonce !== 'string') {
    throw new Error(`the hello carried no nonce: ${JSON.stringify(socket.sent[0])}`)
  }
  return nonce
}

/** The answer the real app gives: the proof over the nonce it was just sent. */
function honestReady(socket: FakeSocket, token: string = TOKEN): string {
  return readyWith(honestProof(token, nonceOf(socket)))
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

describe('proofFor', () => {
  it('takes the token as the key and the nonce as the message, both as strings', async () => {
    // P1. Security, HIGH-1. The one arithmetic in this file, against a vector
    // no part of this repository computed. Two implementations of a protocol
    // that each define the proof their own way pair with nobody, and the two
    // readings below are the ones that would look right in review and produce a
    // handshake that never succeeds.
    await expect(proofFor(VECTOR_TOKEN, VECTOR_NONCE)).resolves.toBe(VECTOR_PROOF)

    await expect(proofFor(VECTOR_TOKEN, VECTOR_NONCE)).resolves.not.toBe(
      PROOF_IF_THE_KEY_WERE_DECODED,
    )
    await expect(proofFor(VECTOR_TOKEN, VECTOR_NONCE)).resolves.not.toBe(
      PROOF_IF_THE_MESSAGE_WERE_DECODED,
    )
  })

  it('answers a different proof for a different nonce', async () => {
    // P2. Security, HIGH-1. A proof that ignored the nonce would be the same
    // string in every session, which is a string an impostor can record once on
    // a port it holds and replay for ever after.
    const first = await proofFor(VECTOR_TOKEN, VECTOR_NONCE)
    const second = await proofFor(VECTOR_TOKEN, 'ffffffffffffffffffffffffffffffff')

    expect(second).not.toBe(first)
    // 64 characters is the contract: the 32 bytes of a SHA-256 digest.
    expect(second).toMatch(/^[0-9a-f]{64}$/)
  })
})

describe('sendFullPage', () => {
  it('hands the page over only after the app has said ready and proved itself', async () => {
    // G1. The handshake is what proves the app on the other end is Snapdeck and
    // that it accepted the token. A page sent before `ready` is a 40 MB frame
    // pushed at something that may not even be listening for it, and a page
    // sent before the proof is checked is one handed to whatever got to the
    // port first.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, TIMEOUT_MS)

    await flush()
    expect(typesOf(socket)).toEqual(['hello'])
    expect(socket.sent[0]?.token).toBe(TOKEN)

    socket.deliver(honestReady(socket))
    // Polled rather than flushed once: recomputing the proof is two turns
    // through WebCrypto, and a fixed number of ticks here would be a number
    // that happens to work on this machine.
    await vi.waitFor(() => {
      expect(typesOf(socket)).toEqual(['hello', 'fullPage'])
    })

    socket.deliver(encode({ type: 'accepted', requestId: PAGE.requestId, savedPath: '/a.png' }))
    await expect(outcome).resolves.toEqual({ ok: true, savedPath: '/a.png' })
  })

  it('challenges the app with a fresh nonce', async () => {
    // G8. Security, HIGH-1. The nonce is what makes the proof about this
    // session rather than about any session. Sixteen bytes as hex is 32
    // characters, and the schema on the far end fixes that length, so a nonce
    // of any other shape is a handshake the app closes.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, SHORT_TIMEOUT_MS)

    await flush()
    expect(nonceOf(socket)).toMatch(NONCE_SHAPE)
    expect(nonceOf(socket)).toHaveLength(32)
    await outcome
  })

  it('never challenges twice with the same nonce', async () => {
    // G9. Security, HIGH-1. A nonce that repeats between sessions is a proof an
    // impostor can replay: it watches one handshake go past on a port it holds,
    // keeps the answer, and passes the next challenge with it.
    const first = fakeSocket()
    const second = fakeSocket()

    await sendFullPage(() => Promise.resolve(first), TOKEN, PAGE, SHORT_TIMEOUT_MS)
    await sendFullPage(() => Promise.resolve(second), TOKEN, PAGE, SHORT_TIMEOUT_MS)

    expect(nonceOf(second)).not.toBe(nonceOf(first))
  })

  it('sends no page to a server that cannot prove it holds the token', async () => {
    // G10. Security, HIGH-1, and the reason the proof exists. Anything on this
    // machine can take port 51837 before Snapdeck does. Without this check that
    // program collects the token, answers a `ready` it made up, and is handed a
    // picture of every page the user captures while they watch each one
    // succeed. What it cannot do is answer the challenge, because that needs
    // the token it has only just been given a copy of - and by then the page
    // has already gone.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, TIMEOUT_MS)

    await flush()
    // A well-formed `ready` from a program that guessed at the proof.
    socket.deliver(readyWith(honestProof('the wrong token', nonceOf(socket))))

    const failure = expectFailure(await outcome)
    expect(failure.code).toBe('impostor')
    expect(typesOf(socket)).not.toContain('fullPage')
    expect(typesOf(socket)).toEqual(['hello'])
    expect(socket.closes()).toBeGreaterThan(0)
  })

  it('refuses a proof that is right except for its last character', async () => {
    // G11. Security, HIGH-1. A comparison that stops early is a proof that can
    // be guessed one character at a time, and the character a prefix comparison
    // never reaches is the last one.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, TIMEOUT_MS)

    await flush()
    const proof = honestProof(TOKEN, nonceOf(socket))
    const nearly = `${proof.slice(0, -1)}${proof.endsWith('0') ? '1' : '0'}`
    socket.deliver(readyWith(nearly))

    expect(expectFailure(await outcome).code).toBe('impostor')
    expect(typesOf(socket)).toEqual(['hello'])
  })

  it('says an impostor is an impostor, not an app that is not running', async () => {
    // G12. Security, HIGH-1. The two need opposite things from the user. "Not
    // running" sends someone to open an app, and they open it while the program
    // that wanted their pages keeps the port and their next capture goes to it.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, TIMEOUT_MS)

    await flush()
    socket.deliver(readyWith('0'.repeat(64)))

    const failure = expectFailure(await outcome)
    expect(failure.code).not.toBe('unreachable')
    expect(failure.message).not.toMatch(/is not running/i)
    // The port is the thing the user has to act on, so it is named.
    expect(failure.message).toContain('51837')
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

  it('treats a ready that misses the schema as an impostor too', async () => {
    // G6. The trust boundary runs both ways. The desktop does not believe the
    // extension, and the extension does not believe whatever answered on a
    // loopback port that anything on this machine can bind. A `ready` with no
    // proof in it is the frame the impostor sent before this contract existed,
    // so it ends the same way a wrong proof does rather than in a sentence
    // about a protocol.
    const socket = fakeSocket()
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, PAGE, SHORT_TIMEOUT_MS)

    await flush()
    socket.deliver(
      JSON.stringify({
        type: 'ready',
        protocolVersion: PROTOCOL_VERSION,
        app: { name: 'Snapdeck', version: '0.1.0' },
      }),
    )

    const failure = expectFailure(await outcome)
    expect(failure.code).toBe('impostor')
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
    socket.deliver(honestReady(socket))
    await flush()
    socket.deliver(encode({ type: 'accepted', requestId: PAGE.requestId, savedPath: null }))

    await expect(outcome).resolves.toEqual({ ok: true, savedPath: null })
  })

  it('never sends a picture that claims more pixels than the contract allows', async () => {
    // G13. Security, MEDIUM-3. The limits are the far end's, and a frame that
    // breaks one is closed with 1008 after 55 MB has crossed the socket. The
    // numbers of a picture cannot be clipped the way a URL can - a smaller
    // width does not make a smaller picture, it makes a wrong one - so the
    // capture is not sent at all and the caller falls back to the download,
    // where the user gets it whole.
    const socket = fakeSocket()
    // 8000 x 8001 device pixels is 64,008,000, past the 64,000,000 a message
    // may claim.
    const huge = { ...PAGE, image: { ...PAGE.image, width: 8000, height: 8001 } }
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, huge, TIMEOUT_MS)

    await flush()
    // The far end is the real app and answers properly, so the only thing that
    // can stop the frame is this side's own reading of the limits.
    socket.deliver(honestReady(socket))

    const failure = expectFailure(await outcome)
    expect(failure.message).toContain('outside the limits')
    expect(typesOf(socket)).toEqual(['hello'])
  })

  it('never sends a device pixel ratio past the contract', async () => {
    // G14. Security, MEDIUM-3. A ratio is what the app multiplies the page by,
    // and page zoom on a retina display can carry it past the 8 the contract
    // allows. Clamping it to 8 would tell the app a ten-times picture is an
    // eight-times one, so this goes to the download too.
    const socket = fakeSocket()
    const zoomed = { ...PAGE, image: { ...PAGE.image, devicePixelRatio: 9 } }
    const outcome = sendFullPage(() => Promise.resolve(socket), TOKEN, zoomed, TIMEOUT_MS)

    await flush()
    socket.deliver(honestReady(socket))

    const failure = expectFailure(await outcome)
    expect(failure.message).toContain('outside the limits')
    expect(typesOf(socket)).toEqual(['hello'])
  })

  it('never sends a handshake heavier than the far end will read', async () => {
    // G15. Security, MEDIUM-2. Until a token has been checked the bridge reads
    // at most 4096 bytes per frame, because a caller that has proved nothing
    // must not be able to make it hold a buffer of that caller's choosing. A
    // hello over that budget is closed rather than answered, so it is never
    // sent.
    const socket = fakeSocket()

    const outcome = await sendFullPage(
      () => Promise.resolve(socket),
      'a'.repeat(5000),
      PAGE,
      TIMEOUT_MS,
    )

    expect(expectFailure(outcome).ok).toBe(false)
    expect(socket.sent).toHaveLength(0)
  })
})

describe('testBridgeConnection', () => {
  it('shakes hands both ways and stops there', async () => {
    // The options page's `Test connection` button. It is pressed while the user
    // is looking at a token they may have pasted wrong, and a button that
    // uploads a page to find that out would be a surprise.
    const socket = fakeSocket()
    const outcome = testBridgeConnection(() => Promise.resolve(socket), TOKEN, TIMEOUT_MS)

    await flush()
    socket.deliver(honestReady(socket))

    await expect(outcome).resolves.toEqual({ ok: true, savedPath: null })
    expect(typesOf(socket)).toEqual(['hello'])
  })

  it('tells the options page when the answer came from something else', async () => {
    // The same button is the one place a user can find out that the port is
    // held by something that is not Snapdeck without capturing a page first.
    const socket = fakeSocket()
    const outcome = testBridgeConnection(() => Promise.resolve(socket), TOKEN, TIMEOUT_MS)

    await flush()
    socket.deliver(readyWith(honestProof('the wrong token', nonceOf(socket))))

    const failure = expectFailure(await outcome)
    expect(failure.code).toBe('impostor')
    expect(failure.message).toContain('51837')
  })
})
