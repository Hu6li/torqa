//! Address discovery on a selected interface; private destinations require explicit consent.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout_at};

use crate::code::{Candidate, CandidateKind, NatObservation};
use crate::crypto::RandomError;
use crate::policy::{AddressPolicy, is_public};
use crate::stun;

/// Public address-discovery services; no ride state is sent to them.
pub const DEFAULT_STUN_SERVERS: [&str; 2] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478"];
/// Discovery is a short part of, never an extension to, the shared connection deadline.
pub const DISCOVERY_WINDOW: Duration = Duration::from_secs(3);

/// Explicit choices controlling discovery and advertised addresses. No router mappings are made.
#[derive(Clone, Debug)]
pub struct GatherOptions {
    /// LAN and local-test permissions; loopback mode is exclusively local and skips STUN.
    pub policy: AddressPolicy,
    /// OS interface name; the system's default interface when absent.
    pub interface: Option<String>,
    /// Up to four STUN servers. Empty means no STUN or DNS requests.
    pub stun_servers: Vec<String>,
}

impl Default for GatherOptions {
    fn default() -> Self {
        Self {
            policy: AddressPolicy::default(),
            interface: None,
            stun_servers: DEFAULT_STUN_SERVERS.map(str::to_owned).to_vec(),
        }
    }
}

/// A socket whose candidates were gathered before handing it to QUIC.
#[derive(Debug)]
pub struct BoundSocket {
    /// Owned socket bound to one selected local address; dropping it closes that socket.
    pub socket: UdpSocket,
    /// Validated destinations, at most a public/STUN and an explicitly permitted local address.
    pub candidates: Vec<Candidate>,
    /// Evidence from the configured STUN servers, not a guarantee about other destinations.
    pub nat: NatObservation,
    /// The server whose observed mapping is advertised; a host refreshes it while waiting.
    pub refresh: Option<SocketAddr>,
}

/// Discovery did not provide a socket. Failures contain no invite or admission secrets.
#[derive(Debug, thiserror::Error)]
pub enum GatherError {
    /// Too many/oversized options, or incompatible interface and local-test choices.
    #[error("The network discovery options are invalid.")]
    Options,
    /// The selected/default interface is unavailable or has no usable addresses.
    #[error(
        "No usable address on the selected network interface. Try a different interface, LAN mode or local-test mode."
    )]
    Interface,
    /// Discovery used the remaining connection time.
    #[error("Network discovery timed out.")]
    Timeout,
    /// Neither address family could open a socket.
    #[error("No UDP socket could be opened.")]
    Socket,
    /// Secure randomness was unavailable; no weak fallback is used.
    #[error(transparent)]
    Random(#[from] RandomError),
}

/// Opens at most two sockets bound to concrete addresses of one interface, and gathers their
/// candidates concurrently. Local-test mode uses only `127.0.0.1`/`::1` and never contacts STUN.
/// Operations share `deadline`, and discovery is additionally limited to three seconds.
///
/// # Errors
/// Returns [`GatherError`] when no socket can be opened or the remaining time expires.
pub async fn bind(
    options: &GatherOptions,
    deadline: Instant,
) -> Result<Vec<BoundSocket>, GatherError> {
    if options.stun_servers.len() > stun::MAX_SERVERS
        || options.stun_servers.iter().any(|s| s.len() > 260)
        || options.interface.as_ref().is_some_and(|s| s.len() > 128)
        || (options.policy.loopback && options.interface.is_some())
    {
        return Err(GatherError::Options);
    }
    let deadline = deadline.min(Instant::now() + DISCOVERY_WINDOW);
    if deadline <= Instant::now() {
        return Err(GatherError::Timeout);
    }
    let addresses = if options.policy.loopback {
        vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ]
    } else {
        let name = options.interface.clone();
        let lan = options.policy.lan;
        timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || selected_addresses(name.as_deref(), lan)),
        )
        .await
        .map_err(|_| GatherError::Timeout)?
        .map_err(|_| GatherError::Interface)??
    };
    let mut tasks = JoinSet::new();
    for ip in addresses {
        let options = options.clone();
        tasks.spawn(async move {
            let socket = timeout_at(deadline, UdpSocket::bind(SocketAddr::new(ip, 0)))
                .await
                .map_err(|_| GatherError::Timeout)?
                .map_err(|_| GatherError::Socket)?;
            let bound = socket.local_addr().map_err(|_| GatherError::Socket)?;
            let mut candidates = Vec::with_capacity(2);
            let kind = if is_public(ip) {
                CandidateKind::Public
            } else {
                CandidateKind::Local
            };
            if let Ok(candidate) = Candidate::new(kind, bound, options.policy) {
                candidates.push(candidate);
            }
            let seen = if options.policy.loopback {
                Vec::new()
            } else {
                stun::discover(&socket, &options.stun_servers, options.policy, deadline).await?
            };
            let mut refresh = None;
            for mapping in &seen {
                if let Ok(candidate) =
                    Candidate::new(CandidateKind::Stun, mapping.seen, options.policy)
                    && !candidates.iter().any(|c| c.addr() == mapping.seen)
                {
                    candidates.push(candidate);
                    refresh = Some(mapping.server);
                    break;
                }
            }
            let nat = observation(&seen.iter().map(|m| m.seen).collect::<Vec<_>>());
            Ok(BoundSocket {
                socket,
                candidates,
                nat,
                refresh,
            })
        });
    }
    let mut bound = Vec::with_capacity(2);
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(socket)) => bound.push(socket),
            Ok(Err(GatherError::Random(error))) => return Err(error.into()),
            _ => {}
        }
    }
    if bound.is_empty() {
        return Err(GatherError::Socket);
    }
    // IPv6 first, as Happy Eyeballs does (RFC 8305): carrier NAT often shares a rider's IPv4
    // address while IPv6 stays directly reachable. Deterministic whichever finishes first.
    bound.sort_by_key(|path| path.socket.local_addr().is_ok_and(|a| a.is_ipv4()));
    Ok(bound)
}

fn selected_addresses(name: Option<&str>, lan: bool) -> Result<Vec<IpAddr>, GatherError> {
    let interface = match name {
        Some(name) => netdev::get_interfaces()
            .into_iter()
            .find(|i| i.name == name)
            .ok_or(GatherError::Interface)?,
        None => netdev::get_default_interface().map_err(|_| GatherError::Interface)?,
    };
    if !interface.is_up() || interface.is_loopback() {
        return Err(GatherError::Interface);
    }
    let addresses = interface_addresses(&interface, lan);
    if addresses.is_empty() {
        return Err(GatherError::Interface);
    }
    Ok(addresses)
}

fn interface_addresses(interface: &netdev::Interface, lan: bool) -> Vec<IpAddr> {
    let v4 = interface
        .ipv4
        .iter()
        .map(netdev::ipnet::Ipv4Net::addr)
        .find(|a| {
            !a.is_loopback() && !a.is_unspecified() && !a.is_link_local() && !a.is_multicast()
        });
    let mut v6 = interface
        .ipv6
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            interface
                .ipv6_addr_flags
                .get(*i)
                .is_none_or(|f| !f.deprecated && !f.tentative)
        })
        .map(|(i, a)| {
            (
                interface
                    .ipv6_addr_flags
                    .get(i)
                    .is_some_and(|f| f.temporary),
                a.addr(),
            )
        })
        .filter(|(_, a)| is_public(IpAddr::V6(*a)) || (lan && a.is_unique_local()))
        .collect::<Vec<_>>();
    v6.sort_by_key(|(temporary, a)| (!is_public(IpAddr::V6(*a)), !*temporary));
    v4.map(IpAddr::V4)
        .into_iter()
        .chain(v6.first().map(|(_, a)| IpAddr::V6(*a)))
        .collect()
}

/// Reports only what the queried destinations observed, without claiming to identify a NAT.
#[must_use]
pub fn observation(seen: &[SocketAddr]) -> NatObservation {
    match seen {
        [] => NatObservation::None,
        [_] => NatObservation::OneServer,
        [first, rest @ ..] if rest.iter().all(|a| a == first) => NatObservation::SameMapping,
        _ => NatObservation::DifferentMappings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_test_mode_never_uses_configured_stun_servers_and_binds_only_loopback() {
        let listener = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let options = GatherOptions {
            policy: AddressPolicy {
                lan: false,
                loopback: true,
            },
            interface: None,
            stun_servers: vec![listener.local_addr().unwrap().to_string()],
        };
        let paths = bind(&options, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(!paths.is_empty());
        for path in paths {
            assert!(path.socket.local_addr().unwrap().ip().is_loopback());
            assert_eq!(path.nat, NatObservation::None);
            assert!(path.candidates.iter().all(|c| c.addr().ip().is_loopback()));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(20), listener.recv_from(&mut [0; 64]))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn expired_deadlines_and_excess_options_fail_before_opening_sockets() {
        let mut options = GatherOptions::default();
        assert!(matches!(
            bind(&options, Instant::now()).await,
            Err(GatherError::Timeout)
        ));
        options.stun_servers = vec!["1.1.1.1:3478".into(); stun::MAX_SERVERS + 1];
        assert!(matches!(
            bind(&options, Instant::now() + Duration::from_secs(1)).await,
            Err(GatherError::Options)
        ));
    }

    #[test]
    fn observations_are_not_overstated_as_a_guaranteed_nat_type() {
        let a = "1.1.1.1:50000".parse().unwrap();
        let b = "1.1.1.1:50001".parse().unwrap();
        assert_eq!(observation(&[]), NatObservation::None);
        assert_eq!(observation(&[a]), NatObservation::OneServer);
        assert_eq!(observation(&[a, a]), NatObservation::SameMapping);
        assert_eq!(observation(&[a, b]), NatObservation::DifferentMappings);
    }

    #[test]
    fn only_one_address_per_family_is_selected_and_link_local_ipv6_is_excluded() {
        let mut interface = netdev::Interface::dummy();
        interface.ipv4 = vec![
            "192.168.1.2/24".parse().unwrap(),
            "10.0.0.2/24".parse().unwrap(),
        ];
        interface.ipv6 = vec![
            "fe80::1/64".parse().unwrap(),
            "2606:4700::1234/64".parse().unwrap(),
            "fd00::1/64".parse().unwrap(),
        ];
        assert_eq!(
            interface_addresses(&interface, false),
            vec![
                "192.168.1.2".parse::<IpAddr>().unwrap(),
                "2606:4700::1234".parse().unwrap()
            ]
        );
    }
}
