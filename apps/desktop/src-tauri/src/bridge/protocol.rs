//! The serde mirror of `packages/protocol`, and the only place a bridge frame
//! is turned into a value.
//!
//! The contract lives once, in TypeScript, because the extension imports it and
//! a schema that is written twice is a schema that drifts. What is here is the
//! other half of the same sentence, held to it by two tests that read those
//! files: `the_shared_limits_match_the_typescript_ones` and
//! `every_error_code_is_in_the_typescript_enum`.
//!
//! Every message is its own struct with `deny_unknown_fields`, rather than one
//! enum tagged with `#[serde(tag = "type")]`. Serde's internal tagging cannot
//! carry `deny_unknown_fields`, and unknown-field strictness is half of what
//! separates this from a shape check: a field nobody declared means the two
//! sides are saying different things, and at a trust boundary that is a
//! failure, not something to drop quietly. The `type` field is carried by a
//! one-variant tag type, so the tag is a declared field like any other and
//! `deny_unknown_fields` has nothing left over to complain about. This is why
//! `packages/protocol` uses `z.strictObject` and never `z.object`.

use serde::{Deserialize, Serialize};

/// Bumped whenever a field is added, removed or given a new meaning.
pub const PROTOCOL_VERSION: u32 = 1;

/// Fixed loopback port, inside the IANA dynamic range so it steals nothing.
pub const BRIDGE_PORT: u16 = 51837;

/// 40 MiB. The largest PNG the bridge will carry.
pub const MAX_PNG_BYTES: usize = 41_943_040;

/// 64 MiB. What `MAX_PNG_BYTES` costs once base64 encoded and wrapped in JSON.
pub const MAX_MESSAGE_BYTES: usize = 67_108_864;

/// Below Chrome's maximum desktop canvas area, so the extension can build it.
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;

/// The only `Origin` the handshake accepts.
pub const EXTENSION_ORIGIN_PREFIX: &str = "chrome-extension://";

/// WebSocket close code used whenever one of the gates shuts the connection.
pub const CLOSE_POLICY_VIOLATION: u16 = 1008;

/// Hex characters in a `hello` nonce: 16 bytes of the extension's randomness.
///
/// Fixed rather than bounded. A nonce shorter than this is not a nonce, and one
/// longer is a different protocol; either way it is not something to negotiate
/// with whoever is on the other end of a socket that has proved nothing yet.
pub const NONCE_HEX_LENGTH: usize = 32;

/// Hex characters in a `ready` proof: the 32 bytes of an HMAC-SHA256 digest.
pub const PROOF_HEX_LENGTH: usize = 64;

/// 4 KiB. What the socket accepts while it is still waiting for `hello`.
///
/// Every field of a `hello` is short by schema, so a real handshake has no
/// reason to weigh more, and until the token is checked the peer is nobody: a
/// caller that has proved nothing must not be able to make this side hold
/// `MAX_MESSAGE_BYTES` of its choosing. The full limit comes on afterwards.
pub const MAX_HANDSHAKE_MESSAGE_BYTES: usize = 4096;

/// The caps every string on the wire is held to, mirrored from
/// `packages/protocol/src/limits.ts` and measured the way that side measures.
pub const MAX_TOKEN_CHARS: usize = 256;
pub const MAX_CLIENT_NAME_CHARS: usize = 64;
pub const MAX_CLIENT_VERSION_CHARS: usize = 32;
pub const MAX_REQUEST_ID_CHARS: usize = 64;
pub const MAX_URL_CHARS: usize = 2048;
pub const MAX_TITLE_CHARS: usize = 1024;

/// What a failure this side reports may say, before it is cut to fit.
pub const MAX_ERROR_MESSAGE_CHARS: usize = 1024;

/// The largest device pixel ratio a real display reports, with room over it.
///
/// A ratio is what the extension multiplies the page by, so an unbounded one is
/// a way of asking for an unbounded canvas.
pub const MAX_DEVICE_PIXEL_RATIO: u32 = 8;

/// The longest `pngBase64` a message may carry: four characters for every three
/// bytes of `MAX_PNG_BYTES`, rounded up to the padded group.
///
/// Derived rather than written down, on both sides, so that the length of the
/// text and the weight of the picture it decodes to cannot drift apart.
pub const MAX_PNG_BASE64_CHARS: usize = MAX_PNG_BYTES.div_ceil(3) * 4;

/// The `type` a handshake frame carries.
const HELLO_TYPE: &str = "hello";

/// The `type` a capture frame carries.
const FULL_PAGE_TYPE: &str = "fullPage";

/// What `encode` says when serde cannot render a response.
///
/// Unreachable as the type stands: every field is a string, a `u32` or a unit
/// variant, and `serde_json` only fails on a map with non-string keys or a
/// float that is not finite. It is here so that the impossible costs the
/// extension one useless frame rather than taking the connection down with an
/// `expect`.
const UNRENDERABLE_RESPONSE: &str = r#"{"type":"error","requestId":null,"code":"saveFailed","message":"the response could not be rendered"}"#;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelloTag {
    Hello,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FullPageTag {
    FullPage,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageInfo {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImagePayload {
    pub png_base64: String,
    pub width: u32,
    pub height: u32,
    pub device_pixel_ratio: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Hello {
    pub r#type: HelloTag,
    pub protocol_version: u32,
    pub token: String,
    /// The extension's challenge to this side, fresh for every session.
    ///
    /// Without it the handshake only runs one way: a program that took the port
    /// before Snapdeck did could answer `ready` out of thin air, because that
    /// frame carries nothing only the real bridge could have written, and then
    /// collect every page the user captured. The nonce is what the answer is
    /// about, and a new one per session is what keeps a proof from being
    /// replayed by whoever watched the last one go past.
    pub nonce: String,
    pub client: ClientInfo,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FullPage {
    pub r#type: FullPageTag,
    pub request_id: String,
    pub page: PageInfo,
    pub image: ImagePayload,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Hello(Hello),
    FullPage(FullPage),
}

impl ClientMessage {
    /// Parses one text frame. The only place a bridge frame is deserialised.
    ///
    /// The `type` field is read once, from a `Value`, and decides which struct
    /// the rest is parsed into. Reading it first is what makes an unrecognised
    /// message its own answer: trying each struct in turn would report the last
    /// schema failure instead, and a newer extension would be told its fields
    /// were wrong rather than that this build does not know the message.
    pub fn parse(raw: &str) -> Result<Self, BridgeError> {
        let frame: serde_json::Value =
            serde_json::from_str(raw).map_err(|err| BridgeError::NotJson(err.to_string()))?;

        let Some(tag) = frame
            .get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            return Err(BridgeError::Schema(
                "the frame carries no `type` field".to_owned(),
            ));
        };

        match tag.as_str() {
            HELLO_TYPE => {
                let hello: Hello = serde_json::from_value(frame)
                    .map_err(|err| BridgeError::Schema(err.to_string()))?;
                hello.check()?;
                Ok(Self::Hello(hello))
            }
            FULL_PAGE_TYPE => {
                let page: FullPage = serde_json::from_value(frame)
                    .map_err(|err| BridgeError::Schema(err.to_string()))?;
                page.check()?;
                Ok(Self::FullPage(page))
            }
            _ => Err(BridgeError::UnknownType(tag)),
        }
    }
}

/// The length `packages/protocol` measures a string by: UTF-16 code units,
/// which is what `String.prototype.length` counts in the extension.
///
/// Measured the same way here so that the two schemas accept and refuse exactly
/// the same strings rather than nearly the same ones. Bytes would refuse a
/// legitimate title of a thousand Turkish characters; scalar values would
/// accept half again as much emoji as the extension will.
fn utf16_length(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Holds one string to the size the schema declares for it.
///
/// serde reads the shape and this reads the size, because a field of the right
/// type and the wrong size is the same kind of disagreement as a missing one:
/// the two sides are saying different things, and at a trust boundary that is a
/// failure rather than something to clamp quietly. A limit declared in
/// `limits.ts` and enforced nowhere is a comment.
fn bounded(name: &str, text: &str, min: usize, max: usize) -> Result<(), BridgeError> {
    let length = utf16_length(text);
    if length < min || length > max {
        return Err(BridgeError::Schema(format!(
            "`{name}` is {length} characters long, outside the {min} to {max} this protocol accepts"
        )));
    }
    Ok(())
}

/// Whether `character` may appear in a `requestId`.
///
/// The alphabet of `crypto.randomUUID`, which is what the extension builds one
/// from, with room around it. An id comes back in answers and goes into log
/// lines, so it is held to an alphabet rather than only to a length: a quote or
/// a newline inside one is a way of writing into somewhere it was only ever
/// meant to be quoted.
fn is_request_id_character(character: u8) -> bool {
    character.is_ascii_alphanumeric() || character == b'-' || character == b'_'
}

impl Hello {
    /// The sizes the schema declares, checked before anything acts on them.
    fn check(&self) -> Result<(), BridgeError> {
        bounded("token", &self.token, 1, MAX_TOKEN_CHARS)?;
        bounded("client.name", &self.client.name, 1, MAX_CLIENT_NAME_CHARS)?;
        bounded(
            "client.version",
            &self.client.version,
            1,
            MAX_CLIENT_VERSION_CHARS,
        )?;

        if self.nonce.len() != NONCE_HEX_LENGTH
            || !self.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            // The value is not echoed back. It came off a socket that has
            // proved nothing yet, and its length is the whole of what the
            // extension needs to fix its side.
            return Err(BridgeError::Schema(format!(
                "`nonce` has to be {NONCE_HEX_LENGTH} hex characters, and this one is {} characters of something else",
                utf16_length(&self.nonce)
            )));
        }
        Ok(())
    }
}

impl FullPage {
    /// The sizes the schema declares, checked before anything acts on them.
    fn check(&self) -> Result<(), BridgeError> {
        bounded("requestId", &self.request_id, 1, MAX_REQUEST_ID_CHARS)?;
        if !self.request_id.bytes().all(is_request_id_character) {
            return Err(BridgeError::Schema(
                "`requestId` is built from letters, digits, `-` and `_`".to_owned(),
            ));
        }
        bounded("page.url", &self.page.url, 1, MAX_URL_CHARS)?;
        bounded("page.title", &self.page.title, 0, MAX_TITLE_CHARS)?;
        self.image.check()
    }
}

impl ImagePayload {
    /// The sizes the schema declares, checked before anything decodes a byte.
    ///
    /// The decoder checks these numbers again, against the picture that
    /// actually arrived. This is the earlier half of the same claim: it refuses
    /// what the message says about itself, which is cheaper than refusing what
    /// it turned out to contain, and it is where `MAX_PNG_BYTES` and
    /// `MAX_IMAGE_PIXELS` are declared to apply.
    fn check(&self) -> Result<(), BridgeError> {
        bounded("image.pngBase64", &self.png_base64, 1, MAX_PNG_BASE64_CHARS)?;

        if self.width == 0 || self.height == 0 {
            return Err(BridgeError::Schema(format!(
                "an image is {} by {} pixels, and a page with no area is not a page",
                self.width, self.height
            )));
        }
        let pixels = u64::from(self.width) * u64::from(self.height);
        if pixels > MAX_IMAGE_PIXELS {
            return Err(BridgeError::Schema(format!(
                "the message claims {pixels} pixels, past the {MAX_IMAGE_PIXELS} this bridge accepts"
            )));
        }

        // Widened rather than compared as it arrived: a ratio that came in as a
        // number JSON could hold and `f32` could not is `inf` by the time it is
        // read here, and `inf` passes every comparison written the other way.
        let ratio = f64::from(self.device_pixel_ratio);
        if !ratio.is_finite() || ratio <= 0.0 || ratio > f64::from(MAX_DEVICE_PIXEL_RATIO) {
            return Err(BridgeError::Schema(format!(
                "a device pixel ratio of {ratio} is outside the 0 to {MAX_DEVICE_PIXEL_RATIO} a display reports"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    UnsupportedProtocolVersion,
    Unauthorized,
    MalformedMessage,
    SaveFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerMessage {
    Ready {
        protocol_version: u32,
        app: AppInfo,
        /// What makes this side's answer a proof rather than a sentence anybody
        /// could have written. See `session::proof_for`.
        proof: String,
    },
    Accepted {
        request_id: String,
        saved_path: Option<String>,
    },
    Error {
        request_id: Option<String>,
        code: ErrorCode,
        message: String,
    },
}

impl ServerMessage {
    /// An `error` frame whose text fits what the extension's schema accepts.
    ///
    /// `message` is the one field this side sends that can be as long as the
    /// frame that caused it: a serde failure names the field it tripped on, and
    /// that name came off the wire. Cut where the frame is built, so an
    /// oversized failure is reported rather than turning into a second failure
    /// at the far end, and cut by UTF-16 code units because that is the length
    /// `packages/protocol` measures it by.
    pub fn error(request_id: Option<String>, code: ErrorCode, message: &str) -> Self {
        let mut fitted = String::new();
        let mut units = 0usize;
        for character in message.chars() {
            units += character.len_utf16();
            if units > MAX_ERROR_MESSAGE_CHARS {
                break;
            }
            fitted.push(character);
        }
        Self::Error {
            request_id,
            code,
            message: fitted,
        }
    }

    /// The frame this response goes out as.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| UNRENDERABLE_RESPONSE.to_owned())
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum BridgeError {
    #[error("the frame is not valid JSON: {0}")]
    NotJson(String),
    #[error("the frame does not match the protocol schema: {0}")]
    Schema(String),
    #[error("unknown message type: {0}")]
    UnknownType(String),
    #[error("the extension speaks protocol version {presented}, which this build does not")]
    VersionMismatch { presented: u32 },
    #[error("the pairing token does not match")]
    Unauthorized,
}

impl BridgeError {
    /// The code this failure is reported to the extension under.
    ///
    /// Three different ways of being malformed arrive under one code on
    /// purpose: the extension cannot do anything different about a frame that
    /// was not JSON and one that was JSON of the wrong shape, and the sentence
    /// that tells them apart travels in `message`.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotJson(_) | Self::Schema(_) | Self::UnknownType(_) => {
                ErrorCode::MalformedMessage
            }
            Self::VersionMismatch { .. } => ErrorCode::UnsupportedProtocolVersion,
            Self::Unauthorized => ErrorCode::Unauthorized,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sixteen bytes of hex, which is what a `hello` has to carry.
    const NONCE: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";

    /// A `hello` frame with every field the schema asks for and nothing else.
    const WELL_FORMED_HELLO: &str = r#"{"type":"hello","protocolVersion":1,"token":"1a2b","nonce":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","client":{"name":"Snapdeck","version":"0.1.0"}}"#;

    /// A `hello` frame carrying `nonce`, and otherwise well formed.
    fn hello_with_nonce(nonce: &str) -> String {
        format!(
            r#"{{"type":"hello","protocolVersion":1,"token":"1a2b","nonce":"{nonce}","client":{{"name":"Snapdeck","version":"0.1.0"}}}}"#
        )
    }

    /// A `fullPage` frame with one field replaced, written into the JSON as it
    /// stands so a test can put anything at all there.
    fn full_page_with(field: &str, value: &str) -> String {
        full_page_replacing(&[(field, value)])
    }

    /// The same, for the claims that are about two fields at once.
    fn full_page_replacing(overrides: &[(&str, &str)]) -> String {
        let mut fields = vec![
            ("requestId", r#""req-1""#.to_owned()),
            ("url", r#""https://example.com/""#.to_owned()),
            ("title", r#""Example""#.to_owned()),
            ("pngBase64", r#""iVBORw0KGgo=""#.to_owned()),
            ("width", "1280".to_owned()),
            ("height", "4200".to_owned()),
            ("devicePixelRatio", "2".to_owned()),
        ];
        for entry in &mut fields {
            if let Some((_, value)) = overrides.iter().find(|(name, _)| *name == entry.0) {
                entry.1 = (*value).to_owned();
            }
        }
        let named = |name: &str| {
            fields
                .iter()
                .find(|entry| entry.0 == name)
                .map(|entry| entry.1.clone())
                .unwrap_or_default()
        };
        format!(
            r#"{{"type":"fullPage","requestId":{},"page":{{"url":{},"title":{}}},"image":{{"pngBase64":{},"width":{},"height":{},"devicePixelRatio":{}}},"truncated":false}}"#,
            named("requestId"),
            named("url"),
            named("title"),
            named("pngBase64"),
            named("width"),
            named("height"),
            named("devicePixelRatio"),
        )
    }

    /// B4. The happy path, and the only place the tag type is read back.
    #[test]
    fn a_hello_frame_parses_as_hello() {
        let message = ClientMessage::parse(WELL_FORMED_HELLO).expect("a well formed hello");
        let ClientMessage::Hello(hello) = message else {
            panic!("a hello frame has to come back as `Hello`, not {message:?}");
        };
        assert_eq!(hello.r#type, HelloTag::Hello);
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        assert_eq!(hello.token, "1a2b");
        assert_eq!(hello.nonce, NONCE);
        assert_eq!(hello.client.name, "Snapdeck");
        assert_eq!(hello.client.version, "0.1.0");
    }

    /// B5. The tag and the body have to be the same message. A frame that
    /// announces itself as a capture and then carries a handshake is refused
    /// rather than read as whichever of the two happens to fit.
    #[test]
    fn a_full_page_tag_over_a_hello_body_is_a_schema_error() {
        let raw = r#"{"type":"fullPage","protocolVersion":1,"token":"1a2b","client":{"name":"Snapdeck","version":"0.1.0"}}"#;
        let error = ClientMessage::parse(raw).expect_err("the body is not a capture");
        assert!(
            matches!(error, BridgeError::Schema(_)),
            "the tag and the body disagree, which is a schema failure: {error:?}"
        );
    }

    /// B6. This is a trust boundary: a field nobody declared is the two sides
    /// saying different things, and it is refused rather than dropped.
    #[test]
    fn an_extra_field_is_refused() {
        let raw = r#"{"type":"hello","protocolVersion":1,"token":"1a2b","client":{"name":"Snapdeck","version":"0.1.0"},"extra":true}"#;
        let error = ClientMessage::parse(raw).expect_err("`extra` is nobody's field");
        assert!(
            matches!(error, BridgeError::Schema(_)),
            "an unknown field is a schema failure: {error:?}"
        );
    }

    /// B7. Two different failures, told apart. "That was not JSON" and "that
    /// was JSON we do not accept" send whoever is holding the extension to two
    /// different places.
    #[test]
    fn unparsable_text_and_an_incomplete_frame_are_different_failures() {
        let not_json = ClientMessage::parse("not json at all").expect_err("not JSON");
        assert!(
            matches!(not_json, BridgeError::NotJson(_)),
            "text that is not JSON: {not_json:?}"
        );

        let incomplete = r#"{"type":"hello","protocolVersion":1,"token":"1a2b"}"#;
        let missing = ClientMessage::parse(incomplete).expect_err("`client` is missing");
        assert!(
            matches!(missing, BridgeError::Schema(_)),
            "JSON that misses a field: {missing:?}"
        );
    }

    /// B8. An unknown message is named, not swallowed. The name is what tells
    /// a newer extension apart from a wrong one.
    #[test]
    fn an_unrecognised_type_is_named_in_the_error() {
        let error = ClientMessage::parse(r#"{"type":"goodbye"}"#).expect_err("no such message");
        let BridgeError::UnknownType(name) = error else {
            panic!("an unknown `type` is its own failure, not {error:?}");
        };
        assert_eq!(name, "goodbye");
    }

    /// B9. The wire form is camelCase all the way down, including the fields
    /// inside a variant, and a missing `requestId` travels as `null` rather
    /// than being left out.
    #[test]
    fn an_error_message_encodes_to_the_camel_case_wire_form() {
        let frame = ServerMessage::Error {
            request_id: None,
            code: ErrorCode::SaveFailed,
            message: "the capture could not be written".to_owned(),
        }
        .encode();
        assert_eq!(
            frame,
            r#"{"type":"error","requestId":null,"code":"saveFailed","message":"the capture could not be written"}"#
        );
    }

    /// B10. The set of codes the extension can be told is closed, and three
    /// different shapes of malformed frame arrive under one of them.
    #[test]
    fn every_failure_maps_to_the_code_the_extension_is_told() {
        for (error, expected) in [
            (
                BridgeError::NotJson("trailing comma".to_owned()),
                ErrorCode::MalformedMessage,
            ),
            (
                BridgeError::Schema("missing field `client`".to_owned()),
                ErrorCode::MalformedMessage,
            ),
            (
                BridgeError::UnknownType("goodbye".to_owned()),
                ErrorCode::MalformedMessage,
            ),
            (
                BridgeError::VersionMismatch { presented: 2 },
                ErrorCode::UnsupportedProtocolVersion,
            ),
            (BridgeError::Unauthorized, ErrorCode::Unauthorized),
        ] {
            assert_eq!(error.code(), expected, "{error:?}");
        }
    }

    /// B11. Cross-language, in the same spirit as
    /// `output::the_jpeg_quality_matches_the_one_the_editor_encodes_at`: the
    /// claim is between the two languages, not between two Rust constants.
    /// Move either side and this goes red.
    #[test]
    fn the_shared_limits_match_the_typescript_ones() {
        let limits = include_str!("../../../../../packages/protocol/src/limits.ts");
        let png_bytes = u64::try_from(MAX_PNG_BYTES).expect("the test runs on 64 bits");
        let message_bytes = u64::try_from(MAX_MESSAGE_BYTES).expect("the test runs on 64 bits");
        let handshake_bytes =
            u64::try_from(MAX_HANDSHAKE_MESSAGE_BYTES).expect("the test runs on 64 bits");
        let as_number = |value: usize| u64::try_from(value).expect("the test runs on 64 bits");
        for (name, value) in [
            ("PROTOCOL_VERSION", u64::from(PROTOCOL_VERSION)),
            ("BRIDGE_PORT", u64::from(BRIDGE_PORT)),
            ("MAX_PNG_BYTES", png_bytes),
            ("MAX_MESSAGE_BYTES", message_bytes),
            ("MAX_IMAGE_PIXELS", MAX_IMAGE_PIXELS),
            ("NONCE_HEX_LENGTH", as_number(NONCE_HEX_LENGTH)),
            ("PROOF_HEX_LENGTH", as_number(PROOF_HEX_LENGTH)),
            ("MAX_HANDSHAKE_MESSAGE_BYTES", handshake_bytes),
            ("MAX_TOKEN_CHARS", as_number(MAX_TOKEN_CHARS)),
            ("MAX_CLIENT_NAME_CHARS", as_number(MAX_CLIENT_NAME_CHARS)),
            (
                "MAX_CLIENT_VERSION_CHARS",
                as_number(MAX_CLIENT_VERSION_CHARS),
            ),
            ("MAX_REQUEST_ID_CHARS", as_number(MAX_REQUEST_ID_CHARS)),
            ("MAX_URL_CHARS", as_number(MAX_URL_CHARS)),
            ("MAX_TITLE_CHARS", as_number(MAX_TITLE_CHARS)),
            (
                "MAX_ERROR_MESSAGE_CHARS",
                as_number(MAX_ERROR_MESSAGE_CHARS),
            ),
            ("MAX_DEVICE_PIXEL_RATIO", u64::from(MAX_DEVICE_PIXEL_RATIO)),
        ] {
            assert_eq!(
                typescript_number(limits, name),
                Some(value),
                "packages/protocol/src/limits.ts and this file disagree about {name}, so the extension and the desktop would be speaking different protocols"
            );
        }
    }

    /// B12. Cross-language again: the error codes are a closed set on both
    /// sides. A fifth Rust variant stops the match below compiling, and a
    /// fifth variant that never reaches `errorCodeSchema` fails the assertion.
    #[test]
    fn every_error_code_is_in_the_typescript_enum() {
        let messages = include_str!("../../../../../packages/protocol/src/messages.ts");
        let listed = messages
            .split("export const errorCodeSchema = z.enum([")
            .nth(1)
            .and_then(|rest| rest.split("])").next())
            .expect("packages/protocol still declares `errorCodeSchema` as a `z.enum` list");

        for code in [
            ErrorCode::UnsupportedProtocolVersion,
            ErrorCode::Unauthorized,
            ErrorCode::MalformedMessage,
            ErrorCode::SaveFailed,
        ] {
            // Spelled out rather than derived, so that adding a variant is a
            // compile error here before it is a mismatch over the wire.
            let name = match code {
                ErrorCode::UnsupportedProtocolVersion => "unsupportedProtocolVersion",
                ErrorCode::Unauthorized => "unauthorized",
                ErrorCode::MalformedMessage => "malformedMessage",
                ErrorCode::SaveFailed => "saveFailed",
            };
            assert_eq!(
                serde_json::to_string(&code).expect("an error code renders as a JSON string"),
                format!("\"{name}\""),
                "the wire name of {code:?}"
            );
            assert!(
                listed.contains(&format!("'{name}'")),
                "`{name}` is not in `errorCodeSchema`, so the extension cannot understand a failure this side reports"
            );
        }
    }

    /// B13. Security. The nonce is the half of the handshake that runs towards
    /// the extension. A `hello` without one leaves this side with nothing to
    /// prove itself against, so it is not a handshake this protocol knows.
    #[test]
    fn a_hello_without_a_nonce_is_refused() {
        let raw = r#"{"type":"hello","protocolVersion":1,"token":"1a2b","client":{"name":"Snapdeck","version":"0.1.0"}}"#;
        let error = ClientMessage::parse(raw).expect_err("there is nothing to prove against");
        assert!(
            matches!(error, BridgeError::Schema(_)),
            "a missing nonce is a schema failure: {error:?}"
        );
    }

    /// B14. Security. Fixed at 32 hex characters, both ends of the length. A
    /// nonce a caller can shorten is a nonce a caller can exhaust, and one it
    /// can fill with anything is a message this side would sign unread.
    #[test]
    fn a_nonce_that_is_not_thirty_two_hex_characters_is_refused() {
        for nonce in [
            // 32 characters is the contract: 16 bytes rendered as hex.
            "0f1e2d3c4b5a69788796a5b4c3d2e1f",
            "0f1e2d3c4b5a69788796a5b4c3d2e1f00",
            "0f1e2d3c4b5a69788796a5b4c3d2e1fg",
            "",
        ] {
            let error = ClientMessage::parse(&hello_with_nonce(nonce))
                .expect_err("`{nonce}` is not a nonce");
            assert!(
                matches!(error, BridgeError::Schema(_)),
                "a nonce of {} characters or the wrong alphabet is refused: {error:?}",
                nonce.len()
            );
        }

        // Either case, because the proof is taken over the string as sent.
        ClientMessage::parse(&hello_with_nonce("0F1E2D3C4B5A69788796A5B4C3D2E1F0"))
            .expect("hex is hex in either case");
    }

    /// B15. Security, MEDIUM-3. `MAX_IMAGE_PIXELS` applies where it is
    /// declared. Two numbers that are each acceptable multiply into an area
    /// that is not, and the area is what gets allocated further down.
    #[test]
    fn a_picture_that_claims_more_pixels_than_the_limit_is_refused() {
        let error = ClientMessage::parse(&full_page_replacing(&[
            ("width", "100000"),
            ("height", "100000"),
        ]))
        .expect_err("ten thousand million pixels is not a page");
        assert!(
            matches!(error, BridgeError::Schema(_)),
            "an area past the limit is a schema failure: {error:?}"
        );

        // The same width, brought under the limit by its height alone. Without
        // this the test would also pass for a check that refused the width on
        // its own, which would lock out every ordinary tall page.
        ClientMessage::parse(&full_page_replacing(&[
            ("width", "100000"),
            ("height", "640"),
        ]))
        .expect("100000 by 640 is exactly the limit and inside it");

        // And a page with no area at all is not a page.
        ClientMessage::parse(&full_page_with("height", "0")).expect_err("nothing is not a page");
    }

    /// B16. Security, MEDIUM-3. `MAX_PNG_BYTES` applies where it is declared
    /// too: on the length of the text that carries it, which is the number this
    /// side can check before anything is held twice.
    #[test]
    fn a_png_longer_than_the_protocol_carries_is_refused() {
        // 55_924_056 characters is the contract: four for every three bytes of
        // the 41_943_040 a PNG may weigh. Written out rather than read from the
        // constant it pins.
        assert_eq!(MAX_PNG_BASE64_CHARS, 55_924_056);

        let too_long = format!("\"{}\"", "A".repeat(MAX_PNG_BASE64_CHARS + 1));
        let error = ClientMessage::parse(&full_page_with("pngBase64", &too_long))
            .expect_err("that is more base64 than the bridge carries");
        assert!(
            matches!(error, BridgeError::Schema(_)),
            "an oversized payload is a schema failure: {error:?}"
        );
    }

    /// B17. A request id comes back in answers and goes into log lines, so it
    /// is held to an alphabet and not only to a length: a quote or a newline
    /// inside one is a way of writing into somewhere it was meant to be quoted.
    #[test]
    fn a_request_id_outside_its_alphabet_or_past_its_length_is_refused() {
        for request_id in [
            r#""req 1""#.to_owned(),
            r#""req\"1""#.to_owned(),
            r#""req\n1""#.to_owned(),
            r#""""#.to_owned(),
            // 64 characters is the contract.
            format!("\"{}\"", "r".repeat(65)),
        ] {
            let error = ClientMessage::parse(&full_page_with("requestId", &request_id))
                .expect_err("that is not a request id");
            assert!(
                matches!(error, BridgeError::Schema(_)),
                "{request_id} is refused: {error:?}"
            );
        }

        // The shape the extension actually sends: `crypto.randomUUID()`.
        ClientMessage::parse(&full_page_with(
            "requestId",
            r#""3f2504e0-4f89-41d3-9a0c-0305e82c3301""#,
        ))
        .expect("a uuid is a request id");
    }

    /// B18. A device pixel ratio multiplies the canvas the extension builds, so
    /// an unbounded one is a way of asking for an unbounded picture. The last
    /// case is the one a bare comparison misses: a number JSON can hold and
    /// `f32` cannot arrives as infinity, which is greater than every limit and
    /// less than none.
    #[test]
    fn a_device_pixel_ratio_outside_what_a_display_reports_is_refused() {
        for ratio in ["0", "-1", "8.5", "1e40"] {
            let error = ClientMessage::parse(&full_page_with("devicePixelRatio", ratio))
                .expect_err("that is not a device pixel ratio");
            assert!(
                matches!(error, BridgeError::Schema(_)),
                "a ratio of {ratio} is refused: {error:?}"
            );
        }

        // 8 is the contract: the largest a display reports, with room over it.
        ClientMessage::parse(&full_page_with("devicePixelRatio", "8"))
            .expect("8 is inside what a display reports");
    }

    /// B19. The two strings that come off a page the user did not write.
    #[test]
    fn a_url_or_a_title_past_its_cap_is_refused() {
        // 2048 and 1024 are the contract, written out rather than imported.
        for (field, value) in [
            ("url", format!("\"h{}\"", "a".repeat(2048))),
            ("title", format!("\"{}\"", "a".repeat(1025))),
            ("url", r#""""#.to_owned()),
        ] {
            let error =
                ClientMessage::parse(&full_page_with(field, &value)).expect_err("that is too long");
            assert!(
                matches!(error, BridgeError::Schema(_)),
                "an oversized `{field}` is refused: {error:?}"
            );
        }
    }

    /// B20. LOW-2. What this side says is bounded exactly as what it is told
    /// is, or a failure naming a field that came off the wire would arrive at
    /// the extension as a second failure. Counted in UTF-16 code units, which
    /// is the length `packages/protocol` measures a string by.
    #[test]
    fn an_error_message_is_cut_to_what_the_other_schema_accepts() {
        let ServerMessage::Error { message, .. } = ServerMessage::error(
            None,
            ErrorCode::MalformedMessage,
            &"unknown field `".repeat(1000),
        ) else {
            panic!("`error` builds an error");
        };
        // 1024 is the contract, written out rather than read from the constant.
        assert_eq!(
            message.encode_utf16().count(),
            1024,
            "the text is cut to the length the extension will accept: {message}"
        );

        let ServerMessage::Error { message, .. } =
            ServerMessage::error(None, ErrorCode::SaveFailed, "the disk is full")
        else {
            panic!("`error` builds an error");
        };
        assert_eq!(
            message, "the disk is full",
            "and an ordinary failure is left exactly as it was"
        );
    }

    /// B21. The proof travels in `ready`, in the same camelCase wire form as
    /// everything else, or the extension cannot read the one field it has to
    /// check before it sends a page.
    #[test]
    fn a_ready_message_carries_its_proof_on_the_wire() {
        let frame = ServerMessage::Ready {
            protocol_version: PROTOCOL_VERSION,
            app: AppInfo {
                name: "snapdeck".to_owned(),
                version: "0.1.0".to_owned(),
            },
            proof: "abc".to_owned(),
        }
        .encode();
        assert_eq!(
            frame,
            r#"{"type":"ready","protocolVersion":1,"app":{"name":"snapdeck","version":"0.1.0"},"proof":"abc"}"#
        );
    }

    /// The value of `export const <name> = <number>` in a TypeScript source,
    /// with the digit separators dropped. `None` when the file does not declare
    /// it at all, which is as much of a drift as a changed number.
    fn typescript_number(source: &str, name: &str) -> Option<u64> {
        let declaration = format!("export const {name} = ");
        let rest = source.split(&declaration).nth(1)?;
        let digits: String = rest
            .chars()
            .take_while(|character| character.is_ascii_digit() || *character == '_')
            .filter(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    }
}
