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
pub(crate) const ALPN: &[u8] = b"orqwa-group/15";

fn transport() -> Arc<TransportConfig> {
    let mut config = TransportConfig::default();
    // Controls, a download of the selected activity and one of the preview (#418).
    config.max_concurrent_bidi_streams(VarInt::from_u32(3));
    config.max_concurrent_uni_streams(VarInt::from_u32(0));
    config.stream_receive_window(VarInt::from_u32(4096));
    config.receive_window(VarInt::from_u32(8192));
    config.send_window(8192);
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
