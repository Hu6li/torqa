//! Offline connection choices. Previewing an invitation never starts discovery or probes.

use orqwa_net::code::{Invite, RouteHash};
use orqwa_net::policy::AddressPolicy;

use super::{ConnectOptions, error};
use crate::{App, AppError};

/// A rider-facing description of where the group is riding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionMode {
    /// Friends use separate home internet connections; LAN access stays disabled.
    Internet,
    /// Friends share a router; public address lookup is unnecessary.
    Home,
    /// Some friends share a router and others use the internet.
    Mixed,
}
impl ConnectionMode {
    /// Decodes a stable presentation key without changing any network permission.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "internet" => Some(Self::Internet),
            "home" => Some(Self::Home),
            "mixed" => Some(Self::Mixed),
            _ => None,
        }
    }

    /// Creates the existing bounded transport options after explicit confirmation.
    /// Home mode performs no public address discovery, even if lookup was previously selected.
    #[must_use]
    pub fn options(self, lookup: bool) -> ConnectOptions {
        let mut options = ConnectOptions::default();
        options.gather.policy.lan = self != Self::Internet;
        if self == Self::Home || !lookup {
            options.gather.stun_servers.clear();
        }
        options
    }
}

/// Bounded invitation advice containing neither connection addresses nor credentials.
pub struct InvitationPreview {
    /// Organizer name validated by the existing invitation parser.
    pub organizer: String,
    /// The invitation includes local destinations requiring explicit home-network permission.
    pub requires_home: bool,
}

/// Validates a pasted lobby invitation entirely offline, without granting its permissions.
///
/// # Errors
/// Returns [`AppError`] for an invalid, oversized, incompatible or non-lobby invitation.
pub fn invitation(text: &str) -> Result<InvitationPreview, AppError> {
    // This permissive parse is inspection only. group_connect rechecks the actual chosen
    // policy before creating an owner; transport repeats that check before any operation.
    let parsed = Invite::from_code(
        text,
        AddressPolicy {
            lan: true,
            loopback: false,
        },
    )
    .map_err(error)?;
    if parsed.route != RouteHash::LOBBY {
        return Err(error("Ask the organizer for a group ride invitation."));
    }
    let requires_home = parsed
        .candidates
        .iter()
        .any(|candidate| !AddressPolicy::default().permits(candidate.addr()));
    Ok(InvitationPreview {
        organizer: parsed.host,
        requires_home,
    })
}

impl App {
    /// Confirms a guided connection choice before starting any discovery or admission.
    ///
    /// # Errors
    /// Returns [`AppError`] for incompatible invitation permissions or an unavailable group.
    pub fn group_connect(
        &mut self,
        invite: Option<String>,
        mode: ConnectionMode,
        lookup: bool,
    ) -> Result<(), AppError> {
        let options = mode.options(lookup);
        if let Some(text) = &invite {
            let parsed = Invite::from_code(text, options.gather.policy).map_err(error)?;
            if parsed.route != RouteHash::LOBBY {
                return Err(error("Ask the organizer for a group ride invitation."));
            }
        }
        self.group_open(invite, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orqwa_net::code::{Candidate, CandidateKind, Fingerprint, NatObservation, RideId};

    #[test]
    fn home_and_internet_choices_keep_discovery_and_local_permissions_explicit() {
        let internet = ConnectionMode::Internet.options(true);
        assert!(!internet.gather.policy.lan && !internet.gather.policy.loopback);
        assert_ne!(internet.gather.stun_servers, Vec::<String>::new());
        let home = ConnectionMode::Home.options(true);
        assert!(home.gather.policy.lan && !home.gather.policy.loopback);
        assert_eq!(home.gather.stun_servers, Vec::<String>::new());
        let mixed = ConnectionMode::Mixed.options(true);
        assert!(mixed.gather.policy.lan && !mixed.gather.stun_servers.is_empty());
        for mode in [
            ConnectionMode::Internet,
            ConnectionMode::Home,
            ConnectionMode::Mixed,
        ] {
            assert_eq!(
                mode.options(false).gather.stun_servers,
                Vec::<String>::new()
            );
            assert!(!mode.options(false).gather.policy.loopback);
        }
        assert!(ConnectionMode::from_key("unexpected").is_none());
    }

    #[test]
    fn offline_preview_validates_codes_without_turning_advice_into_permission() {
        let policy = AddressPolicy {
            lan: true,
            loopback: false,
        };
        let mut invite = Invite {
            ride: RideId([1; 16]),
            fingerprint: Fingerprint([2; 32]),
            route: RouteHash::LOBBY,
            host: "Alex".into(),
            course: "Group ride".into(),
            candidates: vec![
                Candidate::new(
                    CandidateKind::Public,
                    "1.1.1.1:50000".parse().unwrap(),
                    policy,
                )
                .unwrap(),
            ],
            nat: NatObservation::None,
        };
        let code = invite.to_code(policy).unwrap();
        let preview = invitation(&code).unwrap();
        assert_eq!(preview.organizer, "Alex");
        assert!(!preview.requires_home);
        invite.candidates.push(
            Candidate::new(
                CandidateKind::Local,
                "192.168.1.2:50000".parse().unwrap(),
                policy,
            )
            .unwrap(),
        );
        let code = invite.to_code(policy).unwrap();
        assert!(invitation(&code).unwrap().requires_home);
        assert!(
            Invite::from_code(&code, ConnectionMode::Internet.options(true).gather.policy).is_err(),
            "inspection cannot grant LAN access"
        );
        assert!(invitation("orqwa:invalid").is_err());
        assert!(invitation(&"x".repeat(orqwa_net::code::MAX_PASTE_BYTES + 1)).is_err());
        invite.route = RouteHash([9; 32]);
        assert!(invitation(&invite.to_code(policy).unwrap()).is_err());
    }
}
