//! Size-bounded, versioned invite/reply payloads (R160, R161).
//!
//! A code is not proof of its sender's identity: the invite's pin authenticates the host only if
//! the invite arrives unaltered, so exchange codes through a channel the riders trust. A reply
//! names the joiner's own per-ride certificate, so a copied reply admits nobody else.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::policy::{AddressPolicy, is_public};

/// Maximum binary payload size, checked before any parser allocation.
pub const MAX_PAYLOAD_BYTES: usize = 512;
/// Maximum raw pasted UTF-8 bytes, including whitespace, before decoding.
pub const MAX_PASTE_BYTES: usize = 2048;
/// Maximum UTF-8 bytes in a rider or course name.
pub const MAX_NAME_BYTES: usize = 64;
/// Maximum destinations in either code, also limiting probe fan-out.
pub const MAX_CANDIDATES: usize = 8;
/// Wire version. Old Torqa codes are deliberately incompatible with the new security contract.
pub const VERSION: u8 = 9;

const INVITE: u8 = 1;
const REPLY: u8 = 2;

/// An opaque identifier for this ride, independently generated for each host lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RideId(pub [u8; 16]);

/// SHA-256 of the complete per-ride host certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

/// SHA-256 of the route representation shared by all riders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteHash(pub [u8; 32]);

impl RouteHash {
    /// Course-independent app lobby binding. The separate random ride ID still authenticates
    /// one host lifetime; activity identities are exchanged only after admission.
    pub const LOBBY: Self = Self([0; 32]);
}

/// A candidate's source. This label never overrides the receiver's destination policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateKind {
    /// A deliberately enabled router mapping.
    Router,
    /// A global address of the selected local interface.
    Public,
    /// An address observed by a STUN server.
    Stun,
    /// A LAN or loopback candidate, offered only by explicit choice.
    Local,
}

impl CandidateKind {
    const fn byte(self) -> u8 {
        match self {
            Self::Router => 1,
            Self::Public => 2,
            Self::Stun => 3,
            Self::Local => 4,
        }
    }

    fn from_byte(value: u8) -> Result<Self, CodeError> {
        match value {
            1 => Ok(Self::Router),
            2 => Ok(Self::Public),
            3 => Ok(Self::Stun),
            4 => Ok(Self::Local),
            _ => Err(CodeError::Invalid),
        }
    }
}

/// A destination permitted by the policy used to construct or decode it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    kind: CandidateKind,
    addr: SocketAddr,
}

impl Candidate {
    /// Checks a candidate's address and label against the chosen policy.
    ///
    /// # Errors
    /// Returns [`CodeError::Destination`] if either the label or address is not permitted.
    pub fn new(
        kind: CandidateKind,
        addr: SocketAddr,
        policy: AddressPolicy,
    ) -> Result<Self, CodeError> {
        if !policy.permits(addr) || (kind == CandidateKind::Local) == is_public(addr.ip()) {
            return Err(CodeError::Destination);
        }
        Ok(Self { kind, addr })
    }

    /// Its origin, for diagnostics only.
    #[must_use]
    pub const fn kind(self) -> CandidateKind {
        self.kind
    }

    /// The destination. Recheck policy before sending if permissions have changed.
    #[must_use]
    pub const fn addr(self) -> SocketAddr {
        self.addr
    }
}

/// Limited evidence from STUN observations; it does not prove connectivity to another peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NatObservation {
    /// No observation, including when STUN is disabled.
    None,
    /// Only one server answered.
    OneServer,
    /// Multiple servers observed the same mapping.
    SameMapping,
    /// Multiple servers observed different mappings.
    DifferentMappings,
}

impl NatObservation {
    const fn byte(self) -> u8 {
        match self {
            Self::None => 0,
            Self::OneServer => 1,
            Self::SameMapping => 2,
            Self::DifferentMappings => 3,
        }
    }

    fn from_byte(value: u8) -> Result<Self, CodeError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::OneServer),
            2 => Ok(Self::SameMapping),
            3 => Ok(Self::DifferentMappings),
            _ => Err(CodeError::Invalid),
        }
    }
}

/// Whether friends at other homes can probably reach a listener with these addresses (R176).
///
/// Discovery evidence, not proof: nothing here sees the provider's NAT itself, and a direct
/// connection can still fail on a firewall. Riders only need it to decide who should host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reachability {
    /// This computer has a public IPv4 address.
    Direct,
    /// Two servers saw the same public IPv4 mapping, so hole punching usually works.
    Mapped,
    /// A global IPv6 address, while IPv4 evidence is strict or missing: friends without IPv6
    /// may not get through.
    Ipv6,
    /// Servers saw different IPv4 mappings: the NAT changes the port for each destination, as
    /// carrier-grade NAT often does, so friends at other homes probably cannot connect.
    Strict,
    /// Fewer than two servers answered: not enough evidence either way.
    Unknown,
    /// Only home-network or local-test addresses.
    Home,
}

impl Reachability {
    /// Classifies advertised candidates with the IPv4 STUN observation that accompanies them.
    #[must_use]
    pub fn of(candidates: &[Candidate], nat: NatObservation) -> Self {
        let has = |kind: CandidateKind, v4: bool| {
            candidates
                .iter()
                .any(|c| c.kind == kind && c.addr.is_ipv4() == v4)
        };
        let internet = |c: &Candidate| c.kind != CandidateKind::Local;
        if has(CandidateKind::Public, true) || has(CandidateKind::Router, true) {
            Self::Direct
        } else if nat == NatObservation::SameMapping && has(CandidateKind::Stun, true) {
            Self::Mapped
        } else if candidates.iter().any(|c| internet(c) && c.addr.is_ipv6()) {
            Self::Ipv6
        } else if nat == NatObservation::DifferentMappings {
            Self::Strict
        } else if candidates.iter().any(internet) {
            Self::Unknown
        } else {
            Self::Home
        }
    }

    /// A stable key for presentation.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Mapped => "mapped",
            Self::Ipv6 => "ipv6",
            Self::Strict => "strict",
            Self::Unknown => "unknown",
            Self::Home => "home",
        }
    }
}

/// The host's per-ride invite, authenticated by the channel through which it is exchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    /// A fresh identifier for this host lifetime.
    pub ride: RideId,
    /// Certificate pin accepted for this ride only.
    pub fingerprint: Fingerprint,
    /// Route required locally, before joining.
    pub route: RouteHash,
    /// Host's display name, bounded and without terminal/control characters.
    pub host: String,
    /// Course's display name, bounded and without terminal/control characters.
    pub course: String,
    /// At most eight distinct permitted destinations.
    pub candidates: Vec<Candidate>,
    /// What STUN observed, if enabled.
    pub nat: NatObservation,
}

/// An answer bound to an invite's ride, certificate and route; exchange it privately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The particular ride being answered.
    pub ride: RideId,
    /// The host whose certificate will receive the admission credential.
    pub fingerprint: Fingerprint,
    /// The route this joiner has locally.
    pub route: RouteHash,
    /// SHA-256 of the joiner's per-ride client certificate. Admission requires TLS proof of
    /// its private key, which never leaves the joiner's computer; this value is not a secret.
    pub client: Fingerprint,
    /// At most eight distinct permitted destinations.
    pub candidates: Vec<Candidate>,
    /// What STUN observed, if enabled.
    pub nat: NatObservation,
}

/// Why a code cannot be accepted. Errors contain no supplied code or credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CodeError {
    /// The input exceeds a bound before decoding/allocating.
    #[error("The group code is too long.")]
    TooLong,
    /// The format is malformed, truncated, padded, duplicated or semantically invalid.
    #[error("The group code is incomplete or invalid.")]
    Invalid,
    /// The format version is unsupported.
    #[error("The group code uses an unsupported format.")]
    Version,
    /// An invite was supplied where a reply belongs, or vice versa.
    #[error("An invite and a reply were mixed up.")]
    WrongKind,
    /// A destination is forbidden by the receiver's policy.
    #[error("The group code contains a destination this network mode forbids.")]
    Destination,
    /// A reply belongs to another ride, certificate or route.
    #[error("The reply is for a different ride, host or course.")]
    WrongRide,
}

impl Invite {
    /// Validates and encodes the binary payload. No networking or randomness is performed.
    ///
    /// # Errors
    /// Invalid names, identifiers, counts or destinations return [`CodeError`].
    pub fn encode(&self, policy: AddressPolicy) -> Result<Vec<u8>, CodeError> {
        let mut out = header(INVITE, self.ride, self.fingerprint, self.route)?;
        put_name(&mut out, &self.host)?;
        put_name(&mut out, &self.course)?;
        put_candidates(&mut out, &self.candidates, policy)?;
        out.push(self.nat.byte());
        Ok(out)
    }

    /// How friends at other homes can probably reach this host.
    #[must_use]
    pub fn reachability(&self) -> Reachability {
        Reachability::of(&self.candidates, self.nat)
    }

    /// Decodes only bounded, complete invite payloads, checking every destination.
    ///
    /// # Errors
    /// Returns [`CodeError`] for any unsupported, forbidden or malformed input.
    pub fn decode(bytes: &[u8], policy: AddressPolicy) -> Result<Self, CodeError> {
        let mut reader = Reader::new(bytes, INVITE)?;
        let (ride, fingerprint, route) = reader.identity()?;
        let host = reader.name()?;
        let course = reader.name()?;
        let candidates = reader.candidates(policy)?;
        let nat = NatObservation::from_byte(reader.byte()?)?;
        reader.end()?;
        Ok(Self {
            ride,
            fingerprint,
            route,
            host,
            course,
            candidates,
            nat,
        })
    }
}

impl Reply {
    /// Refuses a reply for another invite before any destination is contacted.
    ///
    /// # Errors
    /// Returns [`CodeError::WrongRide`] if any invite binding differs.
    pub fn matches(&self, invite: &Invite) -> Result<(), CodeError> {
        if self.ride != invite.ride
            || self.fingerprint != invite.fingerprint
            || self.route != invite.route
        {
            return Err(CodeError::WrongRide);
        }
        Ok(())
    }

    /// How friends at other homes could probably reach this joiner if it became the host.
    #[must_use]
    pub fn reachability(&self) -> Reachability {
        Reachability::of(&self.candidates, self.nat)
    }

    /// Validates and encodes a reply, including the joiner's certificate pin.
    ///
    /// # Errors
    /// Invalid identifiers, counts or destinations return [`CodeError`].
    pub fn encode(&self, policy: AddressPolicy) -> Result<Vec<u8>, CodeError> {
        let mut out = header(REPLY, self.ride, self.fingerprint, self.route)?;
        if self.client.0 == [0; 32] {
            return Err(CodeError::Invalid);
        }
        out.extend(self.client.0);
        put_candidates(&mut out, &self.candidates, policy)?;
        out.push(self.nat.byte());
        Ok(out)
    }

    /// Decodes only bounded, complete reply payloads, checking every destination.
    ///
    /// # Errors
    /// Returns [`CodeError`] for any unsupported, forbidden or malformed input.
    pub fn decode(bytes: &[u8], policy: AddressPolicy) -> Result<Self, CodeError> {
        let mut reader = Reader::new(bytes, REPLY)?;
        let (ride, fingerprint, route) = reader.identity()?;
        let client = Fingerprint(reader.array()?);
        if client.0 == [0; 32] {
            return Err(CodeError::Invalid);
        }
        let candidates = reader.candidates(policy)?;
        let nat = NatObservation::from_byte(reader.byte()?)?;
        reader.end()?;
        Ok(Self {
            ride,
            fingerprint,
            route,
            client,
            candidates,
            nat,
        })
    }
}

fn header(
    kind: u8,
    ride: RideId,
    fingerprint: Fingerprint,
    route: RouteHash,
) -> Result<Vec<u8>, CodeError> {
    if ride.0 == [0; 16] || fingerprint.0 == [0; 32] {
        return Err(CodeError::Invalid);
    }
    let mut out = Vec::with_capacity(MAX_PAYLOAD_BYTES);
    out.extend([VERSION, kind]);
    out.extend(ride.0);
    out.extend(fingerprint.0);
    out.extend(route.0);
    Ok(out)
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name.trim() == name
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn put_name(out: &mut Vec<u8>, name: &str) -> Result<(), CodeError> {
    if !valid_name(name) {
        return Err(CodeError::Invalid);
    }
    out.push(u8::try_from(name.len()).map_err(|_| CodeError::Invalid)?);
    out.extend(name.as_bytes());
    Ok(())
}

fn put_candidates(
    out: &mut Vec<u8>,
    candidates: &[Candidate],
    policy: AddressPolicy,
) -> Result<(), CodeError> {
    if candidates.is_empty() || candidates.len() > MAX_CANDIDATES {
        return Err(CodeError::Invalid);
    }
    out.push(u8::try_from(candidates.len()).map_err(|_| CodeError::Invalid)?);
    for (i, candidate) in candidates.iter().enumerate() {
        Candidate::new(candidate.kind, candidate.addr, policy)?;
        if candidates[..i].iter().any(|c| c.addr == candidate.addr) {
            return Err(CodeError::Invalid);
        }
        out.push(candidate.kind.byte());
        match candidate.addr.ip() {
            IpAddr::V4(ip) => {
                out.push(4);
                out.extend(ip.octets());
            }
            IpAddr::V6(ip) => {
                out.push(6);
                out.extend(ip.octets());
            }
        }
        out.extend(candidate.addr.port().to_be_bytes());
    }
    Ok(())
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], kind: u8) -> Result<Self, CodeError> {
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(CodeError::TooLong);
        }
        let mut reader = Self(bytes);
        if reader.byte()? != VERSION {
            return Err(CodeError::Version);
        }
        if reader.byte()? != kind {
            return Err(CodeError::WrongKind);
        }
        Ok(reader)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CodeError> {
        let (head, rest) = self.0.split_at_checked(len).ok_or(CodeError::Invalid)?;
        self.0 = rest;
        Ok(head)
    }

    fn byte(&mut self) -> Result<u8, CodeError> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CodeError> {
        self.take(N)?.try_into().map_err(|_| CodeError::Invalid)
    }

    fn identity(&mut self) -> Result<(RideId, Fingerprint, RouteHash), CodeError> {
        let ride = RideId(self.array()?);
        let fingerprint = Fingerprint(self.array()?);
        let route = RouteHash(self.array()?);
        if ride.0 == [0; 16] || fingerprint.0 == [0; 32] {
            return Err(CodeError::Invalid);
        }
        Ok((ride, fingerprint, route))
    }

    fn name(&mut self) -> Result<String, CodeError> {
        let len = usize::from(self.byte()?);
        if len > MAX_NAME_BYTES {
            return Err(CodeError::Invalid);
        }
        let name = std::str::from_utf8(self.take(len)?).map_err(|_| CodeError::Invalid)?;
        if !valid_name(name) {
            return Err(CodeError::Invalid);
        }
        Ok(name.to_owned())
    }

    fn candidates(&mut self, policy: AddressPolicy) -> Result<Vec<Candidate>, CodeError> {
        let count = usize::from(self.byte()?);
        if !(1..=MAX_CANDIDATES).contains(&count) {
            return Err(CodeError::Invalid);
        }
        let mut candidates = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = CandidateKind::from_byte(self.byte()?)?;
            let ip = match self.byte()? {
                4 => IpAddr::V4(Ipv4Addr::from(self.array::<4>()?)),
                6 => IpAddr::V6(Ipv6Addr::from(self.array::<16>()?)),
                _ => return Err(CodeError::Invalid),
            };
            let addr = SocketAddr::new(ip, u16::from_be_bytes(self.array()?));
            let candidate = Candidate::new(kind, addr, policy)?;
            if candidates.iter().any(|c: &Candidate| c.addr == addr) {
                return Err(CodeError::Invalid);
            }
            candidates.push(candidate);
        }
        Ok(candidates)
    }

    fn end(self) -> Result<(), CodeError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(CodeError::Invalid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invite(policy: AddressPolicy) -> Invite {
        Invite {
            ride: RideId([1; 16]),
            fingerprint: Fingerprint([2; 32]),
            route: RouteHash([3; 32]),
            host: "Chloé".into(),
            course: "Col du Galibier".into(),
            candidates: vec![
                Candidate::new(
                    CandidateKind::Public,
                    "1.1.1.1:50000".parse().unwrap(),
                    policy,
                )
                .unwrap(),
            ],
            nat: NatObservation::OneServer,
        }
    }

    #[test]
    fn invite_and_reply_preserve_their_binding_and_bounded_data() {
        let policy = AddressPolicy::default();
        let invite = invite(policy);
        let reply = Reply {
            ride: invite.ride,
            fingerprint: invite.fingerprint,
            route: invite.route,
            client: Fingerprint([4; 32]),
            candidates: invite.candidates.clone(),
            nat: NatObservation::None,
        };
        assert_eq!(
            Invite::decode(&invite.encode(policy).unwrap(), policy),
            Ok(invite.clone())
        );
        assert_eq!(
            Reply::decode(&reply.encode(policy).unwrap(), policy),
            Ok(reply.clone())
        );
        assert_eq!(reply.matches(&invite), Ok(()));
        let mut changed = invite.clone();
        changed.ride.0[0] ^= 1;
        assert_eq!(reply.matches(&changed), Err(CodeError::WrongRide));
        changed = invite.clone();
        changed.fingerprint.0[0] ^= 1;
        assert_eq!(reply.matches(&changed), Err(CodeError::WrongRide));
        changed = invite;
        changed.route.0[0] ^= 1;
        assert_eq!(reply.matches(&changed), Err(CodeError::WrongRide));
        let mut unset = reply;
        unset.client = Fingerprint([0; 32]);
        assert_eq!(unset.encode(policy), Err(CodeError::Invalid));
    }

    #[test]
    fn malformed_payloads_are_refused_without_panics() {
        let policy = AddressPolicy::default();
        let bytes = invite(policy).encode(policy).unwrap();
        for len in 0..bytes.len() {
            assert!(
                Invite::decode(&bytes[..len], policy).is_err(),
                "length {len}"
            );
        }
        let mut padded = bytes.clone();
        padded.push(0);
        assert_eq!(Invite::decode(&padded, policy), Err(CodeError::Invalid));
        assert_eq!(
            Invite::decode(&[0; MAX_PAYLOAD_BYTES + 1], policy),
            Err(CodeError::TooLong)
        );
        assert_eq!(Reply::decode(&bytes, policy), Err(CodeError::WrongKind));
        let mut newer = bytes;
        newer[0] = VERSION + 1;
        assert_eq!(Invite::decode(&newer, policy), Err(CodeError::Version));
    }

    #[test]
    fn names_cannot_escape_the_terminal_or_exceed_the_character_bound() {
        let policy = AddressPolicy::default();
        for name in [
            String::new(),
            " ".into(),
            "Anna\nOther".into(),
            "\u{1b}]52;bad".into(),
            "A\u{202e}B".into(),
            "é".repeat(33),
        ] {
            let mut invite = invite(policy);
            invite.host = name;
            assert!(invite.encode(policy).is_err());
        }
        let mut invite = invite(policy);
        invite.host = "é".repeat(32);
        assert!(invite.encode(policy).is_ok());
    }

    #[test]
    fn changing_policy_or_relabelling_a_candidate_cannot_grant_local_access() {
        let local = AddressPolicy {
            lan: false,
            loopback: true,
        };
        let mut invite = invite(local);
        invite.candidates = vec![
            Candidate::new(
                CandidateKind::Local,
                "127.0.0.1:50000".parse().unwrap(),
                local,
            )
            .unwrap(),
        ];
        let bytes = invite.encode(local).unwrap();
        assert_eq!(
            Invite::decode(&bytes, AddressPolicy::default()),
            Err(CodeError::Destination)
        );
        assert_eq!(
            invite.encode(AddressPolicy::default()),
            Err(CodeError::Destination)
        );
        assert_eq!(
            Candidate::new(
                CandidateKind::Public,
                "127.0.0.1:50000".parse().unwrap(),
                local
            ),
            Err(CodeError::Destination)
        );
        assert_eq!(
            Candidate::new(
                CandidateKind::Local,
                "1.1.1.1:50000".parse().unwrap(),
                local
            ),
            Err(CodeError::Destination)
        );
    }

    #[test]
    fn reachability_follows_the_strongest_evidence_without_claiming_more() {
        let lan = AddressPolicy {
            lan: true,
            loopback: false,
        };
        let at = |kind, addr: &str| Candidate::new(kind, addr.parse().unwrap(), lan).unwrap();
        let home = at(CandidateKind::Local, "192.168.1.2:50000");
        let public = at(CandidateKind::Public, "1.1.1.1:50000");
        let mapped = at(CandidateKind::Stun, "1.1.1.1:50001");
        let v6 = at(CandidateKind::Public, "[2606:4700::1]:50000");
        let of = Reachability::of;
        assert_eq!(of(&[home], NatObservation::None), Reachability::Home);
        assert_eq!(
            of(&[home, public], NatObservation::None),
            Reachability::Direct
        );
        assert_eq!(
            of(&[home, mapped, v6], NatObservation::SameMapping),
            Reachability::Mapped
        );
        // Mappings that differ per server keep a STUN candidate but cannot reach friends.
        assert_eq!(
            of(&[mapped], NatObservation::DifferentMappings),
            Reachability::Strict
        );
        assert_eq!(
            of(&[mapped, v6], NatObservation::DifferentMappings),
            Reachability::Ipv6
        );
        assert_eq!(
            of(&[mapped], NatObservation::OneServer),
            Reachability::Unknown
        );
        assert_eq!(of(&[v6], NatObservation::None), Reachability::Ipv6);
    }

    #[test]
    fn callers_cannot_encode_empty_duplicate_or_excess_candidates() {
        let policy = AddressPolicy::default();
        let mut invite = invite(policy);
        let candidate = invite.candidates[0];
        for candidates in [
            vec![],
            vec![candidate; 2],
            vec![candidate; MAX_CANDIDATES + 1],
        ] {
            invite.candidates = candidates;
            assert_eq!(invite.encode(policy), Err(CodeError::Invalid));
        }
    }

    #[test]
    fn decoder_rejects_forged_names_counts_duplicates_and_address_aliases() {
        let policy = AddressPolicy::default();
        let invite = invite(policy);
        let bytes = invite.encode(policy).unwrap();
        let names_at = 2 + 16 + 32 + 32;
        let count_at = names_at + 1 + invite.host.len() + 1 + invite.course.len();
        let candidate_at = count_at + 1;

        let mut controls = bytes.clone();
        controls[names_at + 1] = 0x1b;
        assert_eq!(Invite::decode(&controls, policy), Err(CodeError::Invalid));
        let mut long_name = bytes[..names_at].to_vec();
        long_name.push(65);
        long_name.extend([b'A'; 65]);
        assert_eq!(Invite::decode(&long_name, policy), Err(CodeError::Invalid));

        let mut too_many = bytes.clone();
        too_many[count_at] = 9;
        assert_eq!(Invite::decode(&too_many, policy), Err(CodeError::Invalid));
        let mut duplicate = bytes[..candidate_at].to_vec();
        duplicate[count_at] = 2;
        duplicate.extend(&bytes[candidate_at..candidate_at + 8]);
        duplicate.extend(&bytes[candidate_at..]);
        assert_eq!(Invite::decode(&duplicate, policy), Err(CodeError::Invalid));

        let mut mapped = bytes[..candidate_at].to_vec();
        mapped.extend([CandidateKind::Public.byte(), 6]);
        mapped.extend("::ffff:127.0.0.1".parse::<Ipv6Addr>().unwrap().octets());
        mapped.extend(50000u16.to_be_bytes());
        mapped.push(NatObservation::None.byte());
        assert_eq!(
            Invite::decode(
                &mapped,
                AddressPolicy {
                    lan: true,
                    loopback: true
                }
            ),
            Err(CodeError::Destination)
        );
    }
}
