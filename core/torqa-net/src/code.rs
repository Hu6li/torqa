//! The codes riders pass each other to ride together (R68): the host's [`Invite`] and each
//! joiner's [`Reply`]. As text they are `torqa:` and the base64url of a compact binary form,
//! short enough to paste into any messenger and to show as a QR code.
//!
//! The binary form: a version byte, a kind byte, then the kind's fields. Strings are a length
//! byte and UTF-8; addresses a candidate kind byte, a family byte (4 or 6), the address and the
//! port, big-endian. After the addresses comes how the router maps ports (see
//! [`NatBehavior`]).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::base64url;
use crate::candidates::{Candidate, CandidateKind, NatBehavior};

/// What every code starts with, so a code is told apart from other text.
pub const PREFIX: &str = "torqa:";

/// The codes' format. A Torqa refuses codes of a newer one rather than misreading them.
pub const VERSION: u8 = 1;

/// At most this many addresses go into a code, the best first: more rarely help, and the code
/// grows with each.
pub const MAX_CANDIDATES: usize = 8;

/// Names are cut to this many bytes in a code (at a character boundary).
pub const MAX_NAME_BYTES: usize = 64;

const KIND_INVITE: u8 = 1;
const KIND_REPLY: u8 = 2;

/// The host's invite to a group ride. One invite serves everyone who joins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    /// The host's name, shown to the joiner.
    pub host: String,
    /// The course everyone rides.
    pub course: Course,
    /// SHA-256 of the certificate the host shows for this ride; a joiner trusts no other.
    pub fingerprint: [u8; 32],
    /// Where the host may be reached, the best first.
    pub candidates: Vec<Candidate>,
    /// How the host's router maps ports, to tell why a connection failed.
    pub nat: NatBehavior,
}

/// The course of a group ride, as a joiner finds it in their library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Course {
    /// The course's name, for telling the joiner which course they are missing.
    pub name: String,
    /// SHA-256 of the course's route: the same course has the same route.
    pub route_hash: [u8; 32],
}

impl Course {
    /// The course named `name` whose route is `route`: the bytes that make the route, the same
    /// for everyone who has the course.
    #[must_use]
    pub fn new(name: &str, route: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            route_hash: crate::link::sha256(&crate::link::provider(), route),
        }
    }
}

/// A joiner's answer to an invite, which the host pastes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The joiner shows this secret once connected; the host lets in no one else.
    pub secret: [u8; 16],
    /// Where the joiner may be reached, the best first.
    pub candidates: Vec<Candidate>,
    /// How the joiner's router maps ports, to tell why a connection failed.
    pub nat: NatBehavior,
}

/// Why text is not a code that can be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodeError {
    /// Not a Torqa code at all, or one damaged on its way (cut short, characters changed).
    #[error("This is not a Torqa code, or it was not copied completely.")]
    NotACode,
    /// A code from a newer Torqa.
    #[error("This code comes from a newer Torqa. Update Torqa to use it.")]
    Newer,
    /// An invite where a reply belongs, or the other way round.
    #[error("This is an invite; paste a reply here.")]
    InviteInsteadOfReply,
    /// A reply where an invite belongs.
    #[error("This is a reply; paste an invite here.")]
    ReplyInsteadOfInvite,
}

impl Invite {
    /// The invite as text.
    #[must_use]
    pub fn to_code(&self) -> String {
        let mut w = Writer::new(KIND_INVITE);
        w.bytes(&self.fingerprint);
        w.bytes(&self.course.route_hash);
        w.string(&self.course.name);
        w.string(&self.host);
        w.candidates(&self.candidates);
        w.0.push(self.nat.to_byte());
        w.finish()
    }

    /// Reads an invite pasted by the rider.
    ///
    /// # Errors
    ///
    /// The text is no invite, or one this Torqa cannot read ([`CodeError`]).
    pub fn from_code(text: &str) -> Result<Self, CodeError> {
        let mut r = Reader::open(text)?;
        match r.kind {
            KIND_INVITE => {}
            KIND_REPLY => return Err(CodeError::ReplyInsteadOfInvite),
            _ => return Err(CodeError::NotACode),
        }
        let fingerprint = r.array()?;
        let route_hash = r.array()?;
        let name = r.string()?;
        let host = r.string()?;
        let candidates = r.candidates()?;
        let nat = NatBehavior::from_byte(r.byte()?).ok_or(CodeError::NotACode)?;
        r.end()?;
        Ok(Self {
            host,
            course: Course { name, route_hash },
            fingerprint,
            candidates,
            nat,
        })
    }
}

impl Reply {
    /// The reply as text.
    #[must_use]
    pub fn to_code(&self) -> String {
        let mut w = Writer::new(KIND_REPLY);
        w.bytes(&self.secret);
        w.candidates(&self.candidates);
        w.0.push(self.nat.to_byte());
        w.finish()
    }

    /// Reads a reply pasted by the host.
    ///
    /// # Errors
    ///
    /// The text is no reply, or one this Torqa cannot read ([`CodeError`]).
    pub fn from_code(text: &str) -> Result<Self, CodeError> {
        let mut r = Reader::open(text)?;
        match r.kind {
            KIND_REPLY => {}
            KIND_INVITE => return Err(CodeError::InviteInsteadOfReply),
            _ => return Err(CodeError::NotACode),
        }
        let secret = r.array()?;
        let candidates = r.candidates()?;
        let nat = NatBehavior::from_byte(r.byte()?).ok_or(CodeError::NotACode)?;
        r.end()?;
        Ok(Self {
            secret,
            candidates,
            nat,
        })
    }
}

/// `text` cut to [`MAX_NAME_BYTES`] at a character boundary.
pub(crate) fn cut_name(text: &str) -> &str {
    let mut end = text.len().min(MAX_NAME_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

struct Writer(Vec<u8>);

impl Writer {
    fn new(kind: u8) -> Self {
        Self(vec![VERSION, kind])
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn string(&mut self, text: &str) {
        let text = cut_name(text);
        #[allow(clippy::cast_possible_truncation)] // at most MAX_NAME_BYTES
        self.0.push(text.len() as u8);
        self.bytes(text.as_bytes());
    }

    fn candidates(&mut self, candidates: &[Candidate]) {
        let candidates = &candidates[..candidates.len().min(MAX_CANDIDATES)];
        #[allow(clippy::cast_possible_truncation)] // at most MAX_CANDIDATES
        self.0.push(candidates.len() as u8);
        for c in candidates {
            self.0.push(c.kind.to_byte());
            match c.addr.ip() {
                IpAddr::V4(ip) => {
                    self.0.push(4);
                    self.bytes(&ip.octets());
                }
                IpAddr::V6(ip) => {
                    self.0.push(6);
                    self.bytes(&ip.octets());
                }
            }
            self.bytes(&c.addr.port().to_be_bytes());
        }
    }

    fn finish(self) -> String {
        format!("{PREFIX}{}", base64url::encode(&self.0))
    }
}

struct Reader {
    bytes: Vec<u8>,
    at: usize,
    kind: u8,
}

impl Reader {
    /// Messengers wrap long lines and add spaces; none of them belong to a code.
    fn open(text: &str) -> Result<Self, CodeError> {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let body = text.strip_prefix(PREFIX).ok_or(CodeError::NotACode)?;
        let bytes = base64url::decode(body).ok_or(CodeError::NotACode)?;
        match bytes.as_slice() {
            [version, ..] if *version > VERSION => Err(CodeError::Newer),
            [VERSION, kind, ..] => Ok(Self {
                kind: *kind,
                bytes,
                at: 2,
            }),
            _ => Err(CodeError::NotACode),
        }
    }

    fn take(&mut self, n: usize) -> Result<&[u8], CodeError> {
        let end = self.at + n;
        let taken = self.bytes.get(self.at..end).ok_or(CodeError::NotACode)?;
        self.at = end;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, CodeError> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CodeError> {
        Ok(self.take(N)?.try_into().expect("took N bytes"))
    }

    fn string(&mut self) -> Result<String, CodeError> {
        let len = usize::from(self.byte()?);
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| CodeError::NotACode)
    }

    fn candidates(&mut self) -> Result<Vec<Candidate>, CodeError> {
        let count = usize::from(self.byte()?);
        if count > MAX_CANDIDATES {
            return Err(CodeError::NotACode);
        }
        (0..count)
            .map(|_| {
                let kind = CandidateKind::from_byte(self.byte()?).ok_or(CodeError::NotACode)?;
                let ip = match self.byte()? {
                    4 => IpAddr::V4(Ipv4Addr::from(self.array::<4>()?)),
                    6 => IpAddr::V6(Ipv6Addr::from(self.array::<16>()?)),
                    _ => return Err(CodeError::NotACode),
                };
                let port = u16::from_be_bytes(self.array()?);
                Ok(Candidate {
                    kind,
                    addr: SocketAddr::new(ip, port),
                })
            })
            .collect()
    }

    fn end(&self) -> Result<(), CodeError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(CodeError::NotACode)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(kind: CandidateKind, addr: &str) -> Candidate {
        Candidate {
            kind,
            addr: addr.parse().unwrap(),
        }
    }

    fn invite() -> Invite {
        Invite {
            host: "Jens".into(),
            course: Course {
                name: "Col du Galibier".into(),
                route_hash: [7; 32],
            },
            fingerprint: [42; 32],
            candidates: vec![
                candidate(CandidateKind::Router, "203.0.113.9:40001"),
                candidate(CandidateKind::Stun, "203.0.113.9:51820"),
                candidate(CandidateKind::Public, "[2001:db8::1]:51820"),
                candidate(CandidateKind::Local, "192.168.1.20:51820"),
            ],
            nat: NatBehavior::EndpointIndependent,
        }
    }

    fn reply() -> Reply {
        Reply {
            secret: [9; 16],
            candidates: vec![candidate(CandidateKind::Stun, "198.51.100.4:61000")],
            nat: NatBehavior::Symmetric,
        }
    }

    #[test]
    fn an_invite_and_a_reply_come_back_as_they_were_sent() {
        let code = invite().to_code();
        assert!(code.starts_with("torqa:"));
        assert_eq!(Invite::from_code(&code), Ok(invite()));
        assert_eq!(Reply::from_code(&reply().to_code()), Ok(reply()));
    }

    #[test]
    fn survives_messengers_wrapping_and_spacing_it() {
        let code = invite().to_code();
        let (a, b) = code.split_at(30);
        let mangled = format!("  {a}\n {b}\r\n");
        assert_eq!(Invite::from_code(&mangled), Ok(invite()));
    }

    #[test]
    fn a_full_invite_fits_a_qr_code_and_a_message() {
        let mut full = invite();
        full.host = "x".repeat(200);
        full.course.name = "y".repeat(200);
        full.candidates = (0..20)
            .map(|i| candidate(CandidateKind::Public, &format!("[2001:db8::{i}]:65535")))
            .collect();
        let code = full.to_code();
        // A QR code of version 25 holds 1273 bytes at error correction level L.
        assert!(code.len() < 800, "{} characters", code.len());
    }

    #[test]
    fn keeps_the_best_candidates_when_there_are_too_many() {
        let mut many = invite();
        many.candidates = (0..20)
            .map(|i| candidate(CandidateKind::Local, &format!("10.0.0.{i}:1000")))
            .collect();
        let read = Invite::from_code(&many.to_code()).unwrap();
        assert_eq!(read.candidates, many.candidates[..MAX_CANDIDATES]);
    }

    #[test]
    fn cuts_long_names_between_characters() {
        let mut long = invite();
        long.host = "ö".repeat(40); // 80 bytes
        let read = Invite::from_code(&long.to_code()).unwrap();
        assert_eq!(read.host, "ö".repeat(MAX_NAME_BYTES / 2));
    }

    #[test]
    fn tells_an_invite_from_a_reply() {
        assert_eq!(
            Invite::from_code(&reply().to_code()),
            Err(CodeError::ReplyInsteadOfInvite)
        );
        assert_eq!(
            Reply::from_code(&invite().to_code()),
            Err(CodeError::InviteInsteadOfReply)
        );
    }

    #[test]
    fn refuses_text_that_is_no_code() {
        for text in ["", "hello", "torqa:", "torqa:!!!!", "https://example.com"] {
            assert_eq!(Invite::from_code(text), Err(CodeError::NotACode), "{text}");
        }
    }

    #[test]
    fn refuses_a_code_cut_short_or_with_more_than_it_should_have() {
        let code = invite().to_code();
        for end in [10, code.len() / 2, code.len() - 4] {
            assert_eq!(
                Invite::from_code(&code[..end]),
                Err(CodeError::NotACode),
                "{end}"
            );
        }
        let mut bytes = base64url::decode(&code[PREFIX.len()..]).unwrap();
        bytes.push(0);
        let longer = format!("{PREFIX}{}", base64url::encode(&bytes));
        assert_eq!(Invite::from_code(&longer), Err(CodeError::NotACode));
    }

    #[test]
    fn says_when_a_code_comes_from_a_newer_torqa() {
        let code = reply().to_code();
        let mut bytes = base64url::decode(&code[PREFIX.len()..]).unwrap();
        bytes[0] = VERSION + 1;
        bytes.push(0xff); // newer versions may add fields
        let newer = format!("{PREFIX}{}", base64url::encode(&bytes));
        assert_eq!(Reply::from_code(&newer), Err(CodeError::Newer));
    }
}
