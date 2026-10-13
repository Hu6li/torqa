//! Base64url without padding (RFC 4648 §5): the codes' text, safe in links, file names and
//! messengers, which tend to mangle `+`, `/` and `=`.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |w, (i, &b)| w | u32::from(b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            text.push(char::from(ALPHABET[(word >> (18 - 6 * i)) as usize & 63]));
        }
    }
    text
}

/// `None` for characters outside the alphabet or a length no encoding has.
pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.as_bytes().chunks(4) {
        let mut word = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            let value = ALPHABET.iter().position(|&a| a == c)?;
            #[allow(clippy::cast_possible_truncation)] // below 64
            let value = value as u32;
            word |= value << (18 - 6 * i);
        }
        #[allow(clippy::cast_possible_truncation)] // bytes of a 24-bit word
        for i in 0..chunk.len() - 1 {
            bytes.push((word >> (16 - 8 * i)) as u8);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_rfc_4648_test_vectors_without_padding() {
        let vectors = [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in vectors {
            assert_eq!(encode(plain.as_bytes()), encoded);
            assert_eq!(decode(encoded).as_deref(), Some(plain.as_bytes()));
        }
    }

    #[test]
    fn uses_the_url_safe_alphabet() {
        assert_eq!(encode(&[0xfb, 0xff]), "-_8");
        assert_eq!(decode("-_8"), Some(vec![0xfb, 0xff]));
    }

    #[test]
    fn every_byte_value_and_length_comes_back() {
        let bytes: Vec<u8> = (0..=255).collect();
        for len in 0..bytes.len() {
            assert_eq!(
                decode(&encode(&bytes[..len])).as_deref(),
                Some(&bytes[..len])
            );
        }
    }

    #[test]
    fn refuses_what_no_encoding_produces() {
        assert_eq!(decode("Zm9v+g"), None);
        assert_eq!(decode("Zm9v="), None);
        assert_eq!(decode("Zm9vY"), None);
    }
}
