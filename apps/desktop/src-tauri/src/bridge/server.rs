//! The loopback listener, and the gate that runs before a session exists.
//!
//! The address is `127.0.0.1` and nothing else. A bridge that bound every
//! interface would put the user's screen one connection away from anyone on the
//! same network, and the extension has no reason to reach across a network:
//! Chrome and Snapdeck run on the same machine or the feature does not apply.
//! There is a test that reads this file looking for the address that would undo
//! that, because the difference between the safe version and the catastrophic
//! one is a single constant.
//!
//! The handshake carries the first gate. `Origin` is a header a browser fills in
//! and a page cannot forge, so refusing everything that is not a Chrome
//! extension is what keeps an ordinary web page's JavaScript off the socket, and
//! it is refused with an HTTP 403 before a WebSocket exists at all. The message
//! size limit is the second gate and belongs to the same layer, because a frame
//! too large to hold is refused before it is read rather than after.
//!
//! One thread per session, and a session is at most a few frames long. An async
//! runtime would buy nothing here: the bridge sees a handful of connections in
//! its life, and the sequence a session is has to be written down somewhere
//! either way.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{header::ORIGIN, StatusCode};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::frame::CloseFrame;
use tungstenite::protocol::{Message, WebSocket, WebSocketConfig};

use crate::bridge::protocol::MAX_MESSAGE_BYTES;
use crate::bridge::session::{origin_is_allowed, run_session, BridgePolicy, Frames};

/// How many sessions may be open at once.
///
/// One browser is the expected case and the limit is here for the unexpected
/// one: something on this machine opening connections in a loop must not be
/// able to spend the process's threads. Refusing the extra connection costs a
/// real user nothing, because a second Chrome profile still only needs a
/// session while a capture is in flight.
const MAX_SESSIONS: usize = 4;

/// How long a connection may stay silent before it is given up on.
///
/// It covers the HTTP handshake and the `hello` that has to follow it. After
/// that the clock comes off: a paired extension is allowed to sit idle between
/// two captures for as long as the user leaves the tab open.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// What a handshake with the wrong `Origin` is told, as the body of the 403.
const REFUSED_ORIGIN: &str = "the bridge only answers a Snapdeck browser extension";

#[derive(Debug, Clone, Copy)]
pub struct BridgeLimits {
    pub max_message_bytes: usize,
    pub max_sessions: usize,
    pub handshake_timeout: Duration,
}

impl Default for BridgeLimits {
    /// The protocol's own numbers. The application never builds this by hand.
    fn default() -> Self {
        Self {
            max_message_bytes: MAX_MESSAGE_BYTES,
            max_sessions: MAX_SESSIONS,
            handshake_timeout: HANDSHAKE_TIMEOUT,
        }
    }
}

/// The sessions that are open, and the count that decides whether another may
/// start.
///
/// A second handle on each socket is kept rather than a bare number, because
/// stopping the server has to reach a thread that is blocked in a read, and the
/// only thing that reaches such a thread is shutting the socket under it.
#[derive(Debug, Default)]
struct Sessions {
    open: Mutex<HashMap<u64, TcpStream>>,
    next_id: AtomicU64,
}

impl Sessions {
    /// Takes a slot for `stream`, or answers `None` when the bridge is full.
    fn admit(self: &Arc<Self>, stream: &TcpStream, max: usize) -> Option<SessionSlot> {
        let mut open = self.open.lock().ok()?;
        if open.len() >= max {
            return None;
        }
        let handle = stream.try_clone().ok()?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        open.insert(id, handle);
        Some(SessionSlot {
            sessions: Arc::clone(self),
            id,
        })
    }

    /// Ends every open session, whatever it is in the middle of.
    fn close_all(&self) {
        let Ok(mut open) = self.open.lock() else {
            return;
        };
        for (_, stream) in open.drain() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// One session's claim on a slot, given back when the session ends.
///
/// The give-back is a `Drop` rather than a line at the end of the session
/// thread, so that a session which ends by unwinding still returns its slot.
/// Counted the other way, one panic would shrink the bridge for the rest of the
/// run.
struct SessionSlot {
    sessions: Arc<Sessions>,
    id: u64,
}

impl Drop for SessionSlot {
    fn drop(&mut self) {
        if let Ok(mut open) = self.sessions.open.lock() {
            open.remove(&self.id);
        }
    }
}

/// A running listener. Dropping it stops accepting and ends open sessions.
#[derive(Debug)]
pub struct BridgeServer {
    local_addr: SocketAddr,
    stopping: Arc<AtomicBool>,
    sessions: Arc<Sessions>,
    accepting: Option<JoinHandle<()>>,
}

impl BridgeServer {
    /// Binds `127.0.0.1:port` and serves until dropped.
    ///
    /// `port` is `protocol::BRIDGE_PORT` in the application and 0 in tests, so
    /// the operating system picks a free one and `local_addr` reports it.
    pub fn start(
        port: u16,
        policy: Arc<dyn BridgePolicy>,
        limits: BridgeLimits,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|err| {
            format!(
                "the bridge could not take port {port} on this machine, so the browser extension has nothing to connect to: {err}"
            )
        })?;
        let local_addr = listener.local_addr().map_err(|err| {
            format!("the bridge took port {port} but could not read the address back: {err}")
        })?;

        let stopping = Arc::new(AtomicBool::new(false));
        let sessions = Arc::new(Sessions::default());
        let accepting = {
            let stopping = Arc::clone(&stopping);
            let sessions = Arc::clone(&sessions);
            std::thread::spawn(move || accept_loop(listener, policy, limits, &stopping, &sessions))
        };

        Ok(Self {
            local_addr,
            stopping,
            sessions,
            accepting: Some(accepting),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.sessions.close_all();
        // The accept loop is asleep inside `accept`, and the flag above cannot
        // wake it. One connection of our own does, and what it finds when it
        // wakes is the flag.
        let _ = TcpStream::connect(self.local_addr);
        if let Some(accepting) = self.accepting.take() {
            let _ = accepting.join();
        }
    }
}

/// Accepts until the server is dropped, one thread per session.
fn accept_loop(
    listener: TcpListener,
    policy: Arc<dyn BridgePolicy>,
    limits: BridgeLimits,
    stopping: &AtomicBool,
    sessions: &Arc<Sessions>,
) {
    loop {
        let accepted = listener.accept();
        if stopping.load(Ordering::Acquire) {
            return;
        }
        // A connection that died between the queue and here is the peer's
        // business, not a reason to stop listening.
        let Ok((stream, _)) = accepted else {
            continue;
        };

        let Some(slot) = sessions.admit(&stream, limits.max_sessions) else {
            // Over the limit. Accepted and closed rather than left queued, so
            // the caller hears now instead of waiting behind a session that may
            // last as long as the user's browsing does.
            let _ = stream.shutdown(Shutdown::Both);
            continue;
        };

        let policy = Arc::clone(&policy);
        std::thread::spawn(move || {
            // Held for the length of the session, and given back by dropping.
            let _slot = slot;
            serve(stream, policy.as_ref(), limits);
        });
    }
}

/// Runs the handshake gate and then one session over the accepted socket.
fn serve(stream: TcpStream, policy: &dyn BridgePolicy, limits: BridgeLimits) {
    // A second handle on the same socket, because `WebSocket` takes the stream
    // and does not lend it back, and the read timeout has to come off once the
    // session is under way.
    let Ok(control) = stream.try_clone() else {
        return;
    };
    if stream
        .set_read_timeout(Some(limits.handshake_timeout))
        .is_err()
    {
        return;
    }

    // Both limits, because a message arrives as frames: capping the message
    // alone would still let a single oversized frame be read into memory first.
    let config = WebSocketConfig::default()
        .max_message_size(Some(limits.max_message_bytes))
        .max_frame_size(Some(limits.max_message_bytes));

    let Ok(socket) = tungstenite::accept_hdr_with_config(stream, origin_gate, Some(config)) else {
        // Either the `Origin` gate refused, in which case the 403 has already
        // been written, or the peer went away mid-handshake. Neither is
        // something this side can do anything further about.
        return;
    };

    let mut frames = SocketFrames {
        socket,
        control,
        past_first_frame: false,
    };
    run_session(&mut frames, policy);
}

/// The first gate: who is allowed to open a WebSocket at all.
///
/// Refusing here rather than after the upgrade is the point. A page that is
/// told 403 never gets a socket, so there is no frame of its to parse and no
/// state of its to hold.
#[expect(
    clippy::result_large_err,
    reason = "this signature is tungstenite's `Callback`, and the refusal it asks for is an \
              `http::Response`. Boxing it, which is what the lint suggests, would no longer \
              satisfy the trait, and the value is built once per refused handshake."
)]
fn origin_gate(request: &Request, response: Response) -> Result<Response, ErrorResponse> {
    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin_is_allowed(origin) {
        return Ok(response);
    }

    let mut refusal = ErrorResponse::new(Some(REFUSED_ORIGIN.to_owned()));
    *refusal.status_mut() = StatusCode::FORBIDDEN;
    Err(refusal)
}

/// The session state machine's view of a real socket.
struct SocketFrames {
    socket: WebSocket<TcpStream>,
    control: TcpStream,
    past_first_frame: bool,
}

impl Frames for SocketFrames {
    fn recv_text(&mut self) -> Result<String, String> {
        loop {
            let message = self.socket.read().map_err(|err| err.to_string())?;
            match message {
                Message::Text(text) => {
                    if !self.past_first_frame {
                        self.past_first_frame = true;
                        // The clock was for the handshake and the `hello` that
                        // follows it. From here the session is the user's to
                        // leave open.
                        let _ = self.control.set_read_timeout(None);
                    }
                    return Ok(text.to_string());
                }
                // Answered by tungstenite itself; neither is a bridge frame.
                Message::Ping(_) | Message::Pong(_) => continue,
                Message::Binary(_) => {
                    return Err("the bridge carries text frames, and this one is binary".to_owned())
                }
                Message::Close(_) => return Err("the extension closed the session".to_owned()),
                Message::Frame(_) => return Err("the bridge does not read raw frames".to_owned()),
            }
        }
    }

    fn send_text(&mut self, text: &str) -> Result<(), String> {
        self.socket
            .send(Message::text(text.to_owned()))
            .map_err(|err| err.to_string())
    }

    fn close(&mut self, code: u16, reason: &str) {
        let frame = CloseFrame {
            code: CloseCode::from(code),
            reason: reason.to_owned().into(),
        };
        // Queued, then pushed. Neither failure has anywhere to go: this is the
        // last thing said on a connection that is already ending.
        let _ = self.socket.close(Some(frame));
        let _ = self.socket.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::Mutex as StdMutex;

    use tungstenite::client::IntoClientRequest;
    use tungstenite::http::HeaderValue;

    use crate::bridge::protocol::{AppInfo, FullPage, PROTOCOL_VERSION};

    /// The pairing token these tests hand around. Any 64 hex characters.
    const TEST_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// An `Origin` a real extension would send.
    const REAL_EXTENSION_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";

    /// Where a delivered page is said to have gone.
    const SAVED_PATH: &str = "/tmp/snapdeck-test.png";

    /// How long a test client waits before calling the bridge silent.
    ///
    /// Long enough that a loaded machine does not fail a passing test, short
    /// enough that a hung bridge is a failed test rather than a hung suite.
    const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

    /// How long a test waits for a session slot to come back, and how often it
    /// looks.
    const SLOT_ATTEMPTS: usize = 100;
    const SLOT_INTERVAL: Duration = Duration::from_millis(50);

    /// A policy that records what reached it and always accepts.
    struct RecordingPolicy {
        token: String,
        deliveries: StdMutex<Vec<String>>,
    }

    impl RecordingPolicy {
        fn holding(token: &str) -> Arc<Self> {
            Arc::new(Self {
                token: token.to_owned(),
                deliveries: StdMutex::new(Vec::new()),
            })
        }

        /// The request ids `deliver` was called with, in order.
        fn delivered(&self) -> Vec<String> {
            self.deliveries.lock().expect("no test panics here").clone()
        }
    }

    impl BridgePolicy for RecordingPolicy {
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
            Ok(Some(SAVED_PATH.to_owned()))
        }
    }

    fn hello_frame(token: &str) -> String {
        format!(
            r#"{{"type":"hello","protocolVersion":{PROTOCOL_VERSION},"token":"{token}","client":{{"name":"snapdeck-extension","version":"0.1.0"}}}}"#
        )
    }

    /// A capture frame whose base64 payload is `padding` characters long, so a
    /// test can decide how big the message on the wire is.
    fn full_page_frame(request_id: &str, padding: usize) -> String {
        let png = "A".repeat(padding);
        format!(
            r#"{{"type":"fullPage","requestId":"{request_id}","page":{{"url":"https://example.com/","title":"Example"}},"image":{{"pngBase64":"{png}","width":4,"height":3,"devicePixelRatio":2}},"truncated":false}}"#
        )
    }

    /// Opens a client the way the extension would, `Origin` and all.
    fn connect(
        addr: SocketAddr,
        origin: Option<&str>,
    ) -> Result<WebSocket<TcpStream>, tungstenite::Error> {
        let mut request = format!("ws://{addr}/")
            .into_client_request()
            .expect("a loopback address makes a websocket uri");
        if let Some(origin) = origin {
            request.headers_mut().insert(
                ORIGIN,
                HeaderValue::from_str(origin).expect("a test origin is a header value"),
            );
        }

        let stream = TcpStream::connect(addr).map_err(tungstenite::Error::Io)?;
        stream
            .set_read_timeout(Some(CLIENT_TIMEOUT))
            .expect("a fresh socket takes a timeout");

        match tungstenite::client::client_with_config(request, stream, None) {
            Ok((socket, _)) => Ok(socket),
            Err(tungstenite::HandshakeError::Failure(err)) => Err(err),
            Err(tungstenite::HandshakeError::Interrupted(_)) => {
                unreachable!("a blocking handshake does not come back interrupted")
            }
        }
    }

    /// Opens a client and pairs it, leaving a session that is ready for pages.
    fn paired(addr: SocketAddr) -> WebSocket<TcpStream> {
        let mut client = connect(addr, Some(REAL_EXTENSION_ORIGIN)).expect("the handshake passes");
        client
            .send(Message::text(hello_frame(TEST_TOKEN)))
            .expect("the bridge takes a hello");
        let ready = read_text(&mut client);
        assert_eq!(
            frame_type(&ready),
            "ready",
            "a paired session is answered with ready: {ready}"
        );
        client
    }

    fn read_text(client: &mut WebSocket<TcpStream>) -> String {
        loop {
            match client.read().expect("the bridge answers") {
                Message::Text(text) => return text.to_string(),
                Message::Ping(_) | Message::Pong(_) => continue,
                other => panic!("the bridge answers in text frames, not {other:?}"),
            }
        }
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

    fn started(limits: BridgeLimits) -> (BridgeServer, Arc<RecordingPolicy>) {
        let policy = RecordingPolicy::holding(TEST_TOKEN);
        let server = BridgeServer::start(0, Arc::clone(&policy) as Arc<dyn BridgePolicy>, limits)
            .expect("the operating system always has a free port");
        (server, policy)
    }

    /// S12. Security, and the one mistake in this file that would be
    /// catastrophic and invisible: an address that is one constant away from
    /// putting the user's screen on the local network. Asserted twice, because
    /// the running listener proves what this build does and the scan proves
    /// that nothing in this file even spells the other address.
    #[test]
    fn the_bridge_listens_on_loopback_and_nowhere_else() {
        let (server, _policy) = started(BridgeLimits::default());

        assert_eq!(
            server.local_addr().ip().to_string(),
            "127.0.0.1",
            "the bridge is reachable from this machine and no other"
        );

        // The two needles are spelled in halves, because the scan reads the
        // file this test is written in: written whole, each would find itself.
        let every_interface = ["0.0", ".0.0"].concat();
        let unspecified = ["UNSPEC", "IFIED"].concat();
        let source = include_str!("server.rs");
        assert!(
            !source.contains(&every_interface),
            "the bridge must never bind {every_interface}"
        );
        assert!(
            !source.contains(&unspecified),
            "nor the constant that means the same thing, {unspecified}"
        );
    }

    /// S13. Security. A handshake that does not say where it came from is not a
    /// browser extension, and gets no socket at all.
    #[test]
    fn a_handshake_without_an_origin_is_refused_with_403() {
        let (server, _policy) = started(BridgeLimits::default());

        let refused = connect(server.local_addr(), None).expect_err("a nameless caller is refused");

        assert_eq!(
            refusal_status(&refused),
            Some(403),
            "the refusal is an HTTP 403 and no websocket is opened: {refused}"
        );
    }

    /// S14. Security. The case this gate exists for: a web page's JavaScript
    /// reaching for the loopback socket. The browser fills in `Origin` for it
    /// and will not let it lie, so this is where that attempt ends.
    #[test]
    fn a_handshake_from_a_web_page_is_refused_with_403() {
        let (server, _policy) = started(BridgeLimits::default());

        let refused = connect(server.local_addr(), Some("https://evil.example"))
            .expect_err("a web page is refused");

        assert_eq!(
            refusal_status(&refused),
            Some(403),
            "a page never gets past the handshake: {refused}"
        );
    }

    fn refusal_status(error: &tungstenite::Error) -> Option<u16> {
        match error {
            tungstenite::Error::Http(response) => Some(response.status().as_u16()),
            _ => None,
        }
    }

    /// S15. Security. A frame past the limit is refused by the transport,
    /// before anything reads it into a value and long before it reaches the
    /// save path.
    #[test]
    fn a_frame_over_the_size_limit_never_reaches_the_policy() {
        let limits = BridgeLimits {
            max_message_bytes: 1024,
            ..BridgeLimits::default()
        };
        let (server, policy) = started(limits);
        let mut client = paired(server.local_addr());

        let oversized = full_page_frame("too-big", 2048);
        assert!(
            oversized.len() > 2048,
            "the frame has to be past the limit for this to prove anything: {} bytes",
            oversized.len()
        );
        // The write may or may not survive the bridge closing under it, and
        // either way the answer is in what follows.
        let _ = client.send(Message::text(oversized));

        let answer = client.read();
        assert!(
            !matches!(&answer, Ok(Message::Text(_))),
            "an oversized frame is never answered: {answer:?}"
        );
        assert!(
            policy.delivered().is_empty(),
            "and never delivered: {:?}",
            policy.delivered()
        );
    }

    /// S16. The application listens with the protocol's limit rather than one
    /// this file made up, and it is `lib.rs` that has to ask for it.
    #[test]
    fn the_application_listens_with_the_protocol_limit() {
        assert_eq!(
            BridgeLimits::default().max_message_bytes,
            67_108_864,
            "64 MiB is the contract, written out rather than read from the constant it pins"
        );
        assert_eq!(
            BridgeLimits::default().max_message_bytes,
            MAX_MESSAGE_BYTES,
            "and it is the protocol's number, not a second one that happens to agree today"
        );

        let lib = include_str!("../lib.rs");
        assert!(
            lib.contains("BridgeLimits::default()"),
            "the application has to start the bridge with the defaults, or the limit tested here is not the limit it runs"
        );
    }

    /// S17. The concurrency limit, and the half of it that is easy to get
    /// wrong: a slot has to come back when the session that took it ends, or
    /// the bridge closes itself down one connection at a time.
    #[test]
    fn a_session_slot_is_refused_while_taken_and_free_once_given_back() {
        let limits = BridgeLimits {
            max_sessions: 1,
            ..BridgeLimits::default()
        };
        let (server, policy) = started(limits);
        let mut first = paired(server.local_addr());

        let refused = connect(server.local_addr(), Some(REAL_EXTENSION_ORIGIN));
        assert!(
            refused.is_err(),
            "a second session is refused while the first holds the only slot"
        );

        first
            .send(Message::text(full_page_frame("one", 8)))
            .expect("the first session is untouched");
        let accepted = read_text(&mut first);
        assert_eq!(
            frame_type(&accepted),
            "accepted",
            "and still working: {accepted}"
        );
        assert_eq!(policy.delivered(), vec!["one".to_owned()]);

        drop(first);

        let third = wait_for_a_slot(server.local_addr());
        assert!(
            third.is_ok(),
            "the slot comes back when the session ends: {third:?}"
        );
    }

    /// Connects until the bridge has room, or gives up.
    ///
    /// The give-back happens on the session's own thread, some short time after
    /// the client's socket closes, and there is nothing to wait on from here.
    fn wait_for_a_slot(addr: SocketAddr) -> Result<WebSocket<TcpStream>, tungstenite::Error> {
        let mut last = connect(addr, Some(REAL_EXTENSION_ORIGIN));
        for _ in 0..SLOT_ATTEMPTS {
            if last.is_ok() {
                return last;
            }
            std::thread::sleep(SLOT_INTERVAL);
            last = connect(addr, Some(REAL_EXTENSION_ORIGIN));
        }
        last
    }

    /// S18. The whole path, over a real socket: the right origin, the right
    /// token, one page, one answer.
    #[test]
    fn a_paired_extension_delivers_a_page_over_the_socket() {
        let (server, policy) = started(BridgeLimits::default());
        let mut client = paired(server.local_addr());

        client
            .send(Message::text(full_page_frame("one", 8)))
            .expect("the bridge takes a page");

        let accepted = parse_frame(&read_text(&mut client));
        assert_eq!(accepted["type"], "accepted", "the page is accepted");
        assert_eq!(accepted["requestId"], "one", "against its own request");
        assert_eq!(accepted["savedPath"], SAVED_PATH, "and says where it went");
        assert_eq!(policy.delivered(), vec!["one".to_owned()]);
    }

    /// S19. A port that is already taken is the ordinary failure here, and the
    /// application has to be able to tell the user which port to go and look
    /// at. Nothing about it may take the process down: a bridge that cannot
    /// listen still leaves every other way of taking a screenshot working.
    #[test]
    fn a_taken_port_is_a_readable_failure_rather_than_a_panic() {
        let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port exists");
        let port = taken
            .local_addr()
            .expect("a bound listener has an address")
            .port();

        let failure = BridgeServer::start(
            port,
            RecordingPolicy::holding(TEST_TOKEN) as Arc<dyn BridgePolicy>,
            BridgeLimits::default(),
        )
        .expect_err("the port is held by this test");

        assert!(
            failure.contains(&port.to_string()),
            "the message has to name the port the user must free: {failure}"
        );
    }

    /// S20. A connection that opens and then says nothing is given up on. Left
    /// alone it would hold a thread for the life of the process, and anything
    /// that can open a socket could open as many as it liked.
    #[test]
    fn a_silent_connection_is_ended_by_the_bridge() {
        let limits = BridgeLimits {
            handshake_timeout: Duration::from_millis(100),
            ..BridgeLimits::default()
        };
        let (server, _policy) = started(limits);

        let mut socket = TcpStream::connect(server.local_addr()).expect("the listener is up");
        socket
            .set_read_timeout(Some(CLIENT_TIMEOUT))
            .expect("a fresh socket takes a timeout");

        let mut buffer = [0u8; 1];
        let read = socket.read(&mut buffer);

        assert!(
            matches!(read, Ok(0)),
            "the bridge ends a silent connection itself rather than waiting on it: {read:?}"
        );
    }
}
