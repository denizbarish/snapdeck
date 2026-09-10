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
use std::time::{Duration, Instant};

use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{
    header::{HOST, ORIGIN},
    StatusCode,
};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::frame::CloseFrame;
use tungstenite::protocol::{Message, WebSocket, WebSocketConfig};

use crate::bridge::protocol::{MAX_HANDSHAKE_MESSAGE_BYTES, MAX_MESSAGE_BYTES};
use crate::bridge::session::{origin_is_allowed, run_session, BridgePolicy, Frames};

/// How many sessions may be open at once.
///
/// One browser is the expected case and the limit is here for the unexpected
/// one: something on this machine opening connections in a loop must not be
/// able to spend the process's threads. Refusing the extra connection costs a
/// real user nothing, because a second Chrome profile still only needs a
/// session while a capture is in flight.
const MAX_SESSIONS: usize = 4;

/// How long a connection has to get from accepted to paired.
///
/// One deadline across the HTTP handshake and the `hello` that has to follow
/// it, not a budget per read; see `DeadlineStream`. After that the clock comes
/// off: a paired extension is allowed to sit idle between two captures for as
/// long as the user leaves the tab open.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// What is recorded when a peer runs out of handshake deadline.
const HANDSHAKE_TOO_SLOW: &str = "the opening took longer than the bridge waits for it";

/// What a handshake with the wrong `Origin` is told, as the body of the 403.
const REFUSED_ORIGIN: &str = "the bridge only answers a Snapdeck browser extension";

/// What a handshake aimed at some other name for this machine is told.
const REFUSED_HOST: &str = "the bridge only answers a request addressed to loopback";

/// What a handshake that says who it is more than once is told.
const REFUSED_HEADERS: &str = "the bridge reads one `Origin` and one `Host`, and this is not that";

/// The names loopback answers to, and the whole of what a `Host` may say.
///
/// The address itself and the name that resolves to it. Anything else is a
/// request that reached this socket while addressed somewhere else, which is
/// what a rebinding attack looks like from in here.
const LOOPBACK_HOSTS: [&str; 2] = ["127.0.0.1", "localhost"];

/// How many control frames a caller may send before it has proved anything.
///
/// A Ping is answered by tungstenite and never reaches the session, so before
/// this cap existed a peer could hold one of the four slots by sending nothing
/// else at all: the frames are well formed, so the handshake deadline is the
/// only thing that ends them, and it ends them one slot at a time. A browser's
/// `WebSocket` sends no control frames of its own before `hello`, so a real
/// extension never comes near this. The cap comes off with the token.
const MAX_CONTROL_FRAMES_BEFORE_AUTH: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct BridgeLimits {
    pub max_message_bytes: usize,
    /// What a message may weigh while the caller is still nobody.
    pub handshake_message_bytes: usize,
    /// How many Pings and Pongs a caller may send before the token is in.
    pub max_control_frames_before_auth: usize,
    pub max_sessions: usize,
    pub handshake_timeout: Duration,
}

impl Default for BridgeLimits {
    /// The protocol's own numbers. The application never builds this by hand.
    fn default() -> Self {
        Self {
            max_message_bytes: MAX_MESSAGE_BYTES,
            handshake_message_bytes: MAX_HANDSHAKE_MESSAGE_BYTES,
            max_control_frames_before_auth: MAX_CONTROL_FRAMES_BEFORE_AUTH,
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
            let port = local_addr.port();
            std::thread::spawn(move || {
                accept_loop(listener, policy, limits, port, &stopping, &sessions);
            })
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
    port: u16,
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
            serve(stream, policy.as_ref(), limits, port);
        });
    }
}

/// Runs the handshake gate and then one session over the accepted socket.
fn serve(stream: TcpStream, policy: &dyn BridgePolicy, limits: BridgeLimits, port: u16) {
    // One deadline for the whole opening, taken before anything is read. See
    // `DeadlineStream` for why this is not a timeout on each read.
    let stream = DeadlineStream::until(stream, Instant::now() + limits.handshake_timeout);

    // The opening budget, not the session's. Until the token is checked the
    // caller is nobody, and nobody may make this side hold 64 MiB of their
    // choosing; every field of a `hello` is short by schema, so a few KiB is
    // room to spare. `SocketFrames::authenticated` lifts it once the token is
    // in. Never above the session's own limit, because the opening is part of
    // the session and a budget larger than the whole is not a budget.
    let opening = limits.handshake_message_bytes.min(limits.max_message_bytes);

    // Both limits, because a message arrives as frames: capping the message
    // alone would still let a single oversized frame be read into memory first.
    let config = WebSocketConfig::default()
        .max_message_size(Some(opening))
        .max_frame_size(Some(opening));

    // A closure rather than a bare function, because the gate has to know which
    // port this listener actually took before it can judge a `Host`.
    #[expect(
        clippy::result_large_err,
        reason = "the closure's `Err` is `handshake_gate`'s, and that signature is tungstenite's \
                  `Callback`. See the same expectation on the function itself."
    )]
    let gate = |request: &Request, response: Response| handshake_gate(port, request, response);
    let Ok(socket) = tungstenite::accept_hdr_with_config(stream, gate, Some(config)) else {
        // Either a gate refused, in which case the 403 has already been
        // written, or the peer went away mid-handshake. Neither is something
        // this side can do anything further about.
        return;
    };

    let mut frames = SocketFrames {
        socket,
        past_first_frame: false,
        control_frames_left: Some(limits.max_control_frames_before_auth),
        max_message_bytes: limits.max_message_bytes,
    };
    run_session(&mut frames, policy);
}

/// The socket a session is opened over, with one deadline across everything
/// read before that session is under way.
///
/// A timeout per read is not a bound on the opening. A peer that sends a byte
/// at a time is never silent long enough to trip such a timeout and never
/// finishes either, so it restarts the clock with every byte and holds a thread
/// for as long as it cares to keep dripping. That is the classic slowloris, and
/// the only thing that ends it is a clock that started once. `max_sessions`
/// bounds how many threads can be held this way, which makes it survivable
/// rather than acceptable: four connections should not be able to close the
/// bridge for the rest of the run.
///
/// The deadline covers the HTTP handshake and the `hello` that has to follow
/// it, and then comes off. A paired extension is allowed to sit idle between
/// two captures for as long as the user leaves the tab open.
struct DeadlineStream {
    inner: TcpStream,
    /// `None` once the session is under way and the peer may go quiet.
    deadline: Option<Instant>,
}

impl DeadlineStream {
    fn until(inner: TcpStream, deadline: Instant) -> Self {
        Self {
            inner,
            deadline: Some(deadline),
        }
    }

    /// Takes the clock off, for a session that has said who it is.
    fn open_ended(&mut self) {
        if self.deadline.take().is_some() {
            let _ = self.inner.set_read_timeout(None);
        }
    }
}

impl std::io::Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if let Some(deadline) = self.deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            // A zero timeout means "block forever" to the socket, which is the
            // opposite of what an expired deadline asks for.
            if left.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    HANDSHAKE_TOO_SLOW,
                ));
            }
            self.inner.set_read_timeout(Some(left))?;
        }
        self.inner.read(buffer)
    }
}

impl std::io::Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Whether a handshake's `Host` was addressed to this listener.
///
/// `Origin` is what keeps a web page off the socket; this is the second lock on
/// the same door. A page served from a name that resolves to `127.0.0.1` is
/// same-origin with nothing here, but the request it sends carries that name in
/// `Host`, and a bridge that never looked would answer it. Today `Origin` alone
/// would refuse such a page. Two gates rather than one, because rebinding is
/// the attack that gets past exactly one of them.
///
/// The port is the one this listener actually took rather than the protocol's,
/// so a request aimed at some other Snapdeck-shaped port is refused too, and so
/// the tests can prove the rule on the ephemeral port they run on.
fn host_is_allowed(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };
    LOOPBACK_HOSTS
        .iter()
        .any(|name| host == format!("{name}:{port}"))
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
fn handshake_gate(
    port: u16,
    request: &Request,
    response: Response,
) -> Result<Response, ErrorResponse> {
    let headers = request.headers();

    // Exactly one of each, before either is read. `HeaderMap::get` answers the
    // first of a repeated header, so a request carrying two `Origin`s would be
    // judged on one of them and could be read elsewhere as the other. A browser
    // sends one; nothing that sends two is asking a question worth answering.
    if headers.get_all(ORIGIN).iter().count() != 1 || headers.get_all(HOST).iter().count() != 1 {
        return Err(refusal(REFUSED_HEADERS));
    }

    let origin = headers.get(ORIGIN).and_then(|value| value.to_str().ok());
    if !origin_is_allowed(origin) {
        return Err(refusal(REFUSED_ORIGIN));
    }

    let host = headers.get(HOST).and_then(|value| value.to_str().ok());
    if !host_is_allowed(host, port) {
        return Err(refusal(REFUSED_HOST));
    }

    Ok(response)
}

/// The 403 a refused handshake is answered with, and the sentence saying why.
fn refusal(reason: &str) -> ErrorResponse {
    let mut refusal = ErrorResponse::new(Some(reason.to_owned()));
    *refusal.status_mut() = StatusCode::FORBIDDEN;
    refusal
}

/// The session state machine's view of a real socket.
struct SocketFrames {
    socket: WebSocket<DeadlineStream>,
    past_first_frame: bool,
    /// How many control frames are left before the token is in, and `None`
    /// once it is: a paired extension may ping for as long as it likes.
    control_frames_left: Option<usize>,
    /// What a message may weigh once the caller has proved who it is.
    max_message_bytes: usize,
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
                        self.socket.get_mut().open_ended();
                    }
                    return Ok(text.to_string());
                }
                // Answered by tungstenite itself; neither is a bridge frame.
                // Counted all the same until the token is in, because a peer
                // that sends nothing but these is never silent, never finished,
                // and holding a slot the whole time.
                Message::Ping(_) | Message::Pong(_) => {
                    if let Some(left) = self.control_frames_left.as_mut() {
                        if *left == 0 {
                            return Err(
                                "the opening carried more control frames than the bridge answers before a token"
                                    .to_owned(),
                            );
                        }
                        *left -= 1;
                    }
                    continue;
                }
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

    /// Lifts what the socket was holding an unproven caller to.
    ///
    /// Both limits are read per frame rather than captured when the socket was
    /// built, so raising them here applies to everything read after this point
    /// and to nothing read before it. The write buffer settings are left
    /// exactly as they were, which is what keeps `set_config`'s own assertion
    /// satisfied.
    fn authenticated(&mut self) {
        self.control_frames_left = None;

        let full = self.max_message_bytes;
        self.socket.set_config(move |config| {
            config.max_message_size = Some(full);
            config.max_frame_size = Some(full);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::Mutex as StdMutex;

    use tungstenite::client::IntoClientRequest;
    use tungstenite::http::HeaderValue;

    use crate::bridge::protocol::{AppInfo, FullPage, PROTOCOL_VERSION};
    use crate::bridge::session::proof_for;

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

    /// Sixteen bytes of hex, which is what a `hello` has to carry.
    const TEST_NONCE: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";

    fn hello_frame(token: &str) -> String {
        format!(
            r#"{{"type":"hello","protocolVersion":{PROTOCOL_VERSION},"token":"{token}","nonce":"{TEST_NONCE}","client":{{"name":"snapdeck-extension","version":"0.1.0"}}}}"#
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
        connect_with(addr, origin, None, &[])
    }

    /// The same, with `host` replacing the one the uri implies and `extra`
    /// appended rather than replacing: appending is how a test says a header
    /// twice, which is the case `HeaderMap::get` would quietly pick one of.
    fn connect_with(
        addr: SocketAddr,
        origin: Option<&str>,
        host: Option<&str>,
        extra: &[(tungstenite::http::HeaderName, String)],
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
        if let Some(host) = host {
            request.headers_mut().insert(
                HOST,
                HeaderValue::from_str(host).expect("a test host is a header value"),
            );
        }
        for (name, value) in extra {
            request.headers_mut().append(
                name.clone(),
                HeaderValue::from_str(value).expect("a test header is a header value"),
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
        assert_eq!(
            parse_frame(&ready)["proof"],
            proof_for(TEST_TOKEN, TEST_NONCE),
            "and the answer proves it holds the token: {ready}"
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

    /// S21. Security. The other half of S20, and the one a timeout per read
    /// does not cover: a peer that is never silent for long enough to trip the
    /// clock, but never finishes either. A byte every so often restarts a
    /// per-read timeout forever, so the bound has to be on the handshake as a
    /// whole rather than on any one read of it.
    #[test]
    fn a_connection_that_drips_bytes_is_ended_at_the_handshake_deadline() {
        let limits = BridgeLimits {
            handshake_timeout: DRIP_DEADLINE,
            ..BridgeLimits::default()
        };
        let (server, _policy) = started(limits);

        let mut socket = TcpStream::connect(server.local_addr()).expect("the listener is up");
        socket
            .set_read_timeout(Some(DRIP_POLL))
            .expect("a fresh socket takes a timeout");

        // The opening of a handshake that is never finished, fed one byte at a
        // time. Every byte is well inside the deadline on its own.
        let request = b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\n";
        let started_at = Instant::now();
        let mut ended_after = None;
        for byte in request.iter().take(DRIP_COUNT) {
            // The write fails once the bridge has hung up, which is the outcome
            // this test is waiting for rather than a problem with it.
            let _ = socket.write_all(&[*byte]);
            std::thread::sleep(DRIP_INTERVAL);
            let mut buffer = [0u8; 1];
            if matches!(socket.read(&mut buffer), Ok(0)) {
                ended_after = Some(started_at.elapsed());
                break;
            }
        }

        let ended_after = ended_after.expect(
            "a peer that keeps dripping must not be able to hold a session thread for as long as it likes",
        );
        assert!(
            ended_after < DRIP_DEADLINE * DEADLINE_SLACK,
            "and it has to end at the deadline rather than whenever the peer stops: {ended_after:?}"
        );
    }

    /// S22. Security, LOW-1. `Origin` is what keeps a web page off this socket,
    /// and it is not allowed to be the only lock on the door: a request that
    /// reached loopback while addressed to some other name is what a rebinding
    /// attack looks like from in here, and `Host` is where it says so.
    ///
    /// Pure, so the rule can be read whole rather than inferred from a handful
    /// of live handshakes.
    #[test]
    fn only_a_host_naming_this_listener_is_allowed() {
        assert!(host_is_allowed(Some("127.0.0.1:51837"), 51_837));
        assert!(
            host_is_allowed(Some("localhost:51837"), 51_837),
            "the name loopback answers to, as a browser may write it"
        );

        for host in [
            None,
            Some(""),
            Some("evil.example:51837"),
            // A name an attacker points at 127.0.0.1. The connection arrives
            // here; the header is what gives it away.
            Some("snapdeck.attacker.example:51837"),
            // The right name and somebody else's port.
            Some("127.0.0.1:51838"),
            // The right name and no port at all.
            Some("127.0.0.1"),
            Some("localhost"),
        ] {
            assert!(
                !host_is_allowed(host, 51_837),
                "the bridge is not addressed as {host:?}"
            );
        }
    }

    /// S23. Security, LOW-1, over a real socket. A handshake with the right
    /// `Origin` and a `Host` naming somewhere else never gets a WebSocket.
    #[test]
    fn a_handshake_addressed_to_another_name_is_refused_with_403() {
        let (server, _policy) = started(BridgeLimits::default());

        let refused = connect_with(
            server.local_addr(),
            Some(REAL_EXTENSION_ORIGIN),
            Some("snapdeck.attacker.example:51837"),
            &[],
        )
        .expect_err("a request addressed elsewhere is refused");

        assert_eq!(
            refusal_status(&refused),
            Some(403),
            "the second lock holds even when the first one opens: {refused}"
        );
    }

    /// S24. Security. A hardening note rather than a live hole: a browser sends
    /// one `Origin`, and `HeaderMap::get` answers the first of a repeated
    /// header. A request judged on the first and read anywhere else as the
    /// second is a disagreement this side refuses rather than resolves.
    #[test]
    fn a_handshake_that_says_where_it_came_from_twice_is_refused() {
        let (server, _policy) = started(BridgeLimits::default());

        let refused = connect_with(
            server.local_addr(),
            Some(REAL_EXTENSION_ORIGIN),
            None,
            &[(ORIGIN, "https://evil.example".to_owned())],
        )
        .expect_err("two origins are not one origin");

        assert_eq!(
            refusal_status(&refused),
            Some(403),
            "a doubled `Origin` is refused whichever of the two would have passed: {refused}"
        );

        // And the other way round, so the rule is not "the first one wins".
        let also_refused = connect_with(
            server.local_addr(),
            Some("https://evil.example"),
            None,
            &[(ORIGIN, REAL_EXTENSION_ORIGIN.to_owned())],
        )
        .expect_err("nor in the other order");
        assert_eq!(refusal_status(&also_refused), Some(403));
    }

    /// S25. Security, MEDIUM-1. A peer that sends nothing but Pings is never
    /// silent and never finished, and before this cap it could hold a session
    /// slot until the handshake deadline for free, four at a time.
    ///
    /// tungstenite answers each Ping itself, so these frames never reach the
    /// session: the count is the only thing that ends them.
    #[test]
    fn a_flood_of_control_frames_before_the_token_ends_the_session() {
        let allowed = 3;
        let limits = BridgeLimits {
            max_control_frames_before_auth: allowed,
            ..BridgeLimits::default()
        };
        let (server, policy) = started(limits);

        let mut client = connect(server.local_addr(), Some(REAL_EXTENSION_ORIGIN))
            .expect("the handshake passes");
        for _ in 0..=allowed {
            // The last of these may fail to write, because the bridge is
            // hanging up underneath it. That is the outcome, not a problem.
            let _ = client.send(Message::Ping(Vec::new().into()));
        }
        let _ = client.send(Message::text(hello_frame(TEST_TOKEN)));

        let mut answered = Vec::new();
        while let Ok(message) = client.read() {
            if let Message::Text(text) = message {
                answered.push(text.to_string());
            }
        }

        assert!(
            answered.is_empty(),
            "a caller past the control frame budget is answered nothing at all: {answered:?}"
        );
        assert!(
            policy.delivered().is_empty(),
            "and delivers nothing: {:?}",
            policy.delivered()
        );
    }

    /// S26. The other half of S25: a real extension pings freely once it holds
    /// a session, because the budget is about callers who have proved nothing.
    #[test]
    fn a_paired_session_may_send_more_control_frames_than_the_budget() {
        let allowed = 2;
        let limits = BridgeLimits {
            max_control_frames_before_auth: allowed,
            ..BridgeLimits::default()
        };
        let (server, policy) = started(limits);
        let mut client = paired(server.local_addr());

        for _ in 0..(allowed * 4) {
            client
                .send(Message::Ping(Vec::new().into()))
                .expect("a paired session takes a ping");
        }
        client
            .send(Message::text(full_page_frame("one", 8)))
            .expect("and still takes a page");

        assert_eq!(
            frame_type(&read_text(&mut client)),
            "accepted",
            "the session is untouched by pings it was allowed to send"
        );
        assert_eq!(policy.delivered(), vec!["one".to_owned()]);
    }

    /// S27. Security, MEDIUM-2. Before the token, the socket holds a few KiB
    /// and no more: a caller that has proved nothing must not be able to make
    /// this side buffer 64 MiB of its choosing.
    #[test]
    fn an_oversized_frame_before_the_token_is_refused() {
        let opening = 512;
        let limits = BridgeLimits {
            handshake_message_bytes: opening,
            ..BridgeLimits::default()
        };
        let (server, policy) = started(limits);

        let mut client = connect(server.local_addr(), Some(REAL_EXTENSION_ORIGIN))
            .expect("the handshake passes");
        // Well past the opening budget and far inside the session's own limit,
        // so only the first of the two limits can refuse it.
        let _ = client.send(Message::text("a".repeat(opening * 4)));

        let answer = client.read();
        assert!(
            !matches!(&answer, Ok(Message::Text(_))),
            "an unproven caller's oversized frame is never answered: {answer:?}"
        );
        assert!(
            policy.delivered().is_empty(),
            "and never delivered: {:?}",
            policy.delivered()
        );
    }

    /// S28. Security, MEDIUM-2, and the half that makes it a two-stage limit
    /// rather than a smaller one: a page is far past the opening budget, and a
    /// paired extension has to be able to send it.
    #[test]
    fn a_page_far_past_the_opening_budget_is_carried_once_the_token_is_in() {
        let opening = 512;
        let limits = BridgeLimits {
            handshake_message_bytes: opening,
            ..BridgeLimits::default()
        };
        let (server, policy) = started(limits);
        let mut client = paired(server.local_addr());

        let page = full_page_frame("one", opening * 8);
        assert!(
            page.len() > opening,
            "the page has to be past the opening budget for this to prove anything: {} bytes",
            page.len()
        );
        client
            .send(Message::text(page))
            .expect("the bridge takes it");

        assert_eq!(
            frame_type(&read_text(&mut client)),
            "accepted",
            "the limit comes off with the token, not before and not never"
        );
        assert_eq!(policy.delivered(), vec!["one".to_owned()]);
    }

    /// S29. The application's own numbers, including the two this change added.
    /// Written out rather than read from the constants they pin, for the reason
    /// S16 gives.
    #[test]
    fn the_application_holds_an_unproven_caller_to_a_few_kilobytes() {
        assert_eq!(
            BridgeLimits::default().handshake_message_bytes,
            4096,
            "4 KiB is the contract for a caller that has proved nothing"
        );
        assert_eq!(
            BridgeLimits::default().handshake_message_bytes,
            MAX_HANDSHAKE_MESSAGE_BYTES,
            "and it is the protocol's number, not a second one that happens to agree"
        );
        assert!(
            BridgeLimits::default().handshake_message_bytes
                < BridgeLimits::default().max_message_bytes,
            "the opening budget is the smaller of the two, or there is only one limit"
        );
        assert_eq!(
            BridgeLimits::default().max_control_frames_before_auth,
            8,
            "and eight control frames is what an unproven caller gets"
        );
    }

    /// The handshake budget the dripping test gives the bridge.
    const DRIP_DEADLINE: Duration = Duration::from_millis(150);

    /// How often the dripping peer sends a byte. Shorter than the deadline, so
    /// a timeout that restarts with every read never fires.
    const DRIP_INTERVAL: Duration = Duration::from_millis(60);

    /// How many bytes it drips before giving up, which is `DRIP_COUNT` times
    /// `DRIP_INTERVAL` of holding the thread: far past the deadline.
    const DRIP_COUNT: usize = 20;

    /// How long the dripping peer waits for the bridge to hang up between
    /// bytes.
    const DRIP_POLL: Duration = Duration::from_millis(20);

    /// How far past the deadline a loaded machine is allowed to be.
    const DEADLINE_SLACK: u32 = 4;
}
