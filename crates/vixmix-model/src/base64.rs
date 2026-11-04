//! Standard padded base64 (RFC 4648): the text an insert's plugin state takes in
//! `setup.ron`, one line instead of one number per line. Vix stores a plugin's state the same
//! way (`crates/vix-core/src/base64.rs`).

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serializer};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` as padded base64.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |word, (i, b)| word | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((word >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes `text` encodes. ASCII whitespace is skipped, so a value wrapped by hand still
/// reads.
pub(crate) fn decode(text: &str) -> Result<Vec<u8>, NotBase64> {
    let symbols: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !symbols.len().is_multiple_of(4) {
        return Err(NotBase64::Length(symbols.len()));
    }
    let mut out = Vec::with_capacity(symbols.len() / 4 * 3);
    let quads = symbols.len() / 4;
    for (q, quad) in symbols.chunks(4).enumerate() {
        let mut word = 0u32;
        let mut padding = 0;
        for (i, &symbol) in quad.iter().enumerate() {
            let value = if symbol == b'=' && q + 1 == quads && i >= 2 {
                padding += 1;
                0
            } else if padding > 0 {
                return Err(NotBase64::AfterPadding(q * 4 + i));
            } else {
                value_of(symbol).ok_or(NotBase64::Symbol {
                    found: char::from(symbol),
                    at: q * 4 + i,
                })?
            };
            word = (word << 6) | value;
        }
        let bytes = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        out.extend_from_slice(&bytes[..3 - padding]);
    }
    Ok(out)
}

fn value_of(symbol: u8) -> Option<u32> {
    ALPHABET.iter().position(|&a| a == symbol).map(|v| v as u32)
}

/// Why a plugin state in `setup.ron` could not be read back. The fix is the same for each:
/// the state is the plugin's own, so it cannot be repaired by hand, only dropped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotBase64 {
    #[error(
        "a plugin state is not base64: {0} symbols is not a whole number of groups of four; write `state: None` to start that plugin from its defaults"
    )]
    Length(usize),
    #[error(
        "a plugin state is not base64: {found:?} at symbol {at} is not one; write `state: None` to start that plugin from its defaults"
    )]
    Symbol { found: char, at: usize },
    #[error(
        "a plugin state is not base64: symbol {0} comes after the `=` padding; write `state: None` to start that plugin from its defaults"
    )]
    AfterPadding(usize),
}

/// `#[serde(with = "crate::base64::option")]` for `Option<Vec<u8>>`: `None`, or
/// `Some("<base64>")`.
pub(crate) mod option {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(
        state: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match state {
            Some(bytes) => serializer.serialize_some(&encode(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| decode(&text).map_err(D::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn it_is_rfc_4648s_standard_alphabet_with_padding() {
        // RFC 4648 § 10's test vectors.
        for (bytes, text) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(bytes.as_bytes()), text);
            assert_eq!(decode(text).unwrap(), bytes.as_bytes());
        }
        assert_eq!(encode(&[0xfb, 0xff]), "+/8=");
        assert_eq!(decode("Zm9v\n  YmFy").unwrap(), b"foobar");
    }

    #[test]
    fn what_is_not_base64_is_refused_with_the_fix() {
        for text in ["Zm9", "Zm9v!mFy", "Zg=a", "Z===", "Zg==Zg=="] {
            let error = decode(text).unwrap_err();
            assert!(error.to_string().contains("state: None"), "{text}: {error}");
        }
    }

    proptest! {
        #[test]
        fn any_bytes_survive_a_round_trip(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            prop_assert_eq!(decode(&encode(&bytes)).unwrap(), bytes);
        }
    }
}
