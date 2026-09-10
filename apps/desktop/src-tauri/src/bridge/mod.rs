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
//!
//! The four gates a page passes are one per module, in the order it meets
//! them: `server` decides who may open a socket, `session` decides what they
//! may say, `protocol` decides what shape it has to be in, and `intake`
//! decides what the picture inside it becomes.

pub mod intake;
pub mod protocol;
pub mod server;
pub mod session;
pub mod token;

use std::sync::Mutex;

/// What `lib::open_bridge` was able to do, so the settings window can say it.
///
/// The user cannot see a listener. From the browser's side a Snapdeck that
/// never opened the port and one that refused the extension look identical, so
/// the window is the one place the difference can be told, and the answer has
/// to survive from launch until the window is opened.
///
/// Managed state of its own rather than a field of `AppState`, because it is
/// the bridge's own answer: nothing outside this module and the settings
/// window reads it, and the running listener is already held elsewhere for a
/// different reason, that dropping it stops it.
///
/// A `Result` rather than a pair of named states, because that is the shape of
/// the question: the port is the whole of the good answer and the sentence is
/// the whole of the bad one.
#[derive(Debug)]
pub struct BridgeOutcome(Mutex<Result<u16, String>>);

/// What the answer is before the launch has produced one.
///
/// Unreachable in the running application, since the settings window cannot be
/// opened before setup has finished, and still a sentence rather than a panic:
/// an unopened bridge is not a reason to refuse to say anything at all.
const NOT_STARTED: &str = "Snapdeck has not tried to open the bridge yet.";

impl Default for BridgeOutcome {
    fn default() -> Self {
        Self(Mutex::new(Err(NOT_STARTED.to_string())))
    }
}

impl BridgeOutcome {
    /// Records the port the bridge took, or why it took none.
    pub fn record(&self, outcome: Result<u16, String>) {
        *self.lock() = outcome;
    }

    /// The port the bridge is listening on, or why it is not.
    pub fn read(&self) -> Result<u16, String> {
        self.lock().clone()
    }

    /// Poisoning is treated as recoverable for the reason `AppState`'s locks
    /// give: the whole value is read or replaced by every user of it, and an
    /// unrelated panic must not leave the settings window unable to say
    /// anything about the bridge.
    fn lock(&self) -> std::sync::MutexGuard<'_, Result<u16, String>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
