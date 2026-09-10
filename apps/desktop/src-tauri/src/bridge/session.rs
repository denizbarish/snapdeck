//! One bridge session, from the first frame to the last.
//!
//! The socket is deliberately not here. What a session is, is an order: the
//! first frame has to be a `hello`, the version is checked before the token,
//! and only a connection that got past both is answered with `ready`. That
//! order is the whole of the security story above the handshake, and a rule
//! that can only be exercised by opening a real socket is a rule that gets
//! tested once and then trusted forever. So the transport is a trait, the
//! policy is a trait, and `run_session` is a function over the two of them.
//!
//! `origin_is_allowed` is split out for the same reason `lib::adopt_shortcuts`
//! is: it is the entire rule, and a live handshake is not something a unit test
//! can arrange.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::bridge::protocol::{
    AppInfo, BridgeError, ClientMessage, ErrorCode, FullPage, ServerMessage,
    CLOSE_POLICY_VIOLATION, EXTENSION_ORIGIN_PREFIX, PROTOCOL_VERSION,
};
use crate::bridge::token::tokens_match;

/// Characters in a Chrome extension id.
const EXTENSION_ID_LENGTH: usize = 32;

/// The first and last character a Chrome extension id is built from.
///
/// Chrome renders the id of an extension from the first 128 bits of a hash, one
/// nibble per character, mapped onto `a`..`p` rather than onto hex. So the
/// alphabet is exactly sixteen letters wide and the length is exactly 32.
const EXTENSION_ID_FIRST: u8 = b'a';
const EXTENSION_ID_LAST: u8 = b'p';

/// What the extension is told when its first frame is not a `hello`.
const FIRST_FRAME_MUST_BE_HELLO: &str = "the first frame of a session has to be `hello`";

/// What the extension is told when it says `hello` twice.
const ONE_HELLO_PER_SESSION: &str = "a session is opened by one `hello`, and this one is open";

/// The digits a proof is rendered with, matching the token's own.
const HEX_DIGITS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// What `proof_for` answers if HMAC ever refuses the token as a key.
///
/// Unreachable as HMAC is built: it takes a key of any length and hashes down
/// the ones longer than its block, so `new_from_slice` answers `Err` for no
/// input at all. It is a value rather than an `expect` for the reason
/// `protocol::UNRENDERABLE_RESPONSE` is, and it is a proof of nothing rather
/// than something obviously broken so that the impossible fails in the safe
/// direction: it is well formed, it is the right answer for no token and no
/// nonce anyone holds, and an extension that receives it takes this side for an
/// impostor and sends no page. Which is correct, because a bridge that could
/// not compute its proof has not proved itself.
const PROOF_OF_NOTHING: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The bridge's half of the handshake: what only a server holding the pairing
/// token can say about the nonce the extension just made up.
///
/// `HMAC-SHA256(key = the token, message = the nonce)`, lowercase hex.
///
/// Both arguments are used as the strings they are, over their UTF-8 bytes,
/// rather than as the bytes their hex spells. The token is 64 hex characters
/// and the nonce is 32, so either reading is available to both sides, and a
/// protocol that leaves the choice open is one where the two implementations
/// agree on every line of this document and still never pair. The string is the
/// reading, in both languages, and there is nothing to decode before signing.
///
/// This is the answer to the one thing the design was missing: the token proved
/// the extension to the bridge and nothing proved the bridge to the extension,
/// so a program that took port 51837 first could collect a token it never
/// checked, answer `ready` out of nothing, and be handed every page the user
/// captured while they watched it succeed.
pub fn proof_for(token: &str, nonce: &str) -> String {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(token.as_bytes()) else {
        return PROOF_OF_NOTHING.to_owned();
    };
    mac.update(nonce.as_bytes());

    let digest = mac.finalize().into_bytes();
    let mut proof = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Both halves of a byte are 0..=15, which is the length of `HEX_DIGITS`.
        proof.push(HEX_DIGITS[usize::from(byte >> 4)]);
        proof.push(HEX_DIGITS[usize::from(byte & 0x0f)]);
    }
    proof
}

/// Whether a handshake's `Origin` may open a bridge session.
///
/// Pure, and separated from the socket for the reason `lib::adopt_shortcuts`
/// is: this is the whole of the rule, and a live handshake is not something a
/// unit test can arrange.
///
/// The id itself is not pinned. An unpacked development load and a Web Store
/// release have different ids, and accepting both is only possible by checking
/// the scheme and the shape. That is enough for the threat this gate exists
/// for: the danger is an ordinary web page's JavaScript opening a socket to
/// loopback, and a browser will not let that page forge an `Origin`. The token
/// is the second half of the rule.
pub fn origin_is_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return false;
    };
    let Some(id) = origin.strip_prefix(EXTENSION_ORIGIN_PREFIX) else {
        return false;
    };
    id.len() == EXTENSION_ID_LENGTH
        && id
            .bytes()
            .all(|character| (EXTENSION_ID_FIRST..=EXTENSION_ID_LAST).contains(&character))
}

/// One session's transport, so the state machine can be driven without a socket.
pub trait Frames {
    fn recv_text(&mut self) -> Result<String, String>;
    fn send_text(&mut self, text: &str) -> Result<(), String>;
    fn close(&mut self, code: u16, reason: &str);
    /// Called once, after the token has been checked and before `ready` goes
    /// out. What a transport does with it is its own business; a real socket
    /// lifts the two limits it accepted an unauthenticated peer under.
    fn authenticated(&mut self);
}

/// What a connection is allowed to do, and who decides.
///
/// Injected rather than read from an `AppHandle`, so the gate can be tested
/// against a real socket without a running Tauri application.
pub trait BridgePolicy: Send + Sync + 'static {
    fn token(&self) -> String;
    fn app_info(&self) -> AppInfo;
    /// Handles one accepted page. `Ok(None)` means the picture reached the
    /// clipboard but not the disk, exactly as a region capture can.
    fn deliver(&self, message: &FullPage) -> Result<Option<String>, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEnd {
    /// The peer closed, or the transport ended.
    Closed,
    /// Refused, with the code the extension was told.
    Refused(ErrorCode),
}

/// Runs one session to its end.
///
/// A failed `deliver` is the one thing here that does not end the session. It
/// is not a protocol violation: the extension said something well formed and
/// the disk refused it, and the user has to be able to try again without
/// re-pairing. Everything else that goes wrong is the extension saying
/// something this build will not answer, and that closes the connection.
pub fn run_session<F: Frames>(frames: &mut F, policy: &dyn BridgePolicy) -> SessionEnd {
    let Ok(first) = frames.recv_text() else {
        return SessionEnd::Closed;
    };
    let proof = match handshake(&first, policy) {
        Ok(proof) => proof,
        Err(error) => return refuse(frames, None, &error),
    };

    // The token is in. Whatever the transport was holding an unproven peer to
    // can come off now, and not one frame earlier.
    frames.authenticated();

    let ready = ServerMessage::Ready {
        protocol_version: PROTOCOL_VERSION,
        app: policy.app_info(),
        proof,
    };
    if frames.send_text(&ready.encode()).is_err() {
        return SessionEnd::Closed;
    }

    loop {
        let Ok(raw) = frames.recv_text() else {
            return SessionEnd::Closed;
        };
        let page = match ClientMessage::parse(&raw) {
            Ok(ClientMessage::FullPage(page)) => page,
            Ok(ClientMessage::Hello(_)) => {
                let error = BridgeError::Schema(ONE_HELLO_PER_SESSION.to_owned());
                return refuse(frames, None, &error);
            }
            Err(error) => return refuse(frames, None, &error),
        };

        let answer = match policy.deliver(&page) {
            Ok(saved_path) => ServerMessage::Accepted {
                request_id: page.request_id,
                saved_path,
            },
            Err(reason) => {
                ServerMessage::error(Some(page.request_id), ErrorCode::SaveFailed, &reason)
            }
        };
        if frames.send_text(&answer.encode()).is_err() {
            return SessionEnd::Closed;
        }
    }
}

/// The three gates the first frame has to pass, in the order they are asked.
///
/// The version is checked before the token on purpose. A version mismatch has
/// to reach the user as a version mismatch: told it was an authorisation
/// failure, they would go and paste the token again, and again, while the real
/// answer is that one of the two sides needs updating. Nothing is hidden by
/// answering honestly here, either, since a successful handshake has already
/// told the caller that a bridge is listening.
///
/// The proof comes back rather than being built by the caller, because it is
/// the last step of this sequence and nothing else may reach it: computing it
/// needs the token, and a proof built anywhere but after the comparison below
/// would be a proof handed to a caller that failed it.
fn handshake(raw: &str, policy: &dyn BridgePolicy) -> Result<String, BridgeError> {
    let hello = match ClientMessage::parse(raw)? {
        ClientMessage::Hello(hello) => hello,
        ClientMessage::FullPage(_) => {
            return Err(BridgeError::Schema(FIRST_FRAME_MUST_BE_HELLO.to_owned()))
        }
    };

    if hello.protocol_version != PROTOCOL_VERSION {
        return Err(BridgeError::VersionMismatch {
            presented: hello.protocol_version,
        });
    }
    let token = policy.token();
    if !tokens_match(&token, &hello.token) {
        return Err(BridgeError::Unauthorized);
    }
    Ok(proof_for(&token, &hello.nonce))
}

/// Tells the extension why, then closes.
///
/// The frame is sent before the close so that the extension has something to
/// show the user: a connection that simply drops is indistinguishable from the
/// application not running.
fn refuse<F: Frames>(
    frames: &mut F,
    request_id: Option<String>,
    error: &BridgeError,
) -> SessionEnd {
    let code = error.code();
    let reason = error.to_string();
    let message = ServerMessage::error(request_id, code, &reason);
    // Both failures are ignored: this is the last thing said on a connection
    // that is already going away, and there is nowhere left to report it to.
    let _ = frames.send_text(&message.encode());
    frames.close(CLOSE_POLICY_VIOLATION, &reason);
    SessionEnd::Refused(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// A transport that is a script: what the peer says, and what it heard.
    #[derive(Default)]
    struct FakeFrames {
        incoming: VecDeque<String>,
        sent: Vec<String>,
        closed: Option<(u16, String)>,
        /// How many frames had been sent when `authenticated` was called, so a
        /// test can say not just that it happened but where in the order.
        authenticated_after: Option<usize>,
        authentications: usize,
    }

    impl FakeFrames {
        fn saying(frames: &[String]) -> Self {
            Self {
                incoming: frames.iter().cloned().collect(),
                ..Self::default()
            }
        }

        /// The `type` of every frame the session sent, in order.
        fn sent_types(&self) -> Vec<String> {
            self.sent.iter().map(|frame| frame_type(frame)).collect()
        }
    }

    impl Frames for FakeFrames {
        fn recv_text(&mut self) -> Result<String, String> {
            self.incoming
                .pop_front()
                .ok_or_else(|| "the peer has nothing left to say".to_owned())
        }

        fn send_text(&mut self, text: &str) -> Result<(), String> {
            self.sent.push(text.to_owned());
            Ok(())
        }

        fn close(&mut self, code: u16, reason: &str) {
            self.closed = Some((code, reason.to_owned()));
        }

        fn authenticated(&mut self) {
            self.authenticated_after = Some(self.sent.len());
            self.authentications += 1;
        }
    }

    /// A policy that records what reached it and answers from a script.
    struct FakePolicy {
        token: String,
        answers: Mutex<VecDeque<Result<Option<String>, String>>>,
        deliveries: Mutex<Vec<String>>,
    }

    impl FakePolicy {
        fn holding(token: &str) -> Self {
            Self {
                token: token.to_owned(),
                answers: Mutex::new(VecDeque::new()),
                deliveries: Mutex::new(Vec::new()),
            }
        }

        fn answering(token: &str, answers: Vec<Result<Option<String>, String>>) -> Self {
            Self {
                token: token.to_owned(),
                answers: Mutex::new(answers.into()),
                deliveries: Mutex::new(Vec::new()),
            }
        }

        /// The request ids `deliver` was called with, in order.
        fn delivered(&self) -> Vec<String> {
            self.deliveries.lock().expect("no test panics here").clone()
        }
    }

    impl BridgePolicy for FakePolicy {
        fn token(&self) -> String {
            self.token.clone()
        }

        fn app_info(&self) -> AppInfo {
            AppInfo {
                name: "snapdeck".to_owned(),
                version: "0.1.0".to_owned(),
            }
        }

        fn deliver(&self, message: &FullPage) -> Result<Option<String>, String> {
            self.deliveries
                .lock()
                .expect("no test panics here")
                .push(message.request_id.clone());
            self.answers
                .lock()
                .expect("no test panics here")
                .pop_front()
                .unwrap_or(Ok(None))
        }
    }

    /// The pairing token these tests hand around. Any 64 hex characters.
    const TEST_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// An `Origin` a real extension would send: 32 characters of `a`..`p`.
    const REAL_EXTENSION_ORIGIN: &str = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// The same, with an id that uses every letter of the alphabet Chrome maps
    /// onto. A gate narrowed to `a` alone would still pass the one above.
    const WIDE_EXTENSION_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";

    /// Sixteen bytes of hex, which is what a `hello` has to carry.
    const TEST_NONCE: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";

    fn hello_frame(protocol_version: u32, token: &str) -> String {
        hello_frame_with_nonce(protocol_version, token, TEST_NONCE)
    }

    fn hello_frame_with_nonce(protocol_version: u32, token: &str, nonce: &str) -> String {
        format!(
            r#"{{"type":"hello","protocolVersion":{protocol_version},"token":"{token}","nonce":"{nonce}","client":{{"name":"snapdeck-extension","version":"0.1.0"}}}}"#
        )
    }

    fn full_page_frame(request_id: &str) -> String {
        format!(
            r#"{{"type":"fullPage","requestId":"{request_id}","page":{{"url":"https://example.com/","title":"Example"}},"image":{{"pngBase64":"iVBORw0KGgo=","width":4,"height":3,"devicePixelRatio":2}},"truncated":false}}"#
        )
    }

    fn parse_frame(frame: &str) -> serde_json::Value {
        serde_json::from_str(frame).expect("the bridge only ever sends JSON")
    }

    fn frame_type(frame: &str) -> String {
        parse_frame(frame)["type"]
            .as_str()
            .expect("every frame carries a string `type`")
            .to_owned()
    }

    /// S1. The origin a real extension presents. Both ends of the alphabet are
    /// here on purpose: a gate written against `a` alone would pass the first
    /// of these and lock every user whose id contains a `p` out of the bridge.
    #[test]
    fn a_chrome_extension_origin_is_allowed() {
        assert!(
            origin_is_allowed(Some(REAL_EXTENSION_ORIGIN)),
            "an extension origin has to be able to open a session: {REAL_EXTENSION_ORIGIN}"
        );
        assert!(
            origin_is_allowed(Some(WIDE_EXTENSION_ORIGIN)),
            "extension ids run over sixteen letters, not one: {WIDE_EXTENSION_ORIGIN}"
        );
    }

    /// S2. Security. The gate that keeps an ordinary web page off the bridge.
    /// A page's JavaScript cannot forge `Origin`, so every one of these is
    /// either a browser page or something that declined to say what it was, and
    /// neither may open a session.
    #[test]
    fn nothing_but_an_extension_origin_is_allowed() {
        for origin in [
            None,
            Some(""),
            Some("null"),
            Some("http://localhost:1420"),
            Some("https://evil.example"),
            Some("file://"),
            // A page whose host is shaped exactly like an extension id. The
            // scheme is the half of the rule that catches this one, and
            // without it a domain anybody can register would be a way in.
            Some("https://abcdefghijklmnopabcdefghijklmnop"),
        ] {
            assert!(
                !origin_is_allowed(origin),
                "the bridge is not open to {origin:?}"
            );
        }
    }

    /// S3. Security. The scheme is half the rule; the shape of the id is the
    /// other half, or `chrome-extension://` on its own would be a way in.
    #[test]
    fn an_extension_origin_of_the_wrong_shape_is_refused() {
        assert!(
            !origin_is_allowed(Some("chrome-extension://short")),
            "an id has to be 32 characters long"
        );
        assert!(
            !origin_is_allowed(Some("chrome-extension://")),
            "and there has to be an id at all"
        );
        assert!(
            !origin_is_allowed(Some("chrome-extension://abcdefghijklmnopabcdefghijklmnoz")),
            "`z` is not a character Chrome renders an id with"
        );
    }

    /// S4. Security. A connection that never presented a token delivers
    /// nothing, whatever it says. This is the gate that makes the token
    /// mandatory rather than merely available.
    #[test]
    fn a_session_whose_first_frame_is_a_page_delivers_nothing() {
        let policy = FakePolicy::holding(TEST_TOKEN);
        let mut frames = FakeFrames::saying(&[full_page_frame("first")]);

        let end = run_session(&mut frames, &policy);

        assert_eq!(
            end,
            SessionEnd::Refused(ErrorCode::MalformedMessage),
            "a page before a handshake is a malformed session"
        );
        assert!(
            policy.delivered().is_empty(),
            "nothing may reach the save path before the token has been checked: {:?}",
            policy.delivered()
        );
        assert_eq!(
            frames.closed.as_ref().map(|(code, _)| *code),
            Some(1008),
            "the connection is closed with the policy violation code"
        );
    }

    /// S5. Security. The wrong token is the whole of what stands between
    /// another program on this machine and the user's screen.
    #[test]
    fn the_wrong_token_is_refused_and_never_answered_with_ready() {
        let policy = FakePolicy::holding(TEST_TOKEN);
        let mut frames = FakeFrames::saying(&[
            hello_frame(PROTOCOL_VERSION, "not the token"),
            full_page_frame("first"),
        ]);

        let end = run_session(&mut frames, &policy);

        assert_eq!(end, SessionEnd::Refused(ErrorCode::Unauthorized));
        assert!(
            !frames.sent_types().contains(&"ready".to_owned()),
            "a session that failed the token is never told it is open: {:?}",
            frames.sent_types()
        );
        assert!(
            policy.delivered().is_empty(),
            "and nothing it queued afterwards is delivered: {:?}",
            policy.delivered()
        );
    }

    /// S6. Security, and a usability rule with teeth. A `hello` that is wrong
    /// about both things is answered about the version, because that is the one
    /// the user can act on; told it was the token, they would paste it again.
    #[test]
    fn a_version_mismatch_is_reported_before_the_token_is_looked_at() {
        let policy = FakePolicy::holding(TEST_TOKEN);
        let mut frames = FakeFrames::saying(&[hello_frame(PROTOCOL_VERSION + 1, "not the token")]);

        let end = run_session(&mut frames, &policy);

        assert_eq!(
            end,
            SessionEnd::Refused(ErrorCode::UnsupportedProtocolVersion),
            "the version is checked first, so the version is what is reported"
        );
        let sent = frames.sent.first().expect("a refusal is always explained");
        assert_eq!(
            parse_frame(sent)["code"],
            "unsupportedProtocolVersion",
            "and the code on the wire says so too: {sent}"
        );
    }

    /// S7. A frame that is the right type and the wrong shape closes the
    /// connection rather than being skipped. Half a schema is not a schema.
    #[test]
    fn a_hello_missing_its_fields_closes_the_connection() {
        let policy = FakePolicy::holding(TEST_TOKEN);
        let mut frames =
            FakeFrames::saying(&[r#"{"type":"hello"}"#.to_owned(), full_page_frame("first")]);

        let end = run_session(&mut frames, &policy);

        assert_eq!(end, SessionEnd::Refused(ErrorCode::MalformedMessage));
        assert!(
            frames.closed.is_some(),
            "a malformed handshake ends the connection"
        );
        assert!(
            policy.delivered().is_empty(),
            "and what followed it is never read: {:?}",
            policy.delivered()
        );
    }

    /// S8. The happy path, and the claim that a session is a session rather
    /// than one delivery: both pages arrive and both are answered.
    #[test]
    fn an_open_session_carries_more_than_one_page() {
        let policy = FakePolicy::answering(
            TEST_TOKEN,
            vec![
                Ok(Some("/tmp/one.png".to_owned())),
                Ok(Some("/tmp/two.png".to_owned())),
            ],
        );
        let mut frames = FakeFrames::saying(&[
            hello_frame(PROTOCOL_VERSION, TEST_TOKEN),
            full_page_frame("one"),
            full_page_frame("two"),
        ]);

        let end = run_session(&mut frames, &policy);

        assert_eq!(end, SessionEnd::Closed, "the peer ran out of frames");
        assert_eq!(
            frames.sent_types(),
            vec!["ready", "accepted", "accepted"],
            "the handshake is answered once and each page once"
        );
        assert_eq!(
            policy.delivered(),
            vec!["one".to_owned(), "two".to_owned()],
            "both pages reach the save path"
        );
        assert_eq!(
            parse_frame(&frames.sent[1])["savedPath"],
            "/tmp/one.png",
            "and the answer says where the first one went"
        );
    }

    /// S9. A save that failed is not a protocol violation. The user has to be
    /// able to fix a full disk and press the button again on the same session.
    #[test]
    fn a_failed_delivery_is_reported_and_the_session_stays_open() {
        let policy = FakePolicy::answering(
            TEST_TOKEN,
            vec![
                Err("the save folder is not writable".to_owned()),
                Ok(Some("/tmp/two.png".to_owned())),
            ],
        );
        let mut frames = FakeFrames::saying(&[
            hello_frame(PROTOCOL_VERSION, TEST_TOKEN),
            full_page_frame("one"),
            full_page_frame("two"),
        ]);

        let end = run_session(&mut frames, &policy);

        assert_eq!(end, SessionEnd::Closed, "not refused, merely finished");
        assert!(
            frames.closed.is_none(),
            "a failed save does not close the connection"
        );
        let failure = parse_frame(&frames.sent[1]);
        assert_eq!(failure["type"], "error", "the failure is reported");
        assert_eq!(failure["code"], "saveFailed", "as a failed save");
        assert_eq!(
            failure["requestId"], "one",
            "against the request that failed"
        );
        assert_eq!(
            policy.delivered(),
            vec!["one".to_owned(), "two".to_owned()],
            "and the next page is still delivered"
        );
        assert_eq!(frames.sent_types(), vec!["ready", "error", "accepted"]);
    }

    /// S10. A capture that only reached the clipboard is an answer, not a
    /// failure: a region capture can end the same way when no folder is set.
    #[test]
    fn a_page_that_reached_only_the_clipboard_is_accepted() {
        let policy = FakePolicy::answering(TEST_TOKEN, vec![Ok(None)]);
        let mut frames = FakeFrames::saying(&[
            hello_frame(PROTOCOL_VERSION, TEST_TOKEN),
            full_page_frame("one"),
        ]);

        run_session(&mut frames, &policy);

        let accepted = parse_frame(&frames.sent[1]);
        assert_eq!(accepted["type"], "accepted", "it is still an acceptance");
        assert!(
            accepted["savedPath"].is_null(),
            "with nothing where the path would be: {accepted}"
        );
    }

    /// S22. Security, HIGH-1. The proof, against a vector computed outside this
    /// program.
    ///
    /// `HMAC-SHA256(key = "0123…cdef", message = "0f1e…e1f0")` is what OpenSSL
    /// answers for those two strings, and the value is written out here rather
    /// than taken from this file's own arithmetic: an implementation that
    /// agrees only with itself is what a second implementation of this protocol
    /// cannot pair with.
    ///
    /// The third assertion is the one the two languages would otherwise argue
    /// about forever. Both the token and the nonce are hex, so a key could mean
    /// the string or the bytes it spells, and the two produce different proofs.
    /// The string is the reading, and the value that would come of the other one
    /// is here so that a future change to it fails rather than drifts.
    #[test]
    fn the_proof_is_hmac_sha256_of_the_nonce_under_the_token_string() {
        assert_eq!(
            proof_for(TEST_TOKEN, TEST_NONCE),
            "bd0b0ea0ed26cb9208586f1c5b039df3c2c157d46ade2476e82a127a6ea8636b",
            "the proof is the one an outside implementation computes for these two strings"
        );

        let other_token = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        assert_eq!(
            proof_for(other_token, TEST_NONCE),
            "f74e7964c7f57a58bfd8da3d39f20107532ed4b2b28cc90b47e9d689dba166dd",
            "and a different token answers a different proof for the same nonce"
        );

        assert_ne!(
            proof_for(TEST_TOKEN, TEST_NONCE),
            "0b7524f58c62339f46c8886db927c2edb6a39b64704ed9924857771e7ca30139",
            "the key is the token as a string, not the 32 bytes its hex spells"
        );
    }

    /// S23. Security, HIGH-1. The proof is over the nonce the extension chose,
    /// so a server that answers the same thing to every session is answering
    /// something it recorded rather than something it computed.
    #[test]
    fn a_different_nonce_gets_a_different_proof() {
        let first = proof_for(TEST_TOKEN, TEST_NONCE);
        let second = proof_for(TEST_TOKEN, "ffffffffffffffffffffffffffffffff");

        assert_ne!(
            first, second,
            "a proof that ignores the nonce is a proof that can be replayed"
        );
        // 64 characters is the contract: the 32 bytes of a SHA-256 digest.
        assert_eq!(first.len(), 64, "and it is 64 hex characters: {first}");
        assert!(
            first
                .chars()
                .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)),
            "lowercase hex, as the token is: {first}"
        );
    }

    /// S24. Security, HIGH-1. The whole of the scenario this exists for, from
    /// the session's side: the answer a paired extension gets carries the proof
    /// for the nonce it sent, and a program that does not hold the token cannot
    /// have written it.
    #[test]
    fn a_paired_session_is_answered_with_the_proof_for_its_own_nonce() {
        let nonce = "abcdefabcdefabcdefabcdefabcdefab";
        let policy = FakePolicy::holding(TEST_TOKEN);
        let mut frames =
            FakeFrames::saying(&[hello_frame_with_nonce(PROTOCOL_VERSION, TEST_TOKEN, nonce)]);

        run_session(&mut frames, &policy);

        let ready = parse_frame(frames.sent.first().expect("a paired session is answered"));
        assert_eq!(ready["type"], "ready");
        assert_eq!(
            ready["proof"],
            proof_for(TEST_TOKEN, nonce),
            "the proof is over the nonce this session presented: {ready}"
        );
        assert_ne!(
            ready["proof"],
            proof_for(TEST_TOKEN, TEST_NONCE),
            "and not over some other one this file happens to know"
        );
    }

    /// S25. Security, HIGH-1. A session that failed the token is told nothing
    /// it could learn the proof from, because the proof is only ever computed
    /// after the comparison the session failed.
    #[test]
    fn a_refused_session_is_never_given_a_proof() {
        let policy = FakePolicy::holding(TEST_TOKEN);
        let mut frames = FakeFrames::saying(&[hello_frame(PROTOCOL_VERSION, "not the token")]);

        run_session(&mut frames, &policy);

        let correct = proof_for(TEST_TOKEN, TEST_NONCE);
        for frame in &frames.sent {
            assert!(
                !frame.contains(&correct),
                "nothing said to a refused caller may carry the proof: {frame}"
            );
        }
        assert_eq!(
            frames.authentications, 0,
            "and the transport is never told the caller is authenticated"
        );
    }

    /// S26. Security, MEDIUM-1 and MEDIUM-2. The transport is told exactly once
    /// that the caller is proven, and it is told after the token was checked and
    /// before `ready` goes out. Everything a socket relaxes for a paired peer
    /// hangs off that call, so its place in the order is the claim.
    #[test]
    fn the_transport_is_told_once_and_only_after_the_token() {
        let policy = FakePolicy::answering(TEST_TOKEN, vec![Ok(None)]);
        let mut frames = FakeFrames::saying(&[
            hello_frame(PROTOCOL_VERSION, TEST_TOKEN),
            full_page_frame("one"),
        ]);

        run_session(&mut frames, &policy);

        assert_eq!(
            frames.authenticated_after,
            Some(0),
            "the transport is told before the first frame is sent, which is `ready`"
        );
        assert_eq!(
            frames.authentications, 1,
            "and once, however many pages the session goes on to carry"
        );
    }

    /// S27. Security, HIGH-1. A `hello` whose nonce is not a nonce ends the
    /// session rather than being signed. Anything this side puts its token to
    /// has to be 16 bytes the extension chose, not a message a caller composed.
    #[test]
    fn a_hello_whose_nonce_is_out_of_schema_never_reaches_ready() {
        for nonce in [
            // 32 hex characters is the contract.
            "0f1e2d3c4b5a69788796a5b4c3d2e1f",
            "0f1e2d3c4b5a69788796a5b4c3d2e1f00",
            "0f1e2d3c4b5a69788796a5b4c3d2e1fg",
            "",
        ] {
            let policy = FakePolicy::holding(TEST_TOKEN);
            let mut frames =
                FakeFrames::saying(&[hello_frame_with_nonce(PROTOCOL_VERSION, TEST_TOKEN, nonce)]);

            let end = run_session(&mut frames, &policy);

            assert_eq!(
                end,
                SessionEnd::Refused(ErrorCode::MalformedMessage),
                "a nonce of {} characters is not a handshake",
                nonce.len()
            );
            assert!(
                !frames.sent_types().contains(&"ready".to_owned()),
                "and nothing is signed for it: {:?}",
                frames.sent_types()
            );
        }
    }

    /// S11. One handshake per session. A second `hello` on an open session is
    /// either a confused extension or someone else's frame, and neither gets a
    /// second chance at the token.
    #[test]
    fn a_second_hello_ends_the_session() {
        let policy = FakePolicy::holding(TEST_TOKEN);
        let mut frames = FakeFrames::saying(&[
            hello_frame(PROTOCOL_VERSION, TEST_TOKEN),
            hello_frame(PROTOCOL_VERSION, TEST_TOKEN),
        ]);

        let end = run_session(&mut frames, &policy);

        assert_eq!(end, SessionEnd::Refused(ErrorCode::MalformedMessage));
        assert!(
            frames.closed.is_some(),
            "and the connection is closed behind it"
        );
    }
}
