//! Which destinations a pasted peer code may cause this computer to contact (R160, R161).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Permissions for peer candidates; discovery services have their own explicit server list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AddressPolicy {
    /// Permit RFC 1918 and unique-local IPv6 addresses for a deliberate LAN ride.
    pub lan: bool,
    /// Permit loopback for a deliberate same-computer test, independently of LAN access.
    pub loopback: bool,
}

impl AddressPolicy {
    /// Whether a peer address is permitted. Candidate labels cannot grant extra permissions.
    #[must_use]
    pub fn permits(self, addr: SocketAddr) -> bool {
        // Our sockets use ephemeral ports. Codes must not target privileged local services.
        if addr.port() < 1024 {
            return false;
        }
        match addr.ip() {
            IpAddr::V4(ip) if ip.is_loopback() => self.loopback,
            IpAddr::V6(ip) if ip.is_loopback() => self.loopback && unscoped(addr),
            IpAddr::V4(ip) if ip.is_private() => self.lan,
            IpAddr::V6(ip) if ip.is_unique_local() => self.lan && unscoped(addr),
            IpAddr::V4(ip) => public_v4(ip),
            IpAddr::V6(ip) => public_v6(ip) && unscoped(addr),
        }
    }
}

fn unscoped(addr: SocketAddr) -> bool {
    match addr {
        SocketAddr::V4(_) => true,
        SocketAddr::V6(addr) => addr.scope_id() == 0 && addr.flowinfo() == 0,
    }
}

/// Whether an address may be advertised as internet-reachable, rather than as a LAN candidate.
#[must_use]
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_documentation()
        || a == 0
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19)))
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let [a, b, ..] = ip.segments();
    // Restrict candidates to global-unicast space. This also excludes mapped/compatible IPv4,
    // NAT64, multicast, link-local, site-local and unspecified addresses without alias bypasses.
    (a & 0xe000 == 0x2000)
        && !(a == 0x2001 && (b < 0x0200 || b == 0x0db8))
        && a != 0x2002 // 6to4 embeds another destination and is not a direct interface address.
        && !(a == 0x3fff && b < 0x1000) // documentation prefix 3fff::/20
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(ip: &str, port: u16) -> SocketAddr {
        SocketAddr::new(ip.parse().unwrap(), port)
    }

    #[test]
    fn internet_mode_never_grants_access_to_local_or_special_destinations() {
        for ip in [
            "0.0.0.0",
            "0.1.2.3",
            "10.2.3.4",
            "127.0.0.1",
            "127.4.5.6",
            "100.64.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "192.0.0.5",
            "192.0.2.1",
            "192.88.99.1",
            "198.18.0.1",
            "198.19.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "239.1.2.3",
            "240.1.2.3",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:8.8.8.8",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "fc00::1",
            "fd00::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2001::1",
            "2001:2::1",
            "2001:db8::1",
            "2002:7f00:1::1",
            "3fff::1",
        ] {
            assert!(!AddressPolicy::default().permits(addr(ip, 50000)), "{ip}");
        }
    }

    #[test]
    fn lan_and_loopback_are_independent_permissions() {
        let lan = AddressPolicy {
            lan: true,
            loopback: false,
        };
        let local_test = AddressPolicy {
            lan: false,
            loopback: true,
        };
        for ip in ["10.1.2.3", "172.31.0.1", "192.168.1.2", "fd12::1"] {
            assert!(lan.permits(addr(ip, 50000)));
            assert!(!local_test.permits(addr(ip, 50000)));
        }
        for ip in ["127.0.0.1", "::1"] {
            assert!(!lan.permits(addr(ip, 50000)));
            assert!(local_test.permits(addr(ip, 50000)));
        }
        for ip in [
            "169.254.1.2",
            "::ffff:127.0.0.1",
            "ff02::1",
            "255.255.255.255",
        ] {
            assert!(
                !AddressPolicy {
                    lan: true,
                    loopback: true
                }
                .permits(addr(ip, 50000))
            );
        }
    }

    #[test]
    fn public_addresses_work_but_no_mode_can_target_a_privileged_port() {
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
        ] {
            assert!(AddressPolicy::default().permits(addr(ip, 50000)));
            assert!(is_public(ip.parse().unwrap()));
            for port in [0, 1, 53, 443, 1023] {
                assert!(
                    !AddressPolicy {
                        lan: true,
                        loopback: true
                    }
                    .permits(addr(ip, port))
                );
            }
        }
    }

    #[test]
    fn scoped_addresses_cannot_be_reinterpreted_by_another_computer() {
        let addr = "[2606:4700:4700::1111%3]:50000".parse().unwrap();
        assert!(!AddressPolicy::default().permits(addr));
    }
}
