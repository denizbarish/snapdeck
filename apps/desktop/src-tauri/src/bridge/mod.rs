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
pub mod server;
pub mod session;
pub mod token;

use protocol::{AppInfo, FullPage};
use session::BridgePolicy;

/// The policy the bridge runs under until a delivered page has somewhere to go.
///
/// The listener is written and tested before the save path on purpose: it is
/// the piece that decides who may speak at all, and it is the piece that is
/// dangerous to get wrong. So a session can be opened and paired here, and a
/// page it delivers is refused, because turning a delivered PNG into a saved
/// capture is `bridge::intake`'s and the stored token is the settings window's.
///
/// The token is minted per run rather than stored, for the same reason: there
/// is nowhere yet to show the user what to paste, and a token nobody can read
/// is the honest state of a bridge that cannot yet save anything.
pub struct LaunchPolicy {
    token: String,
    app: AppInfo,
}

impl LaunchPolicy {
    pub fn new(token: String, app: AppInfo) -> Self {
        Self { token, app }
    }
}

impl BridgePolicy for LaunchPolicy {
    fn token(&self) -> String {
        self.token.clone()
    }

    fn app_info(&self) -> AppInfo {
        self.app.clone()
    }

    fn deliver(&self, _message: &FullPage) -> Result<Option<String>, String> {
        Err("this build cannot save a page delivered over the bridge yet".to_owned())
    }
}
