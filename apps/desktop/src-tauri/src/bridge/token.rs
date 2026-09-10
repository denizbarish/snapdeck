//! The pairing token, the one secret the bridge has.
//!
//! Anything running on this machine can open a socket to `127.0.0.1`, so the
//! port is not a secret and the `Origin` header only says that a Chrome
//! extension is calling, not which one. The token is what turns "some
//! extension" into "the extension the user pasted this into": it is generated
//! here, shown once in the settings window, and pasted into the extension's
//! options page.
//!
//! Hex rather than base64 because it is a string a person copies, and hex has
//! no case to get wrong, no `+/=` to be eaten by a form and no encoding to
//! choose. Thirty-two bytes are 64 characters, which is short enough to paste
//! and far past anything worth guessing at loopback speeds.

/// Bytes of system randomness behind a pairing token.
pub const TOKEN_BYTES: usize = 32;

/// The digits a byte is rendered with. Lowercase, because the token is compared
/// as the bytes it is, not case-folded.
const HEX_DIGITS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// A fresh pairing token: `TOKEN_BYTES` bytes from the system CSPRNG, lowercase hex.
///
/// The failure is passed back rather than swallowed with a fallback: a token
/// built out of anything other than system randomness would look exactly like a
/// real one to the user pasting it, and be worth nothing.
pub fn generate_token() -> Result<String, String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|err| format!("failed to read {TOKEN_BYTES} bytes of system randomness: {err}"))?;

    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        // Both halves of a byte are 0..=15, which is the length of `HEX_DIGITS`.
        token.push(HEX_DIGITS[usize::from(byte >> 4)]);
        token.push(HEX_DIGITS[usize::from(byte & 0x0f)]);
    }
    Ok(token)
}

/// Whether `presented` is `expected`, in time that does not depend on how much
/// of it matched.
///
/// The length difference is folded into the accumulator instead of being an
/// early `return false`, and the walk covers the longer of the two rather than
/// stopping where a `zip` would: a loop that stops at the shorter one answers
/// `true` for any prefix of the real token, which would let an extension pair
/// itself one character at a time.
pub fn tokens_match(expected: &str, presented: &str) -> bool {
    let expected = expected.as_bytes();
    let presented = presented.as_bytes();

    let mut difference = u8::from(expected.len() != presented.len());
    for index in 0..expected.len().max(presented.len()) {
        let left = expected.get(index).copied().unwrap_or_default();
        let right = presented.get(index).copied().unwrap_or_default();
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B1. The token is what stands between anything else on this machine and
    /// the save path, so its shape and its freshness are both part of it.
    #[test]
    fn a_fresh_token_is_lowercase_hex_and_never_repeats() {
        let token = generate_token().expect("the system CSPRNG answers");
        // Written out rather than derived from `TOKEN_BYTES`: 64 characters is
        // the contract, and a length taken from the constant would follow the
        // constant down to something worth guessing.
        assert_eq!(
            token.len(),
            64,
            "a pairing token is 64 hex characters, {TOKEN_BYTES} bytes of randomness: {token}"
        );
        assert!(
            token
                .chars()
                .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)),
            "the token has to survive a copy and a paste exactly as it is: {token}"
        );

        let second = generate_token().expect("the system CSPRNG answers twice");
        assert_ne!(
            token, second,
            "two tokens taken this close together must not be the same value"
        );
    }

    /// B2. The plain case, in both directions.
    #[test]
    fn a_token_matches_itself_and_not_a_changed_one() {
        let token = generate_token().expect("the system CSPRNG answers");
        assert!(tokens_match(&token, &token), "a token is itself: {token}");

        let mut changed = token.clone();
        changed.replace_range(0..1, "z");
        assert!(
            !tokens_match(&token, &changed),
            "one character apart is not a match: {token} against {changed}"
        );
    }

    /// B3. A prefix is not the token, whichever side is short. A comparison
    /// written with `zip` alone stops at the shorter of the two and says yes
    /// here, which makes a one-character token enough to open the bridge.
    #[test]
    fn a_prefix_is_not_the_token() {
        assert!(
            !tokens_match("ab", "abcd"),
            "a longer string that starts with the token"
        );
        assert!(
            !tokens_match("abcd", "ab"),
            "and a prefix of the token presented as the whole of it"
        );
    }
}
