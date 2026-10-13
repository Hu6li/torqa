//! Connecting a group ride (ADR 0015): the [`Host`] and each [`Joiner`] punch through their
//! routers with probes on the ride's sockets, then talk QUIC on the same sockets. The invite pins
//! the host's certificate, and the host lets in only joiners whose replies it took.

use std::collections::HashMap;
use std::net::{Ipv6Addr, SocketAddr, UdpSocket as StdSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{
    ClientConfig, ConnectionError, Endpoint, EndpointConfig, ServerConfig, TransportConfig, VarInt,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};
use tokio::net::UdpSocket;
use tokio::sync::{Notify, mpsc};
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::{Instant, sleep, sleep_until, timeout, timeout_at};

pub use quinn::Connection;

use crate::candidates::{Candidate, CandidateKind, GatherOptions, NatBehavior, gather};
use crate::code::{Course, Invite, Reply};
use crate::router::{RENEW_EVERY, RouterMapping};

/// How long host and joiner try to reach each other: time enough to pass a code on.
pub const PUNCH_WINDOW: Duration = Duration::from_secs(60);

/// How often each side probes the other's addresses while punching.
const PROBE_EVERY: Duration = Duration::from_millis(200);

/// A probe only opens the sender's router. Its first byte has QUIC's fixed bit clear, so QUIC
/// drops it rather than answering it.
const PROBE: &[u8] = &[0, b't', b'q'];

/// A joiner starts its connection afresh this often: QUIC backs off its resends, while the
/// routers between may open only now.
const ATTEMPT: Duration = Duration::from_secs(5);

const ALPN: &[u8] = b"torqa-group/1";
const SERVER_NAME: &str = "torqa";

/// Keep-alives hold the routers' mappings open, which close after 30 s of silence at worst.
const KEEP_ALIVE: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the host waits for a connected joiner to show its secret.
const SECRET_WAIT: Duration = Duration::from_secs(10);

/// The host closes connections of joiners it did not take with this code.
const NOT_ADMITTED: u32 = 1;

/// How to connect.
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    /// Which addresses to offer (R68: STUN can be switched off).
    pub gather: GatherOptions,
    /// How long host and joiner try to reach each other ([`PUNCH_WINDOW`]).
    pub window: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            gather: GatherOptions::default(),
            window: PUNCH_WINDOW,
        }
    }
}

/// Why a host or joiner could not start.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// No UDP socket of either address family.
    #[error("No network socket: {0}")]
    NoSocket(std::io::Error),
    /// The ride's encryption could not be set up.
    #[error("Encryption could not be set up: {0}")]
    Tls(String),
}

/// Why a joiner did not get in.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JoinError {
    /// No answer from any of the host's addresses within the window.
    #[error("The host could not be reached.")]
    Unreachable(Hint),
    /// The host was reached but did not take the joiner's reply in time.
    #[error("The host did not paste your reply in time.")]
    NotAdmitted,
    /// Something answered that is not the host of the invite.
    #[error("Someone other than the host answered.")]
    WrongHost,
    /// The connection broke off while getting in.
    #[error("The connection to the host broke off: {0}")]
    Lost(String),
}

/// What may have kept two riders apart (R69), from what their codes say about their networks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    /// One of them has no address reachable from the internet: no STUN answer (or STUN switched
    /// off), no port from the router and no public IPv6 address.
    NoPublicAddress,
    /// Both routers pick a port per destination; punching cannot get through two of them.
    BothSymmetric,
    /// One router picks a port per destination; punching fails if the other router is strict
    /// about where packets come from.
    OneSymmetric,
    /// Nothing in the codes rules it out: a firewall may block UDP, or a code was passed on
    /// too late.
    Blocked,
}

/// What may have kept a rider with `own` addresses from reaching one with `peer` addresses.
#[must_use]
pub fn diagnose(
    own: &[Candidate],
    own_nat: NatBehavior,
    peer: &[Candidate],
    peer_nat: NatBehavior,
) -> Hint {
    let public = |c: &[Candidate]| c.iter().any(|c| c.kind != CandidateKind::Local);
    let open = |c: &[Candidate]| {
        c.iter()
            .any(|c| matches!(c.kind, CandidateKind::Router | CandidateKind::Public))
    };
    if !public(own) || !public(peer) {
        Hint::NoPublicAddress
    } else if open(own) || open(peer) {
        Hint::Blocked
    } else {
        match (
            own_nat == NatBehavior::Symmetric,
            peer_nat == NatBehavior::Symmetric,
        ) {
            (true, true) => Hint::BothSymmetric,
            (true, false) | (false, true) => Hint::OneSymmetric,
            (false, false) => Hint::Blocked,
        }
    }
}

/// What happens at the host.
#[derive(Debug)]
pub enum HostEvent {
    /// A joiner whose reply the host took is in.
    Joined {
        /// The secret of the joiner's reply.
        secret: [u8; 16],
        /// The connection to the joiner.
        connection: Connection,
    },
    /// A joiner whose reply the host took did not get through within the window.
    GaveUp {
        /// The secret of the joiner's reply.
        secret: [u8; 16],
        /// What may have kept them apart.
        hint: Hint,
    },
}

/// The host of a group ride: makes the invite, takes replies and lets their joiners in.
pub struct Host {
    local: Local,
    fingerprint: [u8; 32],
    window: Duration,
    shared: Arc<Shared>,
    events: mpsc::UnboundedReceiver<HostEvent>,
    accepting: Vec<AbortHandle>,
}

struct Shared {
    admitted: Mutex<HashMap<[u8; 16], Admission>>,
    admissions: Notify,
    events: mpsc::UnboundedSender<HostEvent>,
}

struct Admission {
    probing: AbortHandle,
    joined: bool,
}

impl Host {
    /// Opens the ride's sockets, gathers their addresses and makes the ride's certificate.
    ///
    /// # Errors
    ///
    /// No socket could be opened, or the encryption could not be set up ([`OpenError`]).
    pub async fn open(options: &ConnectOptions) -> Result<Self, OpenError> {
        let provider = provider();
        let (server, fingerprint) = server_config(&provider)?;
        let local = Local::open(&options.gather, Some(&server)).await?;
        let (events_tx, events) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            admitted: Mutex::new(HashMap::new()),
            admissions: Notify::new(),
            events: events_tx,
        });
        let accepting = local
            .paths
            .iter()
            .map(|p| {
                let task = accept_all(p.endpoint.clone(), shared.clone(), options.window);
                tokio::spawn(task).abort_handle()
            })
            .collect();
        Ok(Self {
            local,
            fingerprint,
            window: options.window,
            shared,
            events,
            accepting,
        })
    }

    /// The invite to pass on to everyone who joins.
    #[must_use]
    pub fn invite(&self, host: &str, course: Course) -> Invite {
        Invite {
            host: host.to_owned(),
            course,
            fingerprint: self.fingerprint,
            candidates: self.local.candidates.clone(),
            nat: self.local.nat,
        }
    }

    /// Takes a joiner's reply: probes their addresses and lets them in once they connect,
    /// within the window. A reply taken before is ignored.
    ///
    /// # Panics
    ///
    /// Only if a task panicked while holding the list of replies.
    pub fn admit(&self, reply: &Reply) {
        let mut admitted = self
            .shared
            .admitted
            .lock()
            .expect("no task panics holding it");
        if admitted.contains_key(&reply.secret) {
            return;
        }
        let targets = reply.candidates.iter().map(|c| c.addr).collect();
        let probing = self.local.probe(targets, Instant::now() + self.window);
        admitted.insert(
            reply.secret,
            Admission {
                probing,
                joined: false,
            },
        );
        drop(admitted);
        self.shared.admissions.notify_waiters();

        let hint = diagnose(
            &self.local.candidates,
            self.local.nat,
            &reply.candidates,
            reply.nat,
        );
        let (shared, secret, window) = (self.shared.clone(), reply.secret, self.window);
        tokio::spawn(async move {
            sleep(window).await;
            let mut admitted = shared.admitted.lock().expect("no task panics holding it");
            if admitted.get(&secret).is_some_and(|a| !a.joined) {
                admitted.remove(&secret);
                let _ = shared.events.send(HostEvent::GaveUp { secret, hint });
            }
        });
    }

    /// The next joiner in or given up on; `None` once the host is closed.
    pub async fn next(&mut self) -> Option<HostEvent> {
        self.events.recv().await
    }

    /// Stops letting anyone in, closes every connection and releases the router's port.
    pub async fn close(self) {
        for task in &self.accepting {
            task.abort();
        }
        self.local.close().await;
    }
}

async fn accept_all(endpoint: Endpoint, shared: Arc<Shared>, window: Duration) {
    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(welcome(incoming, shared.clone(), window));
    }
}

/// Lets a connected joiner in once their secret is among the replies taken; a joiner who
/// reaches a host with a public address may be there before the host pastes their reply.
async fn welcome(incoming: quinn::Incoming, shared: Arc<Shared>, window: Duration) {
    let Ok(connection) = incoming.await else {
        return;
    };
    let refuse = |connection: &Connection| {
        connection.close(VarInt::from_u32(NOT_ADMITTED), b"not admitted");
    };
    let Ok(Ok((mut send, mut recv))) = timeout(SECRET_WAIT, connection.accept_bi()).await else {
        refuse(&connection);
        return;
    };
    let mut secret = [0; 16];
    if !matches!(
        timeout(SECRET_WAIT, recv.read_exact(&mut secret)).await,
        Ok(Ok(()))
    ) {
        refuse(&connection);
        return;
    }
    let deadline = Instant::now() + window;
    loop {
        let admission = shared.admissions.notified();
        tokio::pin!(admission);
        admission.as_mut().enable();
        let taken = {
            let mut admitted = shared.admitted.lock().expect("no task panics holding it");
            match admitted.get_mut(&secret) {
                Some(a) if !a.joined => {
                    a.joined = true;
                    a.probing.abort();
                    true
                }
                _ => false,
            }
        };
        if taken {
            if send.write_all(&[1]).await.is_ok() && send.finish().is_ok() {
                let _ = shared.events.send(HostEvent::Joined { secret, connection });
            }
            return;
        }
        if timeout_at(deadline, admission).await.is_err() {
            refuse(&connection);
            return;
        }
    }
}

/// A rider joining a group ride from an invite.
pub struct Joiner {
    local: Local,
    invite: Invite,
    secret: [u8; 16],
    client: ClientConfig,
    pinned: Arc<Pinned>,
    deadline: Instant,
    probing: AbortHandle,
}

impl Joiner {
    /// Opens the ride's sockets, gathers their addresses and starts probing the host's.
    ///
    /// # Errors
    ///
    /// No socket could be opened, or the encryption could not be set up ([`OpenError`]).
    pub async fn open(invite: Invite, options: &ConnectOptions) -> Result<Self, OpenError> {
        let provider = provider();
        let mut secret = [0; 16];
        provider
            .secure_random
            .fill(&mut secret)
            .map_err(|_| OpenError::Tls("no random numbers".into()))?;
        let pinned = Arc::new(Pinned {
            fingerprint: invite.fingerprint,
            provider: provider.clone(),
            mismatch: AtomicBool::new(false),
        });
        let client = client_config(&provider, pinned.clone())?;
        let local = Local::open(&options.gather, None).await?;
        let deadline = Instant::now() + options.window;
        let targets = invite.candidates.iter().map(|c| c.addr).collect();
        let probing = local.probe(targets, deadline);
        Ok(Self {
            local,
            invite,
            secret,
            client,
            pinned,
            deadline,
            probing,
        })
    }

    /// The reply to pass on to the host.
    #[must_use]
    pub fn reply(&self) -> Reply {
        Reply {
            secret: self.secret,
            candidates: self.local.candidates.clone(),
            nat: self.local.nat,
        }
    }

    /// The invite being answered.
    #[must_use]
    pub fn invite(&self) -> &Invite {
        &self.invite
    }

    /// Connects to the host and gets in, trying until the window is over.
    ///
    /// # Errors
    ///
    /// The host could not be reached, did not take the reply, or was not the host of the
    /// invite ([`JoinError`]).
    pub async fn connect(&self) -> Result<Connection, JoinError> {
        let connection = self.reach().await?;
        self.probing.abort();
        let lost = |e: &dyn std::fmt::Display| JoinError::Lost(e.to_string());
        let (mut send, mut recv) = connection.open_bi().await.map_err(|e| lost(&e))?;
        send.write_all(&self.secret).await.map_err(|e| lost(&e))?;
        send.finish().map_err(|e| lost(&e))?;
        let mut welcome = [0; 1];
        match recv.read_exact(&mut welcome).await {
            Ok(()) => Ok(connection),
            Err(e) => match connection.close_reason() {
                Some(ConnectionError::ApplicationClosed(close))
                    if close.error_code == VarInt::from_u32(NOT_ADMITTED) =>
                {
                    Err(JoinError::NotAdmitted)
                }
                _ => Err(lost(&e)),
            },
        }
    }

    /// Tries every address of the host at once, round after round, until one answers.
    async fn reach(&self) -> Result<Connection, JoinError> {
        let unreachable = || {
            JoinError::Unreachable(diagnose(
                &self.local.candidates,
                self.local.nat,
                &self.invite.candidates,
                self.invite.nat,
            ))
        };
        loop {
            let round = Instant::now();
            if round >= self.deadline {
                return Err(unreachable());
            }
            let wait = ATTEMPT.min(self.deadline - round);
            let mut attempts = JoinSet::new();
            for candidate in &self.invite.candidates {
                let Some(endpoint) = self.local.endpoint_for(candidate.addr) else {
                    continue;
                };
                match endpoint.connect_with(self.client.clone(), candidate.addr, SERVER_NAME) {
                    Ok(connecting) => {
                        attempts.spawn(async move { timeout(wait, connecting).await });
                    }
                    Err(e) => tracing::debug!("No connection to {}: {e}", candidate.addr),
                }
            }
            if attempts.is_empty() {
                return Err(unreachable());
            }
            while let Some(attempt) = attempts.join_next().await {
                if let Ok(Ok(Ok(connection))) = attempt {
                    return Ok(connection);
                }
                if self.pinned.mismatch.load(Ordering::Relaxed) {
                    return Err(JoinError::WrongHost);
                }
            }
            sleep_until((round + ATTEMPT).min(self.deadline)).await;
        }
    }

    /// Closes the connection to the host and releases the router's port.
    pub async fn close(self) {
        self.probing.abort();
        self.local.close().await;
    }
}

/// The ride's sockets, one per address family that has one, and what they offer.
struct Local {
    paths: Vec<Path>,
    candidates: Vec<Candidate>,
    nat: NatBehavior,
    mapping: Arc<tokio::sync::Mutex<Option<RouterMapping>>>,
    renewing: AbortHandle,
}

/// A socket: QUIC owns it, and a second handle on it sends the probes.
struct Path {
    endpoint: Endpoint,
    prober: StdSocket,
}

impl Local {
    async fn open(
        options: &GatherOptions,
        server: Option<&ServerConfig>,
    ) -> Result<Self, OpenError> {
        let mut paths = Vec::new();
        let mut candidates = Vec::new();
        let mut nat = NatBehavior::NoAnswer;
        let mut mapping = None;
        let mut failed = None;
        for bind in [
            SocketAddr::from(([0, 0, 0, 0], 0)),
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
        ] {
            let socket = match UdpSocket::bind(bind).await {
                Ok(socket) => socket,
                Err(e) => {
                    tracing::debug!("No socket on {bind}: {e}");
                    failed = Some(e);
                    continue;
                }
            };
            let gathered = gather(&socket, options).await;
            let socket = socket.into_std().map_err(OpenError::NoSocket)?;
            let prober = socket.try_clone().map_err(OpenError::NoSocket)?;
            let endpoint = Endpoint::new(
                EndpointConfig::default(),
                server.cloned(),
                socket,
                Arc::new(quinn::TokioRuntime),
            )
            .map_err(OpenError::NoSocket)?;
            candidates.extend(gathered.candidates);
            // IPv4 comes first and is where routers map ports.
            if nat == NatBehavior::NoAnswer {
                nat = gathered.nat;
            }
            mapping = mapping.or(gathered.mapping);
            paths.push(Path { endpoint, prober });
        }
        if paths.is_empty() {
            return Err(OpenError::NoSocket(
                failed.unwrap_or_else(|| std::io::Error::other("no address family")),
            ));
        }
        candidates.sort_by_key(|c| c.kind);
        let mapping = Arc::new(tokio::sync::Mutex::new(mapping));
        let renewing = tokio::spawn({
            let mapping = mapping.clone();
            async move {
                loop {
                    sleep(RENEW_EVERY).await;
                    if let Some(mapping) = mapping.lock().await.as_mut() {
                        mapping.renew().await;
                    }
                }
            }
        })
        .abort_handle();
        Ok(Self {
            paths,
            candidates,
            nat,
            mapping,
            renewing,
        })
    }

    fn endpoint_for(&self, addr: SocketAddr) -> Option<&Endpoint> {
        self.paths
            .iter()
            .find(|p| {
                p.prober
                    .local_addr()
                    .is_ok_and(|a| a.is_ipv4() == addr.is_ipv4())
            })
            .map(|p| &p.endpoint)
    }

    /// Sends probes to `targets` from the socket of their family until `until`.
    fn probe(&self, targets: Vec<SocketAddr>, until: Instant) -> AbortHandle {
        let probers: Vec<StdSocket> = self
            .paths
            .iter()
            .filter_map(|p| p.prober.try_clone().ok())
            .collect();
        tokio::spawn(async move {
            while Instant::now() < until {
                for target in &targets {
                    let prober = probers.iter().find(|p| {
                        p.local_addr()
                            .is_ok_and(|a| a.is_ipv4() == target.is_ipv4())
                    });
                    // Unreachable addresses are expected among the candidates.
                    if let Some(prober) = prober {
                        let _ = prober.send_to(PROBE, target);
                    }
                }
                sleep(PROBE_EVERY).await;
            }
        })
        .abort_handle()
    }

    async fn close(self) {
        self.renewing.abort();
        for path in &self.paths {
            path.endpoint.close(VarInt::from_u32(0), b"");
        }
        if let Some(mapping) = self.mapping.lock().await.take() {
            mapping.release().await;
        }
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

fn sha256(provider: &CryptoProvider, bytes: &[u8]) -> [u8; 32] {
    let hash = provider
        .cipher_suites
        .iter()
        .filter_map(rustls::SupportedCipherSuite::tls13)
        .map(|suite| suite.common.hash_provider)
        .find(|hash| hash.algorithm() == rustls::crypto::hash::HashAlgorithm::SHA256)
        .expect("TLS 1.3 has SHA-256 suites");
    hash.hash(bytes)
        .as_ref()
        .try_into()
        .expect("SHA-256 has 32 bytes")
}

fn transport() -> Arc<TransportConfig> {
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(KEEP_ALIVE));
    transport.max_idle_timeout(Some(
        IDLE_TIMEOUT
            .try_into()
            .expect("30 s is a valid idle timeout"),
    ));
    Arc::new(transport)
}

fn tls_error(e: impl std::fmt::Display) -> OpenError {
    OpenError::Tls(e.to_string())
}

/// The host's QUIC configuration with a certificate of its own for this ride, and the
/// certificate's fingerprint for the invite.
fn server_config(provider: &Arc<CryptoProvider>) -> Result<(ServerConfig, [u8; 32]), OpenError> {
    let certified =
        rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_owned()]).map_err(tls_error)?;
    let cert = certified.cert.der().clone();
    let fingerprint = sha256(provider, &cert);
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let mut tls = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_error)?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(tls_error)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicServerConfig::try_from(tls).map_err(tls_error)?;
    let mut config = ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(transport());
    Ok((config, fingerprint))
}

fn client_config(
    provider: &Arc<CryptoProvider>,
    pinned: Arc<Pinned>,
) -> Result<ClientConfig, OpenError> {
    let mut tls = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(pinned)
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(tls_error)?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport());
    Ok(config)
}

/// Trusts the one certificate whose fingerprint the invite carries, rather than certificate
/// authorities: the host's certificate is made for the ride and signed by nobody.
#[derive(Debug)]
struct Pinned {
    fingerprint: [u8; 32],
    provider: Arc<CryptoProvider>,
    mismatch: AtomicBool,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if sha256(&self.provider, end_entity) == self.fingerprint {
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
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(window: Duration) -> ConnectOptions {
        ConnectOptions {
            gather: GatherOptions {
                stun_servers: Vec::new(),
                router: false,
                timeout: Duration::from_secs(1),
                loopback: true,
            },
            window,
        }
    }

    fn course() -> Course {
        Course {
            name: "Col du Galibier".into(),
            route_hash: [7; 32],
        }
    }

    fn candidate(kind: CandidateKind, addr: &str) -> Candidate {
        Candidate {
            kind,
            addr: addr.parse().unwrap(),
        }
    }

    /// A UDP socket that never answers, as a host or joiner behind a closed router.
    fn silent() -> (StdSocket, SocketAddr) {
        let socket = StdSocket::bind("127.0.0.1:0").unwrap();
        let addr = socket.local_addr().unwrap();
        (socket, addr)
    }

    async fn joined(host: &mut Host) -> ([u8; 16], Connection) {
        match host.next().await {
            Some(HostEvent::Joined { secret, connection }) => (secret, connection),
            other => panic!("expected a joiner, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_joiner_whose_reply_the_host_took_gets_in_and_both_talk() {
        let opts = options(Duration::from_secs(10));
        let mut host = Host::open(&opts).await.unwrap();
        let joiner = Joiner::open(host.invite("Host", course()), &opts)
            .await
            .unwrap();
        host.admit(&joiner.reply());

        let (to_host, (secret, to_joiner)) = tokio::join!(joiner.connect(), joined(&mut host));
        let to_host = to_host.unwrap();
        assert_eq!(secret, joiner.reply().secret);

        to_host
            .send_datagram(b"from joiner".to_vec().into())
            .unwrap();
        assert_eq!(
            &to_joiner.read_datagram().await.unwrap()[..],
            b"from joiner"
        );
        to_joiner
            .send_datagram(b"from host".to_vec().into())
            .unwrap();
        assert_eq!(&to_host.read_datagram().await.unwrap()[..], b"from host");
        host.close().await;
        joiner.close().await;
    }

    #[tokio::test]
    async fn the_host_may_take_the_reply_after_the_joiner_reached_it() {
        let opts = options(Duration::from_secs(10));
        let mut host = Host::open(&opts).await.unwrap();
        let joiner = Joiner::open(host.invite("Host", course()), &opts)
            .await
            .unwrap();
        let reply = joiner.reply();
        let late = async {
            sleep(Duration::from_millis(500)).await;
            host.admit(&reply);
            joined(&mut host).await
        };
        let (to_host, _) = tokio::join!(joiner.connect(), late);
        assert!(to_host.is_ok(), "{to_host:?}");
    }

    #[tokio::test]
    async fn a_joiner_the_host_did_not_take_is_turned_away() {
        let opts = options(Duration::from_secs(1));
        let host = Host::open(&opts).await.unwrap();
        let joiner = Joiner::open(host.invite("Host", course()), &opts)
            .await
            .unwrap();
        assert_eq!(joiner.connect().await.err(), Some(JoinError::NotAdmitted));
    }

    #[tokio::test]
    async fn a_joiner_trusts_only_the_host_of_the_invite() {
        let opts = options(Duration::from_secs(10));
        let host = Host::open(&opts).await.unwrap();
        let mut invite = host.invite("Host", course());
        invite.fingerprint[0] ^= 1;
        let joiner = Joiner::open(invite, &opts).await.unwrap();
        host.admit(&joiner.reply());
        assert_eq!(joiner.connect().await.err(), Some(JoinError::WrongHost));
    }

    #[tokio::test]
    async fn a_host_that_does_not_answer_is_given_up_on_in_time() {
        let (_closed, addr) = silent();
        let invite = Invite {
            host: "Host".into(),
            course: course(),
            fingerprint: [1; 32],
            candidates: vec![Candidate {
                kind: CandidateKind::Stun,
                addr,
            }],
            nat: NatBehavior::Symmetric,
        };
        let joiner = Joiner::open(invite, &options(Duration::from_millis(1500)))
            .await
            .unwrap();
        let started = Instant::now();
        let result = joiner.connect().await;
        assert!(
            matches!(result, Err(JoinError::Unreachable(_))),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn the_host_gives_up_on_a_reply_whose_joiner_never_comes() {
        let (_closed, addr) = silent();
        let mut host = Host::open(&options(Duration::from_millis(500)))
            .await
            .unwrap();
        let reply = Reply {
            secret: [5; 16],
            candidates: vec![Candidate {
                kind: CandidateKind::Stun,
                addr,
            }],
            nat: NatBehavior::Unknown,
        };
        host.admit(&reply);
        match host.next().await {
            Some(HostEvent::GaveUp { secret, .. }) => assert_eq!(secret, [5; 16]),
            other => panic!("expected the host to give up, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn several_riders_join_one_host() {
        let opts = options(Duration::from_secs(10));
        let mut host = Host::open(&opts).await.unwrap();
        let first = Joiner::open(host.invite("Host", course()), &opts)
            .await
            .unwrap();
        let second = Joiner::open(host.invite("Host", course()), &opts)
            .await
            .unwrap();
        host.admit(&first.reply());
        host.admit(&second.reply());
        let (a, b) = tokio::join!(first.connect(), second.connect());
        assert!(a.is_ok() && b.is_ok());
        let mut secrets = [joined(&mut host).await.0, joined(&mut host).await.0];
        secrets.sort_unstable();
        let mut expected = [first.reply().secret, second.reply().secret];
        expected.sort_unstable();
        assert_eq!(secrets, expected);
    }

    #[test]
    fn hints_say_what_may_have_kept_riders_apart() {
        let local = [candidate(CandidateKind::Local, "192.168.1.2:5000")];
        let stun = [
            candidate(CandidateKind::Stun, "203.0.113.1:5000"),
            candidate(CandidateKind::Local, "192.168.1.2:5000"),
        ];
        let ipv6 = [candidate(CandidateKind::Public, "[2001:db8::1]:5000")];
        let (ei, sym) = (NatBehavior::EndpointIndependent, NatBehavior::Symmetric);

        assert_eq!(diagnose(&local, ei, &stun, ei), Hint::NoPublicAddress);
        assert_eq!(diagnose(&stun, ei, &local, ei), Hint::NoPublicAddress);
        assert_eq!(diagnose(&stun, sym, &stun, sym), Hint::BothSymmetric);
        assert_eq!(diagnose(&stun, sym, &stun, ei), Hint::OneSymmetric);
        assert_eq!(diagnose(&stun, ei, &stun, sym), Hint::OneSymmetric);
        assert_eq!(diagnose(&stun, ei, &stun, ei), Hint::Blocked);
        // A side reachable without punching: only a firewall stands in the way.
        assert_eq!(diagnose(&ipv6, sym, &stun, sym), Hint::Blocked);
    }
}
