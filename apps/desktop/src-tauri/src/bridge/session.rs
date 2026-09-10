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
    if let Err(error) = handshake(&first, policy) {
        return refuse(frames, None, &error);
    }

    let ready = ServerMessage::Ready {
        protocol_version: PROTOCOL_VERSION,
        app: policy.app_info(),
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
            Err(reason) => ServerMessage::Error {
                request_id: Some(page.request_id),
                code: ErrorCode::SaveFailed,
                message: reason,
            },
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
fn handshake(raw: &str, policy: &dyn BridgePolicy) -> Result<(), BridgeError> {
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
    if !tokens_match(&policy.token(), &hello.token) {
        return Err(BridgeError::Unauthorized);
    }
    Ok(())
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
    let message = ServerMessage::Error {
        request_id,
        code,
        message: reason.clone(),
    };
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
    }

    impl FakeFrames {
        fn saying(frames: &[String]) -> Self {
            Self {
                incoming: frames.iter().cloned().collect(),
                sent: Vec::new(),
                closed: None,
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

    fn hello_frame(protocol_version: u32, token: &str) -> String {
        format!(
            r#"{{"type":"hello","protocolVersion":{protocol_version},"token":"{token}","client":{{"name":"snapdeck-extension","version":"0.1.0"}}}}"#
        )
    }

    fn full_page_frame(request_id: &str) -> String {
        format!(
            r#"{{"type":"fullPage","requestId":"{request_id}","page":{{"url":"https://example.com/","title":"Example"}},"image":{{"pngBase64":"","width":4,"height":3,"devicePixelRatio":2}},"truncated":false}}"#
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
