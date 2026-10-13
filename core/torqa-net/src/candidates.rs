//! The addresses a rider may be reached at (ADR 0015): its own, the public address a STUN
//! server sees and a port its router maps, gathered for the socket the ride will use.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;

use crate::router::{self, RouterMapping};
use crate::stun;

/// Public STUN servers asked by default: free, run by large operators, and two of them so that a
/// router giving each destination its own port shows (see [`NatBehavior`]).
pub const DEFAULT_STUN_SERVERS: [&str; 2] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478"];

/// An address a rider may be reached at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Where it comes from.
    pub kind: CandidateKind,
    /// The address.
    pub addr: SocketAddr,
}

/// Where a candidate address comes from, the most likely to work first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    /// A port the rider's router forwards (`UPnP`, NAT-PMP or PCP): open to anyone.
    Router,
    /// An address of the rider's own that is public: IPv6, or IPv4 without a router between.
    Public,
    /// The public address a STUN server saw the rider's packets come from.
    Stun,
    /// A private address, reachable on the same network only.
    Local,
}

impl CandidateKind {
    pub(crate) fn to_byte(self) -> u8 {
        match self {
            Self::Router => 1,
            Self::Public => 2,
            Self::Stun => 3,
            Self::Local => 4,
        }
    }

    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Router),
            2 => Some(Self::Public),
            3 => Some(Self::Stun),
            4 => Some(Self::Local),
            _ => None,
        }
    }
}

/// How the rider's router maps the ride's socket, as far as STUN shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NatBehavior {
    /// Every STUN server saw the same address: the router keeps one port for all destinations,
    /// so others can punch through to it.
    EndpointIndependent,
    /// STUN servers saw different ports: the router picks one per destination. Punching
    /// through it works only towards a rider whose own router does not.
    Symmetric,
    /// One server answered: no way to tell.
    Unknown,
    /// No STUN server answered, or none was asked.
    NoAnswer,
}

impl NatBehavior {
    pub(crate) fn to_byte(self) -> u8 {
        match self {
            Self::EndpointIndependent => 1,
            Self::Symmetric => 2,
            Self::Unknown => 3,
            Self::NoAnswer => 4,
        }
    }

    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::EndpointIndependent),
            2 => Some(Self::Symmetric),
            3 => Some(Self::Unknown),
            4 => Some(Self::NoAnswer),
            _ => None,
        }
    }
}

/// What to gather.
#[derive(Clone, Debug)]
pub struct GatherOptions {
    /// STUN servers (`host:port`) to ask; none to ask nobody (R68).
    pub stun_servers: Vec<String>,
    /// Whether to ask the router for a port.
    pub router: bool,
    /// How long to wait for STUN servers and the router, together.
    pub timeout: Duration,
    /// Also offer the loopback address, for two Torqas on one computer (tests, trying it out).
    pub loopback: bool,
}

impl Default for GatherOptions {
    fn default() -> Self {
        Self {
            stun_servers: DEFAULT_STUN_SERVERS.map(String::from).to_vec(),
            router: true,
            timeout: Duration::from_secs(3),
            loopback: false,
        }
    }
}

/// The candidates of a socket and what was learned on the way.
#[derive(Debug)]
pub struct Gathered {
    /// The candidates, the most likely to work first, without duplicates.
    pub candidates: Vec<Candidate>,
    /// How the router maps the socket.
    pub nat: NatBehavior,
    /// The port the router forwards, to release when the ride is over.
    pub mapping: Option<RouterMapping>,
}

/// Gathers the candidates of `socket`, for its address family. STUN is asked on the socket
/// itself, so it must not be receiving anything else meanwhile.
pub async fn gather(socket: &UdpSocket, options: &GatherOptions) -> Gathered {
    let Ok(bound) = socket.local_addr() else {
        return Gathered {
            candidates: Vec::new(),
            nat: NatBehavior::NoAnswer,
            mapping: None,
        };
    };
    let addresses = interface_addresses();
    let mut own = own_candidates(&addresses, bound);
    if options.loopback {
        let ip = if bound.is_ipv4() {
            IpAddr::from([127, 0, 0, 1])
        } else {
            IpAddr::from(std::net::Ipv6Addr::LOCALHOST)
        };
        own.push(Candidate {
            kind: CandidateKind::Local,
            addr: SocketAddr::new(ip, bound.port()),
        });
    }
    // IPv6 has no NAT to learn about: without a public address of its own, nobody is reached
    // over it, and STUN servers would only be waited for.
    if bound.is_ipv6() && own.iter().all(|c| c.kind != CandidateKind::Public) {
        return Gathered {
            candidates: own,
            nat: NatBehavior::NoAnswer,
            mapping: None,
        };
    }
    let local_v4 = addresses.iter().copied().find(|ip| match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(_) => false,
    });
    let stun = stun::public_addresses(socket, &options.stun_servers, options.timeout);
    let router = async {
        match local_v4 {
            Some(ip) if options.router && bound.is_ipv4() => {
                router::map_port(SocketAddr::new(ip, bound.port()), options.timeout).await
            }
            _ => None,
        }
    };
    let (seen, mapping) = tokio::join!(stun, router);

    let mut candidates = own;
    if let Some(&first) = seen.first() {
        candidates.push(Candidate {
            kind: CandidateKind::Stun,
            addr: first,
        });
    }
    if let Some(mapping) = &mapping {
        candidates.push(Candidate {
            kind: CandidateKind::Router,
            addr: mapping.external(),
        });
    }
    Gathered {
        candidates: ordered(candidates),
        nat: nat_behavior(&seen),
        mapping,
    }
}

/// The addresses of the computer's network interfaces that are up, without loopback.
fn interface_addresses() -> Vec<IpAddr> {
    let interfaces = netdev::get_interfaces();
    let default = interfaces.iter().position(|i| i.default);
    // The default interface's addresses first: they reach the internet.
    let order = default
        .into_iter()
        .chain((0..interfaces.len()).filter(|&i| Some(i) != default));
    order
        .map(|i| &interfaces[i])
        .filter(|i| i.is_up() && !i.is_loopback())
        .flat_map(|i| {
            let v6 = i
                .ipv6
                .iter()
                .zip(
                    i.ipv6_addr_flags
                        .iter()
                        .map(Some)
                        .chain(std::iter::repeat(None)),
                )
                .filter(|(_, flags)| flags.is_none_or(|f| !f.deprecated && !f.tentative))
                .map(|(net, _)| IpAddr::V6(net.addr()));
            i.ipv4.iter().map(|net| IpAddr::V4(net.addr())).chain(v6)
        })
        .collect()
}

/// The candidates from the computer's own addresses that a socket bound to `bound` can use.
fn own_candidates(addresses: &[IpAddr], bound: SocketAddr) -> Vec<Candidate> {
    addresses
        .iter()
        .filter(|ip| ip.is_ipv4() == bound.is_ipv4())
        .filter(|ip| bound.ip().is_unspecified() || **ip == bound.ip())
        .filter_map(|&ip| {
            let kind = match ip {
                IpAddr::V4(v4) if v4.is_loopback() || v4.is_link_local() => return None,
                // Link-local addresses need the interface named, which a code cannot carry.
                IpAddr::V6(v6) if v6.is_loopback() || v6.is_unicast_link_local() => return None,
                _ if is_private(ip) => CandidateKind::Local,
                _ => CandidateKind::Public,
            };
            Some(Candidate {
                kind,
                addr: SocketAddr::new(ip, bound.port()),
            })
        })
        .collect()
}

/// Addresses only reachable nearby: private IPv4, carrier-grade NAT's shared space
/// (100.64.0.0/10, RFC 6598) and unique local IPv6.
pub(crate) fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || (v4.octets()[0] == 100 && v4.octets()[1] & 0xc0 == 64),
        IpAddr::V6(v6) => v6.is_unique_local(),
    }
}

/// Sorted by kind, keeping each address once (a router's port may be the one STUN saw).
fn ordered(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by_key(|c| c.kind);
    let mut seen = Vec::new();
    candidates.retain(|c| {
        let new = !seen.contains(&c.addr);
        seen.push(c.addr);
        new
    });
    candidates
}

fn nat_behavior(seen: &[SocketAddr]) -> NatBehavior {
    match seen {
        [] => NatBehavior::NoAnswer,
        [_] => NatBehavior::Unknown,
        [first, rest @ ..] if rest.iter().all(|a| a == first) => NatBehavior::EndpointIndependent,
        _ => NatBehavior::Symmetric,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn own_addresses_are_classed_as_reachable_from_anywhere_or_only_nearby() {
        let addresses = ips(&[
            "192.168.1.20",
            "100.64.0.7",
            "203.0.113.5",
            "169.254.3.3",
            "2001:db8::20",
            "fd00::20",
            "fe80::1",
        ]);
        let v4 = own_candidates(&addresses, "0.0.0.0:5000".parse().unwrap());
        assert_eq!(
            v4,
            [
                (CandidateKind::Local, "192.168.1.20:5000"),
                (CandidateKind::Local, "100.64.0.7:5000"),
                (CandidateKind::Public, "203.0.113.5:5000"),
            ]
            .map(|(kind, a)| Candidate {
                kind,
                addr: a.parse().unwrap()
            })
        );
        let v6 = own_candidates(&addresses, "[::]:5000".parse().unwrap());
        assert_eq!(
            v6,
            [
                (CandidateKind::Public, "[2001:db8::20]:5000"),
                (CandidateKind::Local, "[fd00::20]:5000"),
            ]
            .map(|(kind, a)| Candidate {
                kind,
                addr: a.parse().unwrap()
            })
        );
    }

    #[test]
    fn a_socket_bound_to_one_address_offers_only_that_one() {
        let addresses = ips(&["192.168.1.20", "10.0.0.3"]);
        let only = own_candidates(&addresses, "10.0.0.3:5000".parse().unwrap());
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].addr, "10.0.0.3:5000".parse().unwrap());
    }

    #[test]
    fn the_most_likely_to_work_come_first_and_each_address_once() {
        let c = |kind, a: &str| Candidate {
            kind,
            addr: a.parse().unwrap(),
        };
        let sorted = ordered(vec![
            c(CandidateKind::Local, "192.168.1.20:5000"),
            c(CandidateKind::Stun, "203.0.113.9:5000"),
            c(CandidateKind::Router, "203.0.113.9:5000"),
            c(CandidateKind::Public, "[2001:db8::20]:5000"),
        ]);
        assert_eq!(
            sorted,
            [
                c(CandidateKind::Router, "203.0.113.9:5000"),
                c(CandidateKind::Public, "[2001:db8::20]:5000"),
                c(CandidateKind::Local, "192.168.1.20:5000"),
            ]
        );
    }

    #[test]
    fn stun_answers_tell_how_the_router_maps() {
        let a: SocketAddr = "203.0.113.9:5000".parse().unwrap();
        let b: SocketAddr = "203.0.113.9:5001".parse().unwrap();
        assert_eq!(nat_behavior(&[]), NatBehavior::NoAnswer);
        assert_eq!(nat_behavior(&[a]), NatBehavior::Unknown);
        assert_eq!(nat_behavior(&[a, a]), NatBehavior::EndpointIndependent);
        assert_eq!(nat_behavior(&[a, b]), NatBehavior::Symmetric);
    }

    #[tokio::test]
    async fn gathers_the_public_address_a_stun_server_sees() {
        let server = stun::testing::server(0).await;
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let options = GatherOptions {
            stun_servers: vec![server.to_string()],
            router: false,
            timeout: Duration::from_secs(2),
            loopback: false,
        };
        let gathered = gather(&socket, &options).await;
        let own = socket.local_addr().unwrap();
        assert!(gathered.candidates.contains(&Candidate {
            kind: CandidateKind::Stun,
            addr: own,
        }));
        assert_eq!(gathered.nat, NatBehavior::Unknown);
        assert!(gathered.mapping.is_none());
    }

    #[tokio::test]
    async fn asks_nobody_when_stun_is_switched_off() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let options = GatherOptions {
            stun_servers: Vec::new(),
            router: false,
            timeout: Duration::from_secs(2),
            loopback: false,
        };
        let gathered = gather(&socket, &options).await;
        assert!(
            gathered
                .candidates
                .iter()
                .all(|c| c.kind != CandidateKind::Stun)
        );
        assert_eq!(gathered.nat, NatBehavior::NoAnswer);
    }
}
