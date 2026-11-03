//! Text decoding bounds both pasted input and decoded bytes before allocating a payload.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::code::{CodeError, Invite, MAX_PASTE_BYTES, MAX_PAYLOAD_BYTES, Reply};
use crate::policy::AddressPolicy;

const PREFIX: &[u8] = b"orqwa:";
const MAX_TEXT: usize = 6 + MAX_PAYLOAD_BYTES.div_ceil(3) * 4;

fn decode(text: &str) -> Result<([u8; MAX_PAYLOAD_BYTES + 3], usize), CodeError> {
    if text.len() > MAX_PASTE_BYTES {
        return Err(CodeError::TooLong);
    }
    let mut normalized = [0; MAX_TEXT];
    let mut count = 0;
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        if !c.is_ascii() {
            return Err(CodeError::Invalid);
        }
        let next = normalized.get_mut(count).ok_or(CodeError::TooLong)?;
        *next = u8::try_from(u32::from(c)).map_err(|_| CodeError::Invalid)?;
        count += 1;
    }
    let body = normalized[..count]
        .strip_prefix(PREFIX)
        .ok_or(CodeError::Invalid)?;
    // base64's conservative output-size estimate can exceed the actual final chunk by two.
    let mut bytes = [0; MAX_PAYLOAD_BYTES + 3];
    let len = URL_SAFE_NO_PAD
        .decode_slice(body, &mut bytes)
        .map_err(|_| CodeError::Invalid)?;
    if len > MAX_PAYLOAD_BYTES {
        return Err(CodeError::TooLong);
    }
    Ok((bytes, len))
}

fn encode(bytes: &[u8]) -> String {
    format!("orqwa:{}", URL_SAFE_NO_PAD.encode(bytes))
}

impl Invite {
    /// A bounded, validated invite to pass through an authenticated private channel.
    ///
    /// # Errors
    /// Returns [`CodeError`] if any payload field or destination is invalid.
    pub fn to_code(&self, policy: AddressPolicy) -> Result<String, CodeError> {
        Ok(encode(&self.encode(policy)?))
    }

    /// Reads a pasted invite, rejecting excessive input before decoding or allocating strings.
    /// Whitespace inserted by messengers is ignored within the raw paste limit.
    ///
    /// # Errors
    /// Returns [`CodeError`] for invalid, oversized or forbidden input.
    pub fn from_code(text: &str, policy: AddressPolicy) -> Result<Self, CodeError> {
        let (bytes, len) = decode(text)?;
        Self::decode(&bytes[..len], policy)
    }
}

impl Reply {
    /// Encodes a reply containing a secret: share it privately, never log it.
    ///
    /// # Errors
    /// Returns [`CodeError`] if any payload field or destination is invalid.
    pub fn to_code(&self, policy: AddressPolicy) -> Result<String, CodeError> {
        Ok(encode(&self.encode(policy)?))
    }

    /// Reads a pasted reply with the same pre-allocation limits as an invite.
    ///
    /// # Errors
    /// Returns [`CodeError`] for invalid, oversized or forbidden input.
    pub fn from_code(text: &str, policy: AddressPolicy) -> Result<Self, CodeError> {
        let (bytes, len) = decode(text)?;
        Self::decode(&bytes[..len], policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::{Candidate, CandidateKind, Fingerprint, NatObservation, RideId, RouteHash};

    fn invite() -> Invite {
        Invite {
            ride: RideId([1; 16]),
            fingerprint: Fingerprint([2; 32]),
            route: RouteHash([3; 32]),
            host: "Anna".into(),
            course: "Col du Galibier".into(),
            candidates: vec![
                Candidate::new(
                    CandidateKind::Public,
                    "1.1.1.1:50000".parse().unwrap(),
                    AddressPolicy::default(),
                )
                .unwrap(),
            ],
            nat: NatObservation::None,
            setup: crate::code::Setup::default(),
        }
    }

    #[test]
    fn valid_invites_and_bound_replies_survive_messenger_whitespace() {
        let policy = AddressPolicy::default();
        let invite = invite();
        let code = invite.to_code(policy).unwrap();
        let wrapped = code.chars().flat_map(|c| [c, '\n']).collect::<String>();
        assert_eq!(Invite::from_code(&wrapped, policy), Ok(invite.clone()));
        let reply = Reply {
            ride: invite.ride,
            fingerprint: invite.fingerprint,
            route: invite.route,
            client: Fingerprint([5; 32]),
            candidates: invite.candidates.clone(),
            nat: NatObservation::None,
        };
        assert_eq!(
            Reply::from_code(&reply.to_code(policy).unwrap(), policy),
            Ok(reply)
        );
        assert_eq!(Reply::from_code(&code, policy), Err(CodeError::WrongKind));
    }

    #[test]
    fn oversized_input_invalid_alphabet_padding_and_noncanonical_base64_are_rejected() {
        let policy = AddressPolicy::default();
        assert_eq!(
            Invite::from_code(&" ".repeat(MAX_PASTE_BYTES + 1), policy),
            Err(CodeError::TooLong)
        );
        assert_eq!(
            Invite::from_code(&format!("orqwa:{}", "A".repeat(MAX_TEXT)), policy),
            Err(CodeError::TooLong)
        );
        for body in [
            "orqwa:AA+Z",
            "orqwa:AA/Z",
            "orqwa:AA==",
            "torqa:AA",
            "orqwa:AB",
            "orqwa:é",
        ] {
            assert!(Invite::from_code(body, policy).is_err());
        }
        let code = encode(&[1; MAX_PAYLOAD_BYTES + 1]);
        assert_eq!(Invite::from_code(&code, policy), Err(CodeError::TooLong));
    }
}
