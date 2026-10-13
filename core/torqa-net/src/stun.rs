//! Asking STUN servers for the public address of a socket: a binding request and its response
//! (RFC 8489), nothing more of the protocol. Kept to these few lines rather than a crate.

use std::hash::{BuildHasher, Hasher, RandomState};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::net::{UdpSocket, lookup_host};
use tokio::time::{Instant, timeout_at};

const MAGIC_COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const BINDING_REQUEST: [u8; 2] = [0x00, 0x01];
const BINDING_SUCCESS: [u8; 2] = [0x01, 0x01];
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const HEADER: usize = 20;

/// The first retransmission timeout of RFC 8489 §6.2.1; it doubles with each retry.
const FIRST_RTO: Duration = Duration::from_millis(500);

/// A binding request with the given transaction id.
#[must_use]
pub fn binding_request(transaction: &[u8; 12]) -> [u8; HEADER] {
    let mut request = [0; HEADER];
    request[..2].copy_from_slice(&BINDING_REQUEST);
    request[4..8].copy_from_slice(&MAGIC_COOKIE);
    request[8..].copy_from_slice(transaction);
    request
}

/// The address in a successful binding response to the request with `transaction`, or `None`
/// for anything else.
#[must_use]
pub fn parse_binding_response(message: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    let header = message.get(..HEADER)?;
    if header[..2] != BINDING_SUCCESS || header[4..8] != MAGIC_COOKIE || header[8..] != *transaction
    {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([header[2], header[3]]));
    let body = message.get(HEADER..HEADER + len)?;
    let mut mask = [0; 16];
    mask[..4].copy_from_slice(&MAGIC_COOKIE);
    mask[4..].copy_from_slice(transaction);

    let mut plain = None;
    let mut at = 0;
    while let Some(head) = body.get(at..at + 4) {
        let kind = u16::from_be_bytes([head[0], head[1]]);
        let len = usize::from(u16::from_be_bytes([head[2], head[3]]));
        let value = body.get(at + 4..at + 4 + len)?;
        match kind {
            XOR_MAPPED_ADDRESS => return address(value, Some(&mask)),
            // Servers of the older RFC 3489 send the address as it is.
            MAPPED_ADDRESS => plain = address(value, None),
            _ => {}
        }
        at += 4 + len.next_multiple_of(4);
    }
    plain
}

fn address(value: &[u8], mask: Option<&[u8; 16]>) -> Option<SocketAddr> {
    let zero = [0; 16];
    let mask = mask.unwrap_or(&zero);
    let port = u16::from_be_bytes([value.get(2)? ^ mask[0], value.get(3)? ^ mask[1]]);
    let unmask = |bytes: &[u8]| -> Vec<u8> { bytes.iter().zip(mask).map(|(b, m)| b ^ m).collect() };
    let ip = match value.get(1)? {
        1 => IpAddr::V4(Ipv4Addr::from(
            <[u8; 4]>::try_from(unmask(value.get(4..8)?)).ok()?,
        )),
        2 => IpAddr::V6(Ipv6Addr::from(
            <[u8; 16]>::try_from(unmask(value.get(4..20)?)).ok()?,
        )),
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// A transaction id. The standard library seeds its hashers from the system's random source,
/// which is random enough to tell answers apart.
fn transaction_id() -> [u8; 12] {
    let mut id = [0; 12];
    id[..8].copy_from_slice(&RandomState::new().build_hasher().finish().to_be_bytes());
    id[8..].copy_from_slice(&RandomState::new().build_hasher().finish().to_be_bytes()[..4]);
    id
}

struct Ask {
    server: SocketAddr,
    transaction: [u8; 12],
    seen: Option<SocketAddr>,
}

/// Asks each of `servers` (`host:port`) for the address it sees `socket` at, all at once on
/// `socket`, retrying as RFC 8489 does until `timeout`. Returns the answers in the servers'
/// order, leaving out servers that did not answer or have no address of the socket's family.
pub async fn public_addresses(
    socket: &UdpSocket,
    servers: &[String],
    timeout: Duration,
) -> Vec<SocketAddr> {
    let deadline = Instant::now() + timeout;
    let v4 = socket.local_addr().map_or(true, |a| a.is_ipv4());
    let mut asks = Vec::new();
    for server in servers {
        match timeout_at(deadline, lookup_host(server.as_str())).await {
            Ok(Ok(mut found)) => {
                if let Some(addr) = found.find(|a| a.is_ipv4() == v4) {
                    asks.push(Ask {
                        server: addr,
                        transaction: transaction_id(),
                        seen: None,
                    });
                }
            }
            Ok(Err(e)) => tracing::debug!("STUN server {server} not found: {e}"),
            Err(_) => tracing::debug!("STUN server {server} not found in time"),
        }
    }

    let mut buffer = [0; 576];
    let mut resend = Instant::now();
    let mut rto = FIRST_RTO;
    while asks.iter().any(|a| a.seen.is_none()) && Instant::now() < deadline {
        if Instant::now() >= resend {
            for ask in asks.iter().filter(|a| a.seen.is_none()) {
                let request = binding_request(&ask.transaction);
                if let Err(e) = socket.send_to(&request, ask.server).await {
                    tracing::debug!("STUN request to {} not sent: {e}", ask.server);
                }
            }
            resend = Instant::now() + rto;
            rto *= 2;
        }
        // Windows reports an unreachable server as an error of the next receive; the other
        // servers may still answer.
        if let Ok(Ok((n, from))) =
            timeout_at(resend.min(deadline), socket.recv_from(&mut buffer)).await
            && let Some(ask) = asks
                .iter_mut()
                .find(|a| a.server == from && a.seen.is_none())
        {
            ask.seen = parse_binding_response(&buffer[..n], &ask.transaction);
        }
    }
    asks.into_iter().filter_map(|a| a.seen).collect()
}

#[cfg(test)]
pub(crate) mod testing {
    //! A stand-in for a STUN server.

    use std::net::SocketAddr;

    use tokio::net::UdpSocket;

    /// A successful binding response telling `seen`.
    pub(crate) fn response(transaction: &[u8; 12], seen: SocketAddr) -> Vec<u8> {
        let mut mask = super::MAGIC_COOKIE.to_vec();
        mask.extend_from_slice(transaction);
        let (family, ip) = match seen.ip() {
            std::net::IpAddr::V4(ip) => (1, ip.octets().to_vec()),
            std::net::IpAddr::V6(ip) => (2, ip.octets().to_vec()),
        };
        let port = seen.port().to_be_bytes();
        let mut value = vec![0, family, port[0] ^ mask[0], port[1] ^ mask[1]];
        value.extend(ip.iter().zip(&mask).map(|(b, m)| b ^ m));

        let mut body = vec![0x80, 0x22, 0, 5];
        body.extend_from_slice(b"stand\0\0\0"); // SOFTWARE, padded
        body.extend_from_slice(&super::XOR_MAPPED_ADDRESS.to_be_bytes());
        #[allow(clippy::cast_possible_truncation)] // 8 or 20 bytes
        body.extend_from_slice(&(value.len() as u16).to_be_bytes());
        body.extend_from_slice(&value);

        let mut message = super::BINDING_SUCCESS.to_vec();
        #[allow(clippy::cast_possible_truncation)] // a few dozen bytes
        message.extend_from_slice(&(body.len() as u16).to_be_bytes());
        message.extend_from_slice(&super::MAGIC_COOKIE);
        message.extend_from_slice(transaction);
        message.extend_from_slice(&body);
        message
    }

    /// A STUN server on the loopback address that ignores the first `ignore` requests, as if
    /// they were lost, and answers the others with the address they came from.
    pub(crate) async fn server(ignore: usize) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buffer = [0; 576];
            let mut ignored = 0;
            while let Ok((n, from)) = socket.recv_from(&mut buffer).await {
                if ignored < ignore {
                    ignored += 1;
                    continue;
                }
                if n == super::HEADER && buffer[..2] == super::BINDING_REQUEST {
                    let transaction = buffer[8..20].try_into().unwrap();
                    let _ = socket.send_to(&response(&transaction, from), from).await;
                }
            }
        });
        addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transaction id of the sample responses of RFC 5769.
    const RFC_5769_TRANSACTION: [u8; 12] = [
        0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
    ];

    /// The sample response of RFC 5769 §2.2 or §2.3 around the given XOR-MAPPED-ADDRESS value;
    /// MESSAGE-INTEGRITY and FINGERPRINT are zeros here, as Torqa does not check them.
    fn rfc_5769_response(xor_mapped: &[u8]) -> Vec<u8> {
        let mut body = vec![0x80, 0x22, 0x00, 0x0b];
        body.extend_from_slice(b"test vector ");
        body.extend_from_slice(&[0x00, 0x20, 0x00]);
        #[allow(clippy::cast_possible_truncation)]
        body.push(xor_mapped.len() as u8);
        body.extend_from_slice(xor_mapped);
        body.extend_from_slice(&[0x00, 0x08, 0x00, 0x14]);
        body.extend_from_slice(&[0; 20]);
        body.extend_from_slice(&[0x80, 0x28, 0x00, 0x04, 0, 0, 0, 0]);
        let mut message = vec![0x01, 0x01, 0x00];
        #[allow(clippy::cast_possible_truncation)]
        message.push(body.len() as u8);
        message.extend_from_slice(&MAGIC_COOKIE);
        message.extend_from_slice(&RFC_5769_TRANSACTION);
        message.extend_from_slice(&body);
        message
    }

    #[test]
    fn reads_the_ipv4_sample_response_of_rfc_5769() {
        let response = rfc_5769_response(&[0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43]);
        assert_eq!(response.len(), 20 + 0x3c);
        assert_eq!(
            parse_binding_response(&response, &RFC_5769_TRANSACTION),
            Some("192.0.2.1:32853".parse().unwrap())
        );
    }

    #[test]
    fn reads_the_ipv6_sample_response_of_rfc_5769() {
        let response = rfc_5769_response(&[
            0x00, 0x02, 0xa1, 0x47, 0x01, 0x13, 0xa9, 0xfa, 0xa5, 0xd3, 0xf1, 0x79, 0xbc, 0x25,
            0xf4, 0xb5, 0xbe, 0xd2, 0xb9, 0xd9,
        ]);
        assert_eq!(response.len(), 20 + 0x48);
        assert_eq!(
            parse_binding_response(&response, &RFC_5769_TRANSACTION),
            Some(
                "[2001:db8:1234:5678:11:2233:4455:6677]:32853"
                    .parse()
                    .unwrap()
            )
        );
    }

    #[test]
    fn reads_the_plain_address_of_older_servers() {
        let mut response = vec![0x01, 0x01, 0x00, 0x0c];
        response.extend_from_slice(&MAGIC_COOKIE);
        response.extend_from_slice(&RFC_5769_TRANSACTION);
        response.extend_from_slice(&[0x00, 0x01, 0x00, 0x08, 0x00, 0x01, 0x80, 0x55, 192, 0, 2, 1]);
        assert_eq!(
            parse_binding_response(&response, &RFC_5769_TRANSACTION),
            Some("192.0.2.1:32853".parse().unwrap())
        );
    }

    #[test]
    fn ignores_answers_to_other_requests_errors_and_damaged_messages() {
        let response = rfc_5769_response(&[0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43]);
        let mut other = RFC_5769_TRANSACTION;
        other[0] ^= 1;
        assert_eq!(parse_binding_response(&response, &other), None);

        let mut error = response.clone();
        error[..2].copy_from_slice(&[0x01, 0x11]);
        assert_eq!(parse_binding_response(&error, &RFC_5769_TRANSACTION), None);

        for end in [0, 10, 20, 30, response.len() - 1] {
            assert_eq!(
                parse_binding_response(&response[..end], &RFC_5769_TRANSACTION),
                None,
                "{end}"
            );
        }
    }

    #[test]
    fn a_request_is_a_bare_binding_request() {
        let request = binding_request(&RFC_5769_TRANSACTION);
        assert_eq!(
            request[..8],
            [0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xa4, 0x42]
        );
        assert_eq!(request[8..], RFC_5769_TRANSACTION);
    }

    #[test]
    fn transaction_ids_differ() {
        assert_ne!(transaction_id(), transaction_id());
    }

    #[tokio::test]
    async fn asks_every_server_and_asks_again_when_a_request_is_lost() {
        let servers = [
            server_name(testing::server(0).await),
            server_name(testing::server(1).await),
        ];
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let own = socket.local_addr().unwrap();
        let seen = public_addresses(&socket, &servers, Duration::from_secs(3)).await;
        assert_eq!(seen, [own, own]);
    }

    #[tokio::test]
    async fn gives_up_on_silent_servers_in_time() {
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let servers = [server_name(silent.local_addr().unwrap())];
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let started = Instant::now();
        let seen = public_addresses(&socket, &servers, Duration::from_millis(800)).await;
        assert_eq!(seen, []);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    fn server_name(addr: SocketAddr) -> String {
        addr.to_string()
    }
}
