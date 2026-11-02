//! Offline connection choices. Previewing an invitation never starts discovery or probes.

use orqwa_net::code::{Invite, Reply, RouteHash};
use orqwa_net::policy::AddressPolicy;
use orqwa_net::stun;
use orqwa_storage::computer;

use super::{ConnectOptions, error};
use crate::{App, AppError, Failure, Kind};

pub use orqwa_net::discovery::DEFAULT_STUN_SERVERS;

/// Whether `text` names servers [`App::set_stun_servers`] takes, checked as the rider types.
#[must_use]
pub fn stun_servers_valid(text: &str) -> bool {
    stun::servers(text).is_ok()
}

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

    /// Creates the existing bounded transport options after explicit confirmation. A lookup
    /// asks `servers` instead of ORQWA's own unless that is empty (R192). Home mode performs
    /// no public address discovery, even if lookup was previously selected.
    #[must_use]
    pub fn options(self, lookup: bool, servers: &[String]) -> ConnectOptions {
        let mut options = ConnectOptions::default();
        options.gather.policy.lan = self != Self::Internet;
        if self == Self::Home || !lookup {
            options.gather.stun_servers.clear();
        } else if !servers.is_empty() {
            options.gather.stun_servers = servers.to_vec();
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

/// Inspection only: connecting rechecks the policy the rider chose, and transport repeats that
/// check before any operation.
const INSPECTION: AddressPolicy = AddressPolicy {
    lan: true,
    loopback: false,
};

/// Finding a code in a paste reads any address: what it permits is checked when it is used.
const EXTRACTION: AddressPolicy = AddressPolicy {
    lan: true,
    loopback: true,
};

/// The codes `text` may be, as riders paste them (R195): alone, wrapped over lines by a
/// messenger, or inside a message ("here: orqwa:… thanks"). The whole text comes first, so a
/// wrapped code keeps all its lines, then each word that begins like a code.
fn candidates(text: &str) -> impl Iterator<Item = &str> {
    let whole = text.trim();
    std::iter::once(whole).chain(
        text.split_whitespace()
            .filter(move |word| word.starts_with("orqwa:") && *word != whole),
    )
}

/// The invitation in a paste, ready to read; `None` if it holds none.
#[must_use]
pub fn pasted_invitation(text: &str) -> Option<&str> {
    candidates(text).find(|code| Invite::from_code(code, EXTRACTION).is_ok())
}

/// The reply in a paste, ready to read; `None` if it holds none.
#[must_use]
pub fn pasted_reply(text: &str) -> Option<&str> {
    candidates(text).find(|code| Reply::from_code(code, EXTRACTION).is_ok())
}

/// Which group code a paste holds, to tell riders who pasted the other one (R195).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeKind {
    /// An organizer's invitation.
    Invitation,
    /// A friend's reply to one.
    Reply,
    /// Neither.
    None,
}
impl CodeKind {
    /// Stable presentation key; empty for none.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Invitation => "invitation",
            Self::Reply => "reply",
            Self::None => "",
        }
    }
}

/// Which group code `text` holds, read offline without reporting anything.
#[must_use]
pub fn code_kind(text: &str) -> CodeKind {
    if pasted_invitation(text).is_some() {
        CodeKind::Invitation
    } else if pasted_reply(text).is_some() {
        CodeKind::Reply
    } else {
        CodeKind::None
    }
}

/// Validates a pasted lobby invitation entirely offline, without granting its permissions.
///
/// # Errors
/// Returns [`AppError`] for an invalid, oversized, incompatible or non-lobby invitation.
pub fn invitation(text: &str) -> Result<InvitationPreview, AppError> {
    let code = pasted_invitation(text).unwrap_or(text);
    let parsed = Invite::from_code(code, INSPECTION).map_err(error)?;
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
    /// The address lookup servers named for this computer's group rides (R192), each as
    /// discovery resolves it; empty while ORQWA asks its own. Entries of a hand-edited file
    /// that cannot be asked are left out, and a fifth onwards, as discovery refuses them.
    #[must_use]
    pub fn stun_servers(&self) -> Vec<String> {
        let mut servers: Vec<String> = Vec::new();
        for server in computer::stun_servers(&self.computer) {
            if let Ok(server) = stun::server(&server)
                && !servers.contains(&server)
                && servers.len() < stun::MAX_SERVERS
            {
                servers.push(server);
            }
        }
        servers
    }

    /// Names the address lookup servers this computer's group rides ask from now on, read
    /// from `text` as [`stun::servers`] reads it; empty text goes back to ORQWA's own.
    ///
    /// # Errors
    /// [`AppError`] if an address cannot be asked or more than four are named, keeping the
    /// servers named before, or if they cannot be saved.
    pub fn set_stun_servers(&mut self, text: &str) -> Result<(), AppError> {
        let servers = stun::servers(text).map_err(error)?;
        computer::set_stun_servers(&self.computer, &servers)
            .map_err(|e| AppError::Reported(Failure::new(Kind::SaveSetting, e)))
    }

    fn connect_options(&self, mode: ConnectionMode, lookup: bool) -> ConnectOptions {
        mode.options(lookup, &self.stun_servers())
    }

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
        let options = self.connect_options(mode, lookup);
        let invite = invite.map(|text| pasted_invitation(&text).unwrap_or(&text).to_owned());
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
        let internet = ConnectionMode::Internet.options(true, &[]);
        assert!(!internet.gather.policy.lan && !internet.gather.policy.loopback);
        assert_ne!(internet.gather.stun_servers, Vec::<String>::new());
        let home = ConnectionMode::Home.options(true, &[]);
        assert!(home.gather.policy.lan && !home.gather.policy.loopback);
        assert_eq!(home.gather.stun_servers, Vec::<String>::new());
        let mixed = ConnectionMode::Mixed.options(true, &[]);
        assert!(mixed.gather.policy.lan && !mixed.gather.stun_servers.is_empty());
        for mode in [
            ConnectionMode::Internet,
            ConnectionMode::Home,
            ConnectionMode::Mixed,
        ] {
            assert_eq!(
                mode.options(false, &[]).gather.stun_servers,
                Vec::<String>::new()
            );
            assert!(!mode.options(false, &[]).gather.policy.loopback);
        }
        assert!(ConnectionMode::from_key("unexpected").is_none());
    }

    #[test]
    fn named_lookup_servers_replace_the_defaults_only_where_a_lookup_is_made() {
        let named = vec!["stun.example.org:3478".to_owned()];
        for mode in [ConnectionMode::Internet, ConnectionMode::Mixed] {
            assert_eq!(mode.options(true, &named).gather.stun_servers, named);
            assert_eq!(
                mode.options(false, &named).gather.stun_servers,
                Vec::<String>::new()
            );
        }
        assert_eq!(
            ConnectionMode::Home
                .options(true, &named)
                .gather
                .stun_servers,
            Vec::<String>::new()
        );
    }

    #[test]
    fn named_lookup_servers_are_kept_for_the_computer_until_cleared() {
        let token = orqwa_net::code::RideId::generate().unwrap();
        let dir = std::env::temp_dir().join(format!("orqwa-lookup-servers-{:x?}", token.0));
        let app = || App::new(dir.join("data"), dir.join("cache")).unwrap();
        let mut first = app();
        let defaults = ConnectionMode::Internet
            .options(true, &[])
            .gather
            .stun_servers;
        assert_eq!(first.stun_servers(), Vec::<String>::new());
        assert_eq!(
            first
                .connect_options(ConnectionMode::Internet, true)
                .gather
                .stun_servers,
            defaults
        );

        first
            .set_stun_servers("stun:stun.example.org, 192.0.2.1:3479")
            .unwrap();
        let named = vec![
            "stun.example.org:3478".to_owned(),
            "192.0.2.1:3479".to_owned(),
        ];
        let mut again = app();
        assert_eq!(again.stun_servers(), named, "kept for the next start");
        assert_eq!(
            again
                .connect_options(ConnectionMode::Mixed, true)
                .gather
                .stun_servers,
            named
        );
        assert!(again.set_stun_servers("stun.example.org:0").is_err());
        assert!(
            again
                .set_stun_servers("a.example b.example c.example d.example e.example")
                .is_err()
        );
        assert_eq!(
            again.stun_servers(),
            named,
            "a mistake keeps what was named"
        );

        again.set_stun_servers(" ").unwrap();
        assert_eq!(again.stun_servers(), Vec::<String>::new());
        assert_eq!(
            again
                .connect_options(ConnectionMode::Internet, true)
                .gather
                .stun_servers,
            defaults
        );
        drop((first, again));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pasted_codes_are_found_alone_wrapped_or_in_a_message_and_told_apart() {
        let invite = Invite {
            ride: RideId([1; 16]),
            fingerprint: Fingerprint([2; 32]),
            route: RouteHash::LOBBY,
            host: "Alex".into(),
            course: "Group ride".into(),
            candidates: vec![
                Candidate::new(
                    CandidateKind::Public,
                    "1.1.1.1:50000".parse().unwrap(),
                    INSPECTION,
                )
                .unwrap(),
            ],
            nat: NatObservation::None,
        };
        let reply = Reply {
            ride: invite.ride,
            fingerprint: invite.fingerprint,
            route: invite.route,
            client: Fingerprint([3; 32]),
            candidates: invite.candidates.clone(),
            nat: NatObservation::None,
        };
        let invitation_code = invite.to_code(INSPECTION).unwrap();
        let reply_code = reply.to_code(INSPECTION).unwrap();
        assert_eq!(code_kind(&invitation_code), CodeKind::Invitation);
        assert_eq!(code_kind(&reply_code), CodeKind::Reply);
        assert_eq!(code_kind("see you at seven"), CodeKind::None);
        // Pasted with the message it came in.
        let message = format!("Join us tonight: {invitation_code} See you!");
        assert_eq!(pasted_invitation(&message), Some(invitation_code.as_str()));
        assert_eq!(invitation(&message).unwrap().organizer, "Alex");
        let answer = format!("Here is mine:\n{reply_code}\nthanks");
        assert_eq!(pasted_reply(&answer), Some(reply_code.as_str()));
        assert_eq!(code_kind(&answer), CodeKind::Reply);
        // Wrapped over lines by a messenger.
        let wrapped: String = invitation_code
            .as_bytes()
            .chunks(40)
            .map(|line| std::str::from_utf8(line).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(code_kind(&wrapped), CodeKind::Invitation);
        assert!(
            pasted_reply(&invitation_code).is_none(),
            "an invitation is no reply"
        );
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
            Invite::from_code(
                &code,
                ConnectionMode::Internet.options(true, &[]).gather.policy
            )
            .is_err(),
            "inspection cannot grant LAN access"
        );
        assert!(invitation("orqwa:invalid").is_err());
        assert!(invitation(&"x".repeat(orqwa_net::code::MAX_PASTE_BYTES + 1)).is_err());
        invite.route = RouteHash([9; 32]);
        assert!(invitation(&invite.to_code(policy).unwrap()).is_err());
    }
}
