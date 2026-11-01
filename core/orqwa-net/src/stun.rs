//! Uncredentialed STUN Binding discovery (RFC 8489). An observation is not authentication.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::net::{UdpSocket, lookup_host};
use tokio::time::{Instant, timeout_at};

use crate::crypto::{RandomError, random};
use crate::policy::AddressPolicy;

const COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const HEADER: usize = 20;
const MAX_MESSAGE: usize = 576;
/// At most four explicitly configured discovery servers; bounds DNS and outgoing requests.
pub const MAX_SERVERS: usize = 4;

/// A Binding request with a caller-supplied cryptographically random transaction ID.
#[must_use]
pub fn binding_request(transaction: &[u8; 12]) -> [u8; HEADER] {
    let mut bytes = [0; HEADER];
    bytes[1] = 1;
    bytes[4..8].copy_from_slice(&COOKIE);
    bytes[8..].copy_from_slice(transaction);
    bytes
}

/// A Binding indication, which keeps this socket's NAT mapping alive without any answer
/// (RFC 8489 section 3). Use a fresh cryptographically random transaction ID each time.
#[must_use]
pub fn binding_indication(transaction: &[u8; 12]) -> [u8; HEADER] {
    let mut bytes = binding_request(transaction);
    bytes[1] = 0x11;
    bytes
}

/// What one server observed of this socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    /// The answering server, to which a refresh of this mapping is sent.
    pub server: SocketAddr,
    /// The public address and port it saw.
    pub seen: SocketAddr,
}

/// Reads a complete bounded response, including a FINGERPRINT check when present.
/// The source address must be matched separately by the socket reader.
#[must_use]
pub fn parse_binding_response(bytes: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    if bytes.len() > MAX_MESSAGE
        || bytes.len() < HEADER
        || bytes[..2] != [1, 1]
        || bytes[4..8] != COOKIE
        || bytes[8..20] != *transaction
    {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
    if len % 4 != 0 || bytes.len() != HEADER + len {
        return None;
    }
    let mut mask = [0; 16];
    mask[..4].copy_from_slice(&COOKIE);
    mask[4..].copy_from_slice(transaction);
    let mut at = HEADER;
    let mut xor = None;
    let mut plain = None;
    while at < bytes.len() {
        let head = bytes.get(at..at + 4)?;
        let kind = u16::from_be_bytes([head[0], head[1]]);
        let len = usize::from(u16::from_be_bytes([head[2], head[3]]));
        let value = bytes.get(at + 4..at + 4 + len)?;
        let next = at + 4 + len.next_multiple_of(4);
        if next > bytes.len() {
            return None;
        }
        match kind {
            0x0020 => {
                if xor.is_some() {
                    return None;
                }
                xor = Some(address(value, &mask)?);
            }
            0x0001 => {
                if plain.is_some() {
                    return None;
                }
                plain = Some(address(value, &[0; 16])?);
            }
            0x8028 => {
                if len != 4 || next != bytes.len() {
                    return None;
                }
                let got = u32::from_be_bytes(value.try_into().ok()?);
                if got != crc32(&bytes[..at]) ^ 0x5354_554e {
                    return None;
                }
            }
            // No credentials are configured for public address discovery. These fields are
            // not treated as proof of authenticity; the advertised destination is still untrusted.
            0x0008 if len == 20 => {}
            0x001c if len == 32 => {}
            _ if kind & 0x8000 != 0 => {}
            _ => return None,
        }
        at = next;
    }
    xor.or(plain)
}

fn address(value: &[u8], mask: &[u8; 16]) -> Option<SocketAddr> {
    if value.first() != Some(&0) {
        return None;
    }
    let port = u16::from_be_bytes([*value.get(2)? ^ mask[0], *value.get(3)? ^ mask[1]]);
    let ip = match (value.get(1)?, value.len()) {
        (1, 8) => IpAddr::V4(Ipv4Addr::from(std::array::from_fn::<_, 4, _>(|i| {
            value[4 + i] ^ mask[i]
        }))),
        (2, 20) => IpAddr::V6(Ipv6Addr::from(std::array::from_fn::<_, 16, _>(|i| {
            value[4 + i] ^ mask[i]
        }))),
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ ((crc & 1).wrapping_neg() & 0xedb8_8320);
        }
    }
    !crc
}

struct Ask {
    server: SocketAddr,
    transaction: [u8; 12],
    seen: Option<SocketAddr>,
}

/// Asks at most four servers on this socket, only until the shared absolute deadline.
/// DNS results are filtered by destination policy before sending. Empty servers means no DNS
/// or STUN traffic. The socket must not have another receiver until discovery finishes.
///
/// # Errors
/// Returns [`RandomError`] if secure transaction IDs cannot be generated.
pub async fn discover(
    socket: &UdpSocket,
    servers: &[String],
    policy: AddressPolicy,
    deadline: Instant,
) -> Result<Vec<Mapping>, RandomError> {
    let Ok(bound) = socket.local_addr() else {
        return Ok(Vec::new());
    };
    let mut asks: Vec<Ask> = Vec::with_capacity(MAX_SERVERS);
    for server in servers.iter().take(MAX_SERVERS) {
        if server.len() > 260 {
            continue;
        }
        if let Ok(Ok(mut addresses)) = timeout_at(deadline, lookup_host(server.as_str())).await
            && let Some(addr) =
                addresses.find(|addr| addr.is_ipv4() == bound.is_ipv4() && policy.permits(*addr))
            && !asks.iter().any(|ask| ask.server == addr)
        {
            asks.push(Ask {
                server: addr,
                transaction: random()?,
                seen: None,
            });
        }
    }
    let mut buffer = [0; MAX_MESSAGE + 1];
    let mut resend = Instant::now();
    let mut rto = Duration::from_millis(500);
    let mut transmissions = 0;
    while Instant::now() < deadline && asks.iter().any(|ask| ask.seen.is_none()) {
        if Instant::now() >= resend {
            if transmissions >= 4 {
                break;
            }
            for ask in asks.iter().filter(|ask| ask.seen.is_none()) {
                let _ = timeout_at(
                    deadline,
                    socket.send_to(&binding_request(&ask.transaction), ask.server),
                )
                .await;
            }
            transmissions += 1;
            resend = Instant::now() + rto;
            rto *= 2;
        }
        if let Ok(Ok((len, from))) =
            timeout_at(resend.min(deadline), socket.recv_from(&mut buffer)).await
            && let Some(ask) = asks
                .iter_mut()
                .find(|ask| ask.server == from && ask.seen.is_none())
            && let Some(seen) = parse_binding_response(&buffer[..len], &ask.transaction)
            && seen.is_ipv4() == bound.is_ipv4()
        {
            ask.seen = Some(seen);
        }
    }
    Ok(asks
        .into_iter()
        .filter_map(|ask| {
            ask.seen.map(|seen| Mapping {
                server: ask.server,
                seen,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    const TRANSACTION: [u8; 12] = [
        0xb7, 0xe7, 0xa7, 1, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
    ];

    #[test]
    fn rfc5769_response_vectors_validate_xor_addresses_and_actual_fingerprints() {
        // RFC 5769 sections 2.2/2.3; test-vector data, including MESSAGE-INTEGRITY and CRC.
        let ipv4 = hex(concat!(
            "0101003c2112a442b7e7a701bc34d686fa87dfae",
            "8022000b7465737420766563746f7220",
            "002000080001a147e112a643",
            "000800142b91f599fd9e90c38c7489f92af9ba53f06be7d7",
            "80280004c07d4c96"
        ));
        let ipv6 = hex(concat!(
            "010100482112a442b7e7a701bc34d686fa87dfae",
            "8022000b7465737420766563746f7220",
            "002000140002a1470113a9faa5d3f179bc25f4b5bed2b9d9",
            "00080014a382954e4be67bf11784c97c8292c275bfe3ed41",
            "80280004c8fb0b4c"
        ));
        assert_eq!(
            parse_binding_response(&ipv4, &TRANSACTION),
            Some("192.0.2.1:32853".parse().unwrap())
        );
        assert_eq!(
            parse_binding_response(&ipv6, &TRANSACTION),
            Some(
                "[2001:db8:1234:5678:11:2233:4455:6677]:32853"
                    .parse()
                    .unwrap()
            )
        );
        let mut bad = ipv4.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(parse_binding_response(&bad, &TRANSACTION).is_none());
        for len in 0..ipv4.len() {
            assert!(parse_binding_response(&ipv4[..len], &TRANSACTION).is_none());
        }
        let mut padded = ipv4;
        padded.push(0);
        assert!(parse_binding_response(&padded, &TRANSACTION).is_none());
    }

    fn response(transaction: &[u8; 12]) -> Vec<u8> {
        let mut bytes = binding_request(transaction).to_vec();
        bytes[..4].copy_from_slice(&[1, 1, 0, 12]);
        bytes.extend([0, 1, 0, 8, 0, 1]);
        bytes.extend(60000u16.to_be_bytes());
        bytes.extend([1, 1, 1, 1]);
        bytes
    }

    #[test]
    fn a_mapping_refresh_is_an_indication_that_servers_never_answer() {
        let indication = binding_indication(&TRANSACTION);
        assert_eq!(indication[..4], [0x00, 0x11, 0, 0]);
        assert_eq!(indication[4..8], COOKIE);
        assert_eq!(indication[8..], TRANSACTION);
        assert_eq!(parse_binding_response(&indication, &TRANSACTION), None);
    }

    #[tokio::test]
    async fn a_local_stand_in_can_drop_a_request_without_using_public_servers() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut buf = [0; 100];
            server.recv_from(&mut buf).await.unwrap();
            let (_, from) = server.recv_from(&mut buf).await.unwrap();
            server
                .send_to(&response(buf[8..20].try_into().unwrap()), from)
                .await
                .unwrap();
        });
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let policy = AddressPolicy {
            lan: false,
            loopback: true,
        };
        let seen = discover(
            &socket,
            &[addr.to_string()],
            policy,
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(
            seen,
            vec![Mapping {
                server: addr,
                seen: "1.1.1.1:60000".parse().unwrap()
            }]
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn empty_servers_and_silent_servers_finish_within_the_shared_deadline() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let policy = AddressPolicy {
            lan: false,
            loopback: true,
        };
        assert_eq!(
            discover(
                &socket,
                &[],
                policy,
                Instant::now() + Duration::from_secs(1)
            )
            .await
            .unwrap(),
            [] as [Mapping; 0]
        );
        let end = Instant::now() + Duration::from_millis(30);
        assert_eq!(
            discover(
                &socket,
                &[server.local_addr().unwrap().to_string()],
                policy,
                end
            )
            .await
            .unwrap(),
            [] as [Mapping; 0]
        );
        assert!(Instant::now() < end + Duration::from_millis(200));
    }
}
