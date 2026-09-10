/**
 * The numbers both sides of the bridge agree on. The desktop mirrors them in
 * `bridge/protocol.rs`; nothing here may drift without the version going up.
 */

/** Bumped whenever a field is added, removed or given a new meaning. */
export const PROTOCOL_VERSION = 1

/** Fixed loopback port, inside the IANA dynamic range so it steals nothing. */
export const BRIDGE_PORT = 51837

/** 40 MiB. The largest PNG the bridge will carry. */
export const MAX_PNG_BYTES = 41_943_040

/**
 * 64 MiB. Not an independent number: it is what `MAX_PNG_BYTES` costs once it
 * is base64 encoded and wrapped in the JSON envelope. `limits.test.ts` holds
 * the two together.
 */
export const MAX_MESSAGE_BYTES = 67_108_864

/** Below Chrome's maximum desktop canvas area, so the extension can build it. */
export const MAX_IMAGE_PIXELS = 64_000_000

/** The only `Origin` the handshake accepts. */
export const EXTENSION_ORIGIN_PREFIX = 'chrome-extension://'

/** WebSocket close code used whenever one of the gates shuts the connection. */
export const CLOSE_POLICY_VIOLATION = 1008

/**
 * Hex characters in a `hello` nonce: 16 bytes of the extension's own
 * randomness. Fixed rather than bounded, because a nonce shorter than this is
 * not a nonce and one longer is a different protocol.
 */
export const NONCE_HEX_LENGTH = 32

/** Hex characters in a `ready` proof: the 32 bytes of an HMAC-SHA256 digest. */
export const PROOF_HEX_LENGTH = 64

/**
 * 4 KiB. What the socket accepts while it is still waiting for `hello`.
 *
 * Every field of a `hello` is short by schema, so a handshake has no reason to
 * weigh more than this, and until the token has been checked the peer is
 * nobody: a caller that has proved nothing must not be able to make this side
 * hold `MAX_MESSAGE_BYTES` of its choosing. The full limit comes on once the
 * token is in.
 */
export const MAX_HANDSHAKE_MESSAGE_BYTES = 4096

/**
 * The caps on every string and number that crosses the bridge, counted the way
 * each side counts: UTF-16 code units in zod, Unicode scalar values in serde.
 * The two agree for everything below U+10000 and differ by at most a factor of
 * two above it, which is a bound either way, and `MAX_MESSAGE_BYTES` is the
 * hard one underneath them all.
 */
export const MAX_TOKEN_CHARS = 256
export const MAX_CLIENT_NAME_CHARS = 64
export const MAX_CLIENT_VERSION_CHARS = 32
export const MAX_REQUEST_ID_CHARS = 64
export const MAX_URL_CHARS = 2048
export const MAX_TITLE_CHARS = 1024

/** What a failure the bridge reports may say, before it is cut to fit. */
export const MAX_ERROR_MESSAGE_CHARS = 1024

/** Far past any path either operating system will hand back. */
export const MAX_SAVED_PATH_CHARS = 4096

/**
 * The largest device pixel ratio a real display reports, with room over it.
 * A ratio is what the extension multiplies the page by, so an unbounded one is
 * a way of asking for an unbounded canvas.
 */
export const MAX_DEVICE_PIXEL_RATIO = 8

/**
 * The longest `pngBase64` a message may carry: four characters for every three
 * bytes of `MAX_PNG_BYTES`, rounded up to the padded group. Derived rather than
 * written down, so the two numbers cannot drift apart.
 */
export const MAX_PNG_BASE64_CHARS = Math.ceil(MAX_PNG_BYTES / 3) * 4

/**
 * The characters a `requestId` is built from. The extension uses
 * `crypto.randomUUID()`, and this is that alphabet with room around it: an id
 * travels into log lines and answers, and nothing that ends up there should be
 * able to carry a newline or a quote.
 */
export const REQUEST_ID_PATTERN = /^[A-Za-z0-9_-]+$/
