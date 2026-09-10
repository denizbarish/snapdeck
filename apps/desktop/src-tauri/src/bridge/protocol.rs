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
            HELLO_TYPE => serde_json::from_value(frame)
                .map(Self::Hello)
                .map_err(|err| BridgeError::Schema(err.to_string())),
            FULL_PAGE_TYPE => serde_json::from_value(frame)
                .map(Self::FullPage)
                .map_err(|err| BridgeError::Schema(err.to_string())),
            _ => Err(BridgeError::UnknownType(tag)),
        }
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

    /// A `hello` frame with every field the schema asks for and nothing else.
    const WELL_FORMED_HELLO: &str = r#"{"type":"hello","protocolVersion":1,"token":"1a2b","client":{"name":"Snapdeck","version":"0.1.0"}}"#;

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
        for (name, value) in [
            ("PROTOCOL_VERSION", u64::from(PROTOCOL_VERSION)),
            ("BRIDGE_PORT", u64::from(BRIDGE_PORT)),
            ("MAX_PNG_BYTES", png_bytes),
            ("MAX_MESSAGE_BYTES", message_bytes),
            ("MAX_IMAGE_PIXELS", MAX_IMAGE_PIXELS),
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
