//! The loopback bridge the Chrome extension hands a full-page capture over.
//!
//! Nothing here reaches the network. The bridge listens on `127.0.0.1` and
//! only ever answers a browser extension running on the same machine, which is
//! why the two things it is built out of first are the pairing token and the
//! message schema: the token says which extension, the schema says what it is
//! allowed to say.
//!
//! `protocol` is a mirror, not a source. The contract lives once, in
//! `packages/protocol`, and the extension imports it; this side restates it in
//! serde and is held to it by tests that read those TypeScript files.

pub mod protocol;
pub mod token;
