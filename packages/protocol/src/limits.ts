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
