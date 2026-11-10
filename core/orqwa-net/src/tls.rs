//! Per-ride pinned TLS with handshake signatures, explicit QUIC limits and no resumption.
//! Both sides present a fresh certificate: the invite pins the host's, a reply the joiner's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, ServerConfig, TransportConfig, VarInt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::code::Fingerprint;
use crate::crypto::{fingerprint, provider};
use crate::limits::MAX_CONNECTIONS;
use crate::link::LinkError;

pub(crate) const SERVER_NAME: &str = "orqwa";
pub(crate) const ALPN: &[u8] = b"orqwa-group/17";

fn transport() -> Arc<TransportConfig> {
    let mut config = TransportConfig::default();
    // Controls, a download of the selected activity and one of the preview (#418).
    config.max_concurrent_bidi_streams(VarInt::from_u32(3));
    config.max_concurrent_uni_streams(VarInt::from_u32(0));
    // Throughput is at most one window per round trip: 4 KiB made a 40 MB course take many
    // minutes between homes (#535). 1 MiB carries about 10 MiB/s at 100 ms, while a full
    // host still buffers at most 16 connections × 2 MiB from peers that stop reading.
    config.stream_receive_window(VarInt::from_u32(1024 * 1024));
    config.receive_window(VarInt::from_u32(2 * 1024 * 1024));
    config.send_window(2 * 1024 * 1024);
    config.crypto_buffer_size(16 * 1024);
    config.datagram_receive_buffer_size(Some(4096));
    config.datagram_send_buffer_size(4096);
    config.keep_alive_interval(Some(Duration::from_secs(10)));
    config.max_idle_timeout(Some(
        Duration::from_secs(30)
            .try_into()
            .expect("valid idle timeout"),
    ));
    Arc::new(config)
}

fn setup(error: impl std::fmt::Display) -> LinkError {
    LinkError::Encryption(error.to_string())
}

/// A fresh certificate and private key for one ride. The key never leaves this computer;
/// deliberately without Debug, so it cannot reach diagnostics.
pub(crate) struct Identity {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    pin: Fingerprint,
}

impl Identity {
    pub(crate) fn generate() -> Result<Self, LinkError> {
        let certified =
            rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_owned()]).map_err(setup)?;
        let certificate = certified.cert.der().clone();
        Ok(Self {
            pin: fingerprint(&certificate),
            certificate,
            key: PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()),
        })
    }
    pub(crate) const fn pin(&self) -> Fingerprint {
        self.pin
    }
    fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(self.key.clone_key())
    }
}

pub(crate) fn server() -> Result<(ServerConfig, Fingerprint), LinkError> {
    let identity = Identity::generate()?;
    let mut tls = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(setup)?
        .with_client_cert_verifier(Arc::new(Joiners(provider())))
        .with_single_cert(vec![identity.certificate.clone()], identity.key())
        .map_err(setup)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    tls.max_early_data_size = 0;
    tls.send_tls13_tickets = 0;
    let mut config =
        ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(tls).map_err(setup)?));
    config.transport_config(transport());
    config
        .max_incoming(MAX_CONNECTIONS)
        .incoming_buffer_size(4096)
        .incoming_buffer_size_total(64 * 1024)
        .migration(false);
    Ok((config, identity.pin))
}

pub(crate) fn client(
    pin: Fingerprint,
    identity: &Identity,
) -> Result<(ClientConfig, Arc<Pinned>), LinkError> {
    let pinned = Arc::new(Pinned {
        pin,
        provider: provider(),
        mismatch: AtomicBool::new(false),
    });
    let mut tls = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(setup)?
        .dangerous()
        .with_custom_certificate_verifier(pinned.clone())
        .with_client_auth_cert(vec![identity.certificate.clone()], identity.key())
        .map_err(setup)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    tls.resumption = rustls::client::Resumption::disabled();
    tls.enable_early_data = false;
    let mut config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls).map_err(setup)?));
    config.transport_config(transport());
    Ok((config, pinned))
}

#[derive(Debug)]
pub(crate) struct Pinned {
    pin: Fingerprint,
    provider: Arc<CryptoProvider>,
    mismatch: AtomicBool,
}

impl Pinned {
    pub(crate) fn mismatched(&self) -> bool {
        self.mismatch.load(Ordering::Relaxed)
    }
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        certificate: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if fingerprint(certificate) == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            self.mismatch.store(true, Ordering::Relaxed);
            Err(rustls::Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The certificate a joiner proved possession of during the handshake.
pub(crate) fn client_pin(connection: &quinn::Connection) -> Option<Fingerprint> {
    let chain = connection
        .peer_identity()?
        .downcast::<Vec<CertificateDer<'static>>>()
        .ok()?;
    chain.first().map(|certificate| fingerprint(certificate))
}

// Any certificate may finish the handshake; admission then requires its pin to match a reply
// the host accepted. Verifying the handshake signature proves the joiner holds that key.
#[derive(Debug)]
struct Joiners(Arc<CryptoProvider>);

impl ClientCertVerifier for Joiners {
    fn offer_client_auth(&self) -> bool {
        true
    }
    fn client_auth_mandatory(&self) -> bool {
        true
    }
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        _certificate: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::Mutex;

    use quinn::Endpoint;
    use tokio::net::UdpSocket;
    use tokio::time::{Instant, sleep};

    use super::*;

    // One-way delay of a typical connection between two homes.
    const DELAY: Duration = Duration::from_millis(25);

    // Forwards UDP both ways between one client and `server`, each packet `DELAY` late.
    async fn delayed(server: SocketAddr) -> SocketAddr {
        let loopback = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let front = Arc::new(UdpSocket::bind(loopback).await.unwrap());
        let back = Arc::new(UdpSocket::bind(loopback).await.unwrap());
        let address = front.local_addr().unwrap();
        let client = Arc::new(Mutex::new(None::<SocketAddr>));
        let (from, to, seen) = (front.clone(), back.clone(), client.clone());
        tokio::spawn(async move {
            let mut bytes = vec![0; 65_536];
            loop {
                let (count, sender) = from.recv_from(&mut bytes).await.unwrap();
                *seen.lock().unwrap() = Some(sender);
                let (packet, to) = (bytes[..count].to_vec(), to.clone());
                tokio::spawn(async move {
                    sleep(DELAY).await;
                    let _ = to.send_to(&packet, server).await;
                });
            }
        });
        tokio::spawn(async move {
            let mut bytes = vec![0; 65_536];
            loop {
                let (count, _) = back.recv_from(&mut bytes).await.unwrap();
                let Some(client) = *client.lock().unwrap() else {
                    continue;
                };
                let (packet, front) = (bytes[..count].to_vec(), front.clone());
                tokio::spawn(async move {
                    sleep(DELAY).await;
                    let _ = front.send_to(&packet, client).await;
                });
            }
        });
        address
    }

    #[tokio::test]
    async fn a_shared_course_downloads_in_seconds_between_homes() {
        // #535: throughput is at most the flow-control window per round trip. 8 MiB over a
        // 50 ms round trip must take seconds, not the ~100 s that 4 KiB windows allow.
        const BYTES: usize = 8 * 1024 * 1024;
        let loopback = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let (config, pin) = server().unwrap();
        let host = Endpoint::server(config, loopback).unwrap();
        let relay = delayed(host.local_addr().unwrap()).await;
        let (config, _) = client(pin, &Identity::generate().unwrap()).unwrap();
        let mut joiner = Endpoint::client(loopback).unwrap();
        joiner.set_default_client_config(config);

        let serve = async {
            let connection = host.accept().await.unwrap().await.unwrap();
            let (mut send, _) = connection.accept_bi().await.unwrap();
            let chunk = vec![7; 16 * 1024];
            for _ in 0..BYTES / chunk.len() {
                send.write_all(&chunk).await.unwrap();
            }
            send.finish().unwrap();
            send.stopped().await.ok();
        };
        let download = async {
            let connection = joiner.connect(relay, SERVER_NAME).unwrap().await.unwrap();
            let (mut send, mut recv) = connection.open_bi().await.unwrap();
            // A stream reaches the peer only once it carries data.
            send.write_all(&[1]).await.unwrap();
            let started = Instant::now();
            let mut received = 0;
            let mut chunk = vec![0; 64 * 1024];
            while let Some(count) = recv.read(&mut chunk).await.unwrap() {
                received += count;
            }
            (received, started.elapsed())
        };
        let ((), (received, elapsed)) = tokio::join!(serve, download);
        assert_eq!(received, BYTES);
        assert!(
            elapsed < Duration::from_secs(10),
            "8 MiB took {elapsed:?} at a 50 ms round trip"
        );
    }
}
