//! Pinned QUIC, one-use admission, finite probing and owner-controlled endpoint lifetimes.
//! A host may hold an admitted rider's credential after a lost connection, so the same
//! certificate reconnects without new codes (#434), and lets it in again when the organizer
//! adds a reply for it once more (#536).

use std::collections::HashMap;
use std::future::Future;
use std::net::{SocketAddr, UdpSocket as StdSocket};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use quinn::{Endpoint, EndpointConfig, RecvStream, SendStream, VarInt};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until, timeout_at};

use crate::code::{CodeError, Fingerprint, Invite, Reply, RideId, RouteHash, Setup, valid_name};
use crate::crypto::{RandomError, random};
use crate::discovery::{self, GatherError, GatherOptions};
use crate::limits::{ConnectionBudget, ConnectionPermit, MAX_CONNECTIONS};
use crate::tls::{self, Pinned};

/// One absolute connection deadline, including discovery, TLS and admission.
pub const PUNCH_WINDOW: Duration = Duration::from_secs(60);
/// All accepted reply records, including tombstones, are bounded for one host lifetime.
pub const MAX_ADMISSIONS: usize = 16;
const AUTH_WAIT: Duration = Duration::from_secs(3);
const ATTEMPT: Duration = Duration::from_secs(5);
const PROBE_EVERY: Duration = Duration::from_millis(500);
const PROBE_LIMIT: usize = 100;
const PROBE: &[u8] = &[0, b'o', b'q'];
/// How often a host refreshes its advertised STUN mapping while nobody is connected through it.
/// Joiners only probe once their reply is admitted, and home and carrier NATs often forget an
/// idle UDP mapping within a minute; ICE refreshes its candidates as often (RFC 8445).
pub const MAPPING_REFRESH: Duration = Duration::from_secs(15);
const AUTH_LEN: usize = 81;
const ACK_OK: u8 = 1;
const ACK_WAIT: u8 = 2;
const ACK_REPLAY: u8 = 3;
const ACK_INVALID: u8 = 4;
const ACK_EXPIRED: u8 = 5;
/// Application close code asking the peer to reconnect: the connection failed, not the rider.
const RECONNECT: VarInt = VarInt::from_u32(2);
/// How often a host holding a credential probes the rider's last addresses, to keep its own
/// NAT open for the reconnection; a few bytes each, bounded by the hold and a packet budget.
const HOLD_PROBE_EVERY: Duration = Duration::from_secs(2);
const HOLD_PROBE_LIMIT: usize = 1024;
const OPEN: u8 = 0;
const LOST: u8 = 1;
const LEFT: u8 = 2;

/// Explicit network choices and the shared connection window.
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    /// Which interface, discovery services and address permissions may be used.
    pub gather: GatherOptions,
    /// At most one minute, starting before discovery. Shorter windows are useful for tests.
    pub window: Duration,
    /// The connection choice a host's invite tells its riders; [`Self::new`] gathers by it.
    pub setup: Setup,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            gather: GatherOptions::default(),
            window: PUNCH_WINDOW,
            setup: Setup::default(),
        }
    }
}

impl ConnectOptions {
    /// Connects as `setup` says: home-network addresses unless friends ride at separate
    /// homes, and a lookup asking `servers`, or ORQWA's own while that is empty (R192).
    #[must_use]
    pub fn new(setup: Setup, servers: &[String]) -> Self {
        let mut options = Self {
            setup,
            ..Self::default()
        };
        options.gather.policy.lan = setup.lan();
        if !setup.lookup() {
            options.gather.stun_servers.clear();
        } else if !servers.is_empty() {
            options.gather.stun_servers = servers.to_vec();
        }
        options
    }
}

/// A connection failure. Peer-provided close strings are never copied into diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// Discovery could not open an appropriate socket.
    #[error(transparent)]
    Discovery(#[from] GatherError),
    /// An invalid or forbidden invite/reply.
    #[error(transparent)]
    Code(#[from] CodeError),
    /// Secure entropy was unavailable.
    #[error(transparent)]
    Random(#[from] RandomError),
    /// No validated destination can be advertised.
    #[error(
        "No reachable addresses. Enable STUN, choose a public IPv6 interface, or explicitly use LAN/local-test mode."
    )]
    NoCandidates,
    /// A zero or over-one-minute connection window.
    #[error("The connection window must be greater than zero and at most one minute.")]
    Options,
    /// The complete connection/admission window elapsed.
    #[error(
        "The direct UDP connection did not finish in time. Check the codes, UDP firewall, IPv6 or LAN mode; some networks cannot connect without a relay."
    )]
    Timeout,
    /// None of the answering candidates proved possession of the invite's host key.
    #[error("The responding computer did not present the host certificate from the invite.")]
    WrongHost,
    /// A credential has already been consumed, expired or revoked.
    #[error("This reply has already been used or expired. Create a fresh reply.")]
    Replay,
    /// The bounded admission table is full; a fresh host lifetime is needed.
    #[error(
        "This invite has reached its reply-attempt limit. Start a new lobby for a fresh invite."
    )]
    Full,
    /// The authenticated peer went away or spoke an invalid protocol.
    #[error("The connection to the peer ended or used an invalid protocol.")]
    Lost,
    /// The negotiated QUIC idle timeout expired without communication.
    #[error("The group connection timed out without receiving packets from the peer.")]
    IdleTimeout,
    /// The peer's transport was reset, for example by a process restart.
    #[error("The peer reset the group connection.")]
    Reset,
    /// The remote application closed the connection, without copying its supplied reason.
    #[error("The peer closed the group connection.")]
    PeerClosed,
    /// The peer's transport rejected the connection, without copying its supplied reason.
    #[error("The peer's QUIC transport rejected the group connection.")]
    Transport,
    /// Local TLS configuration/certificate generation failed; contains only local diagnostics.
    #[error("Encryption could not be configured: {0}")]
    Encryption(String),
    /// A local socket could not be handed to the transport.
    #[error("The UDP transport could not be opened.")]
    Socket,
}

fn deadline(options: &ConnectOptions) -> Result<Instant, LinkError> {
    if options.window.is_zero() || options.window > PUNCH_WINDOW {
        return Err(LinkError::Options);
    }
    Ok(Instant::now() + options.window)
}

async fn until<T, E>(
    deadline: Instant,
    future: impl Future<Output = Result<T, E>>,
) -> Result<T, LinkError> {
    timeout_at(deadline, future)
        .await
        .map_err(|_| LinkError::Timeout)?
        .map_err(|_| LinkError::Lost)
}

struct Path {
    endpoint: Endpoint,
    prober: StdSocket,
    refresh: Option<SocketAddr>,
}
impl Drop for Path {
    fn drop(&mut self) {
        self.endpoint.close(VarInt::from_u32(0), b"closed");
    }
}
struct Local {
    runtime: Arc<crate::runtime::OwnedRuntime>,
    paths: Vec<Path>,
    candidates: Vec<crate::code::Candidate>,
    nat: crate::code::NatObservation,
}

impl Local {
    async fn open(
        options: &GatherOptions,
        deadline: Instant,
        server: Option<&quinn::ServerConfig>,
    ) -> Result<Self, LinkError> {
        let sockets = discovery::bind(options, deadline).await?;
        let mut local = Self {
            runtime: Arc::default(),
            paths: Vec::with_capacity(2),
            candidates: Vec::new(),
            nat: crate::code::NatObservation::None,
        };
        for path in sockets {
            let socket = path.socket.into_std().map_err(|_| LinkError::Socket)?;
            let prober = socket.try_clone().map_err(|_| LinkError::Socket)?;
            let endpoint = Endpoint::new(
                EndpointConfig::default(),
                server.cloned(),
                socket,
                local.runtime.clone(),
            )
            .map_err(|_| LinkError::Socket)?;
            // Codes report what STUN saw of IPv4, where address sharing happens.
            let ipv4 = prober.local_addr().is_ok_and(|a| a.is_ipv4());
            if ipv4 || local.nat == crate::code::NatObservation::None {
                local.nat = path.nat;
            }
            local.candidates.extend(path.candidates);
            local.paths.push(Path {
                endpoint,
                prober,
                refresh: path.refresh,
            });
        }
        if local.candidates.is_empty() {
            return Err(LinkError::NoCandidates);
        }
        Ok(local)
    }

    fn endpoint(&self, address: SocketAddr) -> Option<&Endpoint> {
        self.paths
            .iter()
            .find(|path| {
                path.endpoint
                    .local_addr()
                    .is_ok_and(|a| a.is_ipv4() == address.is_ipv4())
            })
            .map(|path| &path.endpoint)
    }

    fn probers(&self) -> Vec<StdSocket> {
        self.paths
            .iter()
            .filter_map(|p| p.prober.try_clone().ok())
            .collect()
    }
    fn close(&self) {
        for path in &self.paths {
            path.endpoint.close(VarInt::from_u32(0), b"closed");
        }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.close();
        self.runtime.shutdown();
    }
}

async fn probe(
    probers: Vec<StdSocket>,
    targets: Vec<SocketAddr>,
    deadline: Instant,
    stop: Arc<AtomicBool>,
) {
    probe_every(probers, targets, deadline, stop, PROBE_EVERY, PROBE_LIMIT).await;
}

async fn probe_every(
    probers: Vec<StdSocket>,
    targets: Vec<SocketAddr>,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    every: Duration,
    limit: usize,
) {
    let mut packets = 0;
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) && packets < limit {
        for target in &targets {
            if Instant::now() >= deadline || packets >= limit || stop.load(Ordering::Relaxed) {
                return;
            }
            if let Some(socket) = probers.iter().find(|p| {
                p.local_addr()
                    .is_ok_and(|a| a.is_ipv4() == target.is_ipv4())
            }) {
                let _ = socket.send_to(PROBE, target);
                packets += 1;
            }
        }
        sleep_until((Instant::now() + every).min(deadline)).await;
    }
}

struct LiveConnection {
    quic: quinn::Connection,
    client: Option<Fingerprint>,
    deadline: Instant,
    /// Why it ended, recorded before this side closes it: a local close replaces the
    /// transport's reason. [`OPEN`], [`LOST`] or [`LEFT`].
    ended: AtomicU8,
    _permit: Option<ConnectionPermit>,
    _owner: Option<Arc<Local>>,
}

impl Drop for LiveConnection {
    fn drop(&mut self) {
        self.quic.close(VarInt::from_u32(0), b"closed");
    }
}

/// An admitted connection. Clones share one lifetime guard; its last drop closes the connection.
/// At the joiner it also owns the endpoint; at the host its permit reserves global capacity.
#[derive(Clone)]
pub struct Connection(Arc<LiveConnection>);

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("peer", &self.peer_address())
            .finish_non_exhaustive()
    }
}

impl Connection {
    pub(crate) fn is_closed(&self) -> bool {
        self.0.quic.close_reason().is_some()
    }

    pub(crate) fn ended_normally(&self) -> bool {
        matches!(self.0.quic.close_reason(), Some(quinn::ConnectionError::ApplicationClosed(close)) if close.error_code == VarInt::from_u32(0))
    }

    pub(crate) fn failure(&self) -> LinkError {
        match self.0.quic.close_reason() {
            Some(quinn::ConnectionError::TimedOut) => LinkError::IdleTimeout,
            Some(quinn::ConnectionError::Reset) => LinkError::Reset,
            Some(quinn::ConnectionError::ApplicationClosed(_)) => LinkError::PeerClosed,
            Some(
                quinn::ConnectionError::TransportError(_)
                | quinn::ConnectionError::ConnectionClosed(_),
            ) => LinkError::Transport,
            _ => LinkError::Lost,
        }
    }

    /// At the host, the joiner certificate this connection proved and was admitted with.
    pub(crate) fn client(&self) -> Option<Fingerprint> {
        self.0.client
    }
    /// Peer address for local diagnostics; not forwarded to other riders.
    #[must_use]
    pub fn peer_address(&self) -> SocketAddr {
        self.0.quic.remote_address()
    }
    /// Remaining admission deadline; the lobby must also finish within it.
    #[must_use]
    pub fn admission_deadline(&self) -> Instant {
        self.0.deadline
    }
    /// Transport round trip estimate for the common countdown.
    #[must_use]
    pub fn rtt(&self) -> Duration {
        self.0.quic.rtt()
    }
    /// Opens a reliable stream; the caller applies the admission/message deadline.
    ///
    /// # Errors
    /// Returns [`LinkError::Lost`] if the connection ended.
    pub async fn open_bi(&self) -> Result<(SendStream, RecvStream), LinkError> {
        self.0.quic.open_bi().await.map_err(|_| LinkError::Lost)
    }
    /// Accepts a reliable stream; the caller applies the admission/message deadline.
    ///
    /// # Errors
    /// Returns [`LinkError::Lost`] if the connection ended.
    pub async fn accept_bi(&self) -> Result<(SendStream, RecvStream), LinkError> {
        self.0.quic.accept_bi().await.map_err(|_| LinkError::Lost)
    }
    /// Sends a small disposable packet, bounded independently of the caller.
    ///
    /// # Errors
    /// Returns [`LinkError::Lost`] if too large, disconnected or unavailable.
    pub fn send_datagram(&self, bytes: &[u8]) -> Result<(), LinkError> {
        if bytes.len() > 1024 {
            return Err(LinkError::Lost);
        }
        self.0
            .quic
            .send_datagram(bytes.to_vec().into())
            .map_err(|_| LinkError::Lost)
    }

    // Reserve most datagram capacity for ride state. At most eight ordinary 99-byte
    // voice packets can wait, allowing one simultaneous frame per remote speaker.
    pub(crate) fn send_voice(&self, bytes: &[u8]) -> Result<(), LinkError> {
        if self.0.quic.datagram_send_buffer_space() < 4096 - 800 + bytes.len() {
            return Ok(());
        }
        self.send_datagram(bytes)
    }
    /// Receives a datagram; protocol code must validate it before queueing or forwarding.
    ///
    /// # Errors
    /// Returns [`LinkError::Lost`] when disconnected. Transport buffers are fixed and small.
    pub async fn read_datagram(&self) -> Result<Vec<u8>, LinkError> {
        self.0
            .quic
            .read_datagram()
            .await
            .map(|b| b.to_vec())
            .map_err(|_| LinkError::Lost)
    }
    /// Waits for disconnection, without exposing peer-controlled close strings.
    pub async fn closed(&self) {
        let _ = self.0.quic.closed().await;
    }
    /// Closes the connection immediately; queued state is disposable.
    pub fn close(&self) {
        self.end(LEFT);
        self.0.quic.close(VarInt::from_u32(0), b"closed");
    }
    /// Closes a connection that stalled, telling the peer to reconnect rather than leave.
    pub(crate) fn close_for_reconnect(&self) {
        self.end(LOST);
        self.0.quic.close(RECONNECT, b"reconnect");
    }
    /// Whether it ended because the network failed, not by a deliberate close (#434): it
    /// timed out, or either side closed it for the other to reconnect. The first reason
    /// sticks, so a later local close cannot turn a peer's leaving into a lost connection.
    pub(crate) fn lost(&self) -> bool {
        self.end(OPEN) == LOST
    }
    // Records why the connection ended, the transport's reason first, else `local`'s, and
    // returns what is recorded. `OPEN` only reads, and leaves an open connection unrecorded.
    // A reset means the peer forgot the connection, as after closing it for a reconnection
    // whose close was lost; holding a seat too long costs less than dropping one in use.
    fn end(&self, local: u8) -> u8 {
        let reason = match self.0.quic.close_reason() {
            Some(quinn::ConnectionError::TimedOut | quinn::ConnectionError::Reset) => LOST,
            Some(quinn::ConnectionError::ApplicationClosed(close))
                if close.error_code == RECONNECT =>
            {
                LOST
            }
            Some(_) => LEFT,
            None => local,
        };
        if reason == OPEN {
            return self.0.ended.load(Ordering::Acquire);
        }
        match self
            .0
            .ended
            .compare_exchange(OPEN, reason, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => reason,
            Err(recorded) => recorded,
        }
    }
}

/// Link events have a bounded queue. Capacity exhaustion closes an unconsumed connection.
#[derive(Debug)]
pub enum HostEvent {
    /// A client proved an explicitly admitted, one-use credential.
    Joined(Connection),
    /// A pending reply expired; a new reply is needed rather than extending its lifetime.
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdmissionState {
    Pending,
    Used,
    Expired,
}
struct Admission {
    expires: Instant,
    state: AdmissionState,
    stop: Arc<AtomicBool>,
    /// The reply's addresses, probed again while the credential is held.
    targets: Vec<SocketAddr>,
    /// Whether the credential is held for a rider reconnecting rather than a fresh reply.
    held: bool,
}
struct Shared {
    admissions: Mutex<HashMap<Fingerprint, Admission>>,
    budget: ConnectionBudget,
    events: mpsc::Sender<HostEvent>,
    invite: Invite,
    sealed: AtomicBool,
}

impl Shared {
    fn admissions(&self) -> MutexGuard<'_, HashMap<Fingerprint, Admission>> {
        self.admissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn pending(&self) -> bool {
        let now = Instant::now();
        self.admissions()
            .values()
            .any(|a| a.state == AdmissionState::Pending && a.expires > now)
    }
}

/// Host owner. Dropping it closes every endpoint and aborts all accept/probe tasks.
pub struct Host {
    local: Local,
    shared: Arc<Shared>,
    events: mpsc::Receiver<HostEvent>,
    tasks: JoinSet<()>,
    options: ConnectOptions,
}

impl Host {
    /// Opens an empty app lobby, binding admission to this ride rather than mutable content.
    ///
    /// # Errors
    /// Returns [`LinkError`] for discovery, entropy or encryption failures.
    pub async fn lobby(host: &str, options: &ConnectOptions) -> Result<Self, LinkError> {
        Self::open(host, "Group ride", RouteHash::LOBBY, options).await
    }
    /// Creates this ride's certificate and sockets. Nothing is accepted before a reply is taken.
    ///
    /// # Errors
    /// Returns [`LinkError`] for invalid names, discovery, entropy or encryption failures.
    pub async fn open(
        host: &str,
        course: &str,
        route: RouteHash,
        options: &ConnectOptions,
    ) -> Result<Self, LinkError> {
        let end = deadline(options)?;
        if !valid_name(host) || !valid_name(course) {
            return Err(CodeError::Invalid.into());
        }
        let (server, fingerprint) = tls::server()?;
        let ride = RideId::generate()?;
        let local = Local::open(&options.gather, end, Some(&server)).await?;
        if Instant::now() >= end {
            return Err(LinkError::Timeout);
        }
        let invite = Invite {
            ride,
            fingerprint,
            route,
            host: host.to_owned(),
            course: course.to_owned(),
            candidates: local.candidates.clone(),
            nat: local.nat,
            setup: options.setup,
        };
        invite.encode(options.gather.policy)?;
        let (send, events) = mpsc::channel(MAX_CONNECTIONS);
        let shared = Arc::new(Shared {
            admissions: Mutex::new(HashMap::with_capacity(MAX_ADMISSIONS)),
            budget: ConnectionBudget::new(Instant::now().into_std()),
            events: send,
            invite,
            sealed: AtomicBool::new(false),
        });
        let mut tasks = JoinSet::new();
        for path in &local.paths {
            tasks.spawn(accept_all(path.endpoint.clone(), shared.clone()));
            if let Some(server) = path.refresh
                && let Ok(prober) = path.prober.try_clone()
            {
                tasks.spawn(keep_mapping(
                    prober,
                    server,
                    path.endpoint.clone(),
                    shared.clone(),
                    MAPPING_REFRESH,
                ));
            }
        }
        Ok(Self {
            local,
            shared,
            events,
            tasks,
            options: options.clone(),
        })
    }

    /// The per-ride invite. It contains public identity data, not an admission credential.
    #[must_use]
    pub fn invite(&self) -> &Invite {
        &self.shared.invite
    }

    /// Stops all pending replies and probes when the lobby countdown begins.
    /// Future pasted replies must be refused by the lobby owner.
    pub fn seal(&mut self) {
        self.shared.sealed.store(true, Ordering::Relaxed);
        for admission in self.shared.admissions().values_mut() {
            if admission.state == AdmissionState::Pending {
                admission.state = AdmissionState::Expired;
                admission.stop.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Explicitly admits one reply, validating its binding and policy before any probes.
    /// Consumed and expired records remain bounded tombstones for this host lifetime. Adding
    /// a reply again for a certificate whose seat was released lets that rider back in, and
    /// one for a seat still held just lets the host probe its new addresses too (#536).
    ///
    /// # Errors
    /// Returns [`LinkError`] for a wrong ride, forbidden destination, a reply still pending
    /// or connected, or an excess reply.
    pub fn admit(&mut self, reply: &Reply) -> Result<(), LinkError> {
        reply.matches(self.invite())?;
        reply.encode(self.options.gather.policy)?;
        let targets: Vec<SocketAddr> = reply.candidates.iter().map(|c| c.addr()).collect();
        let expires = Instant::now() + self.options.window;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut admissions = self.shared.admissions();
            let full = admissions.len() >= MAX_ADMISSIONS;
            match admissions.get_mut(&reply.client) {
                Some(entry) if entry.state == AdmissionState::Pending && entry.held => {
                    let (until, stop) = (entry.expires, entry.stop.clone());
                    for target in &targets {
                        if !entry.targets.contains(target) {
                            entry.targets.push(*target);
                        }
                    }
                    drop(admissions);
                    self.tasks
                        .spawn(probe(self.local.probers(), targets, until, stop));
                    return Ok(());
                }
                // The organizer lets a rider whose seat was released back in, with the same
                // certificate: only its private key can use it, as on the first admission.
                Some(entry) if entry.state == AdmissionState::Expired => {
                    *entry = Admission {
                        expires,
                        state: AdmissionState::Pending,
                        stop: stop.clone(),
                        targets: targets.clone(),
                        held: false,
                    };
                }
                Some(_) => return Err(LinkError::Replay),
                None if full => return Err(LinkError::Full),
                None => {
                    admissions.insert(
                        reply.client,
                        Admission {
                            expires,
                            state: AdmissionState::Pending,
                            stop: stop.clone(),
                            targets: targets.clone(),
                            held: false,
                        },
                    );
                }
            }
        }
        self.tasks
            .spawn(probe(self.local.probers(), targets, expires, stop));
        let shared = self.shared.clone();
        let client = reply.client;
        self.tasks.spawn(async move {
            sleep_until(expires).await;
            let timed_out = {
                let mut admissions = shared.admissions();
                // A credential held since is no longer this admission's to expire.
                if let Some(entry) = admissions.get_mut(&client)
                    && entry.state == AdmissionState::Pending
                    && entry.expires == expires
                {
                    entry.state = AdmissionState::Expired;
                    entry.stop.store(true, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            };
            if timed_out {
                let _ = shared.events.try_send(HostEvent::Expired);
            }
        });
        Ok(())
    }

    /// Revokes a pending reply without removing its replay-prevention tombstone.
    pub fn revoke(&mut self, reply: &Reply) {
        self.release(reply.client);
    }

    /// Lets the rider whose connection with `client` was lost reconnect with the same
    /// certificate until `until` (#434), probing their last address and the reply's so this
    /// computer's NAT lets them in. Only a credential that was already used can be held: it
    /// never admits anyone new, and its tombstone keeps it out of the admission budget.
    pub(crate) fn hold(&mut self, client: Fingerprint, until: Instant, last: SocketAddr) {
        let stop = Arc::new(AtomicBool::new(false));
        let mut targets = {
            let mut admissions = self.shared.admissions();
            let Some(entry) = admissions
                .get_mut(&client)
                .filter(|entry| entry.state == AdmissionState::Used)
            else {
                return;
            };
            entry.state = AdmissionState::Pending;
            entry.expires = until;
            entry.stop = stop.clone();
            entry.held = true;
            entry.targets.clone()
        };
        if !targets.contains(&last) {
            targets.insert(0, last);
        }
        self.tasks.spawn(probe_every(
            self.local.probers(),
            targets,
            until,
            stop,
            HOLD_PROBE_EVERY,
            HOLD_PROBE_LIMIT,
        ));
    }

    /// Ends a held or pending credential for good; the rider needs fresh codes to come back.
    pub(crate) fn release(&mut self, client: Fingerprint) {
        if let Some(entry) = self.shared.admissions().get_mut(&client) {
            entry.state = AdmissionState::Expired;
            entry.held = false;
            entry.stop.store(true, Ordering::Relaxed);
        }
    }

    /// The next admitted connection or expiry. Completed owned jobs are reaped as events arrive.
    pub async fn next(&mut self) -> Option<HostEvent> {
        loop {
            tokio::select! {event=self.events.recv()=>return event,Some(_)=self.tasks.join_next()=>{}}
        }
    }

    /// Closes endpoints immediately and waits a bounded time for task/transport cleanup.
    pub async fn close(mut self) {
        self.local.close();
        self.tasks.abort_all();
        let end = Instant::now() + Duration::from_secs(1);
        while matches!(timeout_at(end, self.tasks.join_next()).await, Ok(Some(_))) {}
        for path in &self.local.paths {
            let _ = timeout_at(end, path.endpoint.wait_idle()).await;
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.local.close();
        self.tasks.abort_all();
        for entry in self.shared.admissions().values() {
            entry.stop.store(true, Ordering::Relaxed);
        }
    }
}

// A Binding indication refreshes the mapping the invite advertises; servers never answer it.
// Once a rider is connected, QUIC keep-alives refresh the same mapping instead.
async fn keep_mapping(
    prober: StdSocket,
    server: SocketAddr,
    endpoint: Endpoint,
    shared: Arc<Shared>,
    every: Duration,
) {
    loop {
        sleep_until(Instant::now() + every).await;
        if shared.sealed.load(Ordering::Relaxed) {
            return;
        }
        if endpoint.open_connections() > 0 {
            continue;
        }
        let Ok(transaction) = random() else {
            return;
        };
        let _ = prober.send_to(&crate::stun::binding_indication(&transaction), server);
    }
}

async fn accept_all(endpoint: Endpoint, shared: Arc<Shared>) {
    let mut jobs = JoinSet::new();
    loop {
        tokio::select! {
            incoming=endpoint.accept()=>{
                let Some(incoming)=incoming else {return;};
                if jobs.len()>=MAX_CONNECTIONS||!shared.pending() {incoming.refuse();continue;}
                // Reserve global/per-source/rate capacity before accepting TLS or spawning work.
                let Some(permit)=shared.budget.try_acquire(incoming.remote_address().ip(),Instant::now().into_std()) else {incoming.refuse();continue;};
                if !incoming.remote_address_validated() {let _=incoming.retry();continue;}
                jobs.spawn(welcome(incoming,shared.clone(),permit));
            }
            Some(_)=jobs.join_next()=>{}
        }
    }
}

// The joiner's credential is its TLS client certificate; this binds that proof to one ride.
fn auth_message(invite: &Invite) -> [u8; AUTH_LEN] {
    let mut bytes = [0; AUTH_LEN];
    bytes[0] = 1;
    bytes[1..17].copy_from_slice(&invite.ride.0);
    bytes[17..49].copy_from_slice(&invite.fingerprint.0);
    bytes[49..81].copy_from_slice(&invite.route.0);
    bytes
}

fn take_credential(shared: &Shared, bytes: &[u8], client: Fingerprint) -> (u8, Option<Instant>) {
    if bytes.len() != AUTH_LEN
        || bytes[0] != 1
        || bytes[1..17] != shared.invite.ride.0
        || bytes[17..49] != shared.invite.fingerprint.0
        || bytes[49..81] != shared.invite.route.0
    {
        return (ACK_INVALID, None);
    }
    let mut admissions = shared.admissions();
    let Some(entry) = admissions.get_mut(&client) else {
        return (ACK_WAIT, None);
    };
    if entry.state == AdmissionState::Used {
        return (ACK_REPLAY, None);
    }
    if entry.state == AdmissionState::Expired || Instant::now() >= entry.expires {
        entry.state = AdmissionState::Expired;
        entry.stop.store(true, Ordering::Relaxed);
        return (ACK_EXPIRED, None);
    }
    entry.state = AdmissionState::Used;
    entry.held = false;
    entry.stop.store(true, Ordering::Relaxed);
    // A held credential may last minutes; the lobby handshake still gets one connection window.
    (
        ACK_OK,
        Some(entry.expires.min(Instant::now() + PUNCH_WINDOW)),
    )
}

async fn welcome(incoming: quinn::Incoming, shared: Arc<Shared>, permit: ConnectionPermit) {
    let end = Instant::now() + AUTH_WAIT;
    let Ok(quic) = until(end, async { incoming.await }).await else {
        return;
    };
    let Some(client) = tls::client_pin(&quic) else {
        quic.close(VarInt::from_u32(1), b"admission failed");
        return;
    };
    let result = async {
        let (mut send, mut recv) = until(end, quic.accept_bi()).await?;
        let bytes = until(end, recv.read_to_end(AUTH_LEN)).await?;
        let (ack, expires) = take_credential(&shared, &bytes, client);
        until(end, send.write_all(&[ack])).await?;
        send.finish().map_err(|_| LinkError::Lost)?;
        if ack != ACK_OK {
            let _ = timeout_at(end, send.stopped()).await;
            return Err(LinkError::Replay);
        }
        Ok(expires.expect("an accepted credential has an expiry"))
    }
    .await;
    match result {
        Ok(expires) => {
            let connection = Connection(Arc::new(LiveConnection {
                quic,
                client: Some(client),
                // AUTH_WAIT bounds only the credential exchange. The lobby handshake that
                // follows shares the admission's one deadline (R161), as the joiner's does.
                deadline: expires,
                ended: AtomicU8::new(OPEN),
                _permit: Some(permit),
                _owner: None,
            }));
            // A full/closed queue drops the wrapper, which closes this connection and permit.
            let _ = shared.events.try_send(HostEvent::Joined(connection));
        }
        Err(_) => {
            quic.close(VarInt::from_u32(1), b"admission failed");
        }
    }
}

/// Joiner owner before connecting. Its consuming connect future owns all sockets and tasks:
/// cancelling it closes everything. Successful connections take ownership of its endpoint.
pub struct Joiner {
    local: Option<Local>,
    invite: Invite,
    reply: Reply,
    deadline: Instant,
    retry: Duration,
    client: quinn::ClientConfig,
    pinned: Arc<Pinned>,
    tasks: JoinSet<()>,
}

/// A joiner's certificate and private key, kept to answer the same group again: a reply
/// with it lets the organizer give the rider their old seat back (#536). The key never
/// leaves this computer; it is deliberately not printed.
#[derive(Clone)]
pub struct Credential(tls::Identity);

impl Credential {
    /// A fresh certificate, as each new group gets.
    ///
    /// # Errors
    /// [`LinkError::Encryption`] if no certificate can be made.
    pub fn generate() -> Result<Self, LinkError> {
        tls::Identity::generate().map(Self)
    }
    /// The certificate's pin, as a reply carries it.
    #[must_use]
    pub const fn pin(&self) -> Fingerprint {
        self.0.pin()
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Credential").field(&self.0.pin()).finish()
    }
}

impl Joiner {
    /// Answers an app-lobby invitation without requiring a preexisting local course.
    ///
    /// # Errors
    /// Returns [`LinkError`] for legacy course invites or transport setup failure.
    pub async fn lobby(invite: Invite, options: &ConnectOptions) -> Result<Self, LinkError> {
        Self::open(invite, RouteHash::LOBBY, options).await
    }
    /// Answers an app-lobby invitation like [`Joiner::lobby`], with a certificate kept from
    /// an earlier answer to the same group (#536).
    ///
    /// # Errors
    /// Returns [`LinkError`] for legacy course invites or transport setup failure.
    pub async fn lobby_as(
        invite: Invite,
        options: &ConnectOptions,
        credential: &Credential,
    ) -> Result<Self, LinkError> {
        Self::open_as(invite, RouteHash::LOBBY, options, credential.0.clone()).await
    }
    /// Validates the expected route and destination policy before discovery or probing.
    ///
    /// # Errors
    /// Returns [`LinkError`] for a mismatched course, forbidden invite, timeout or setup error.
    pub async fn open(
        invite: Invite,
        route: RouteHash,
        options: &ConnectOptions,
    ) -> Result<Self, LinkError> {
        Self::open_as(invite, route, options, tls::Identity::generate()?).await
    }
    async fn open_as(
        invite: Invite,
        route: RouteHash,
        options: &ConnectOptions,
        identity: tls::Identity,
    ) -> Result<Self, LinkError> {
        let end = deadline(options)?;
        if invite.route != route {
            return Err(CodeError::WrongRide.into());
        }
        invite.encode(options.gather.policy)?;
        let (client, pinned) = tls::client(invite.fingerprint, &identity)?;
        let local = Local::open(&options.gather, end, None).await?;
        let reply = Reply {
            ride: invite.ride,
            fingerprint: invite.fingerprint,
            route,
            client: identity.pin(),
            candidates: local.candidates.clone(),
            nat: local.nat,
        };
        reply.encode(options.gather.policy)?;
        let mut tasks = JoinSet::new();
        tasks.spawn(probe(
            local.probers(),
            invite.candidates.iter().map(|c| c.addr()).collect(),
            end,
            Arc::new(AtomicBool::new(false)),
        ));
        Ok(Self {
            local: Some(local),
            invite,
            reply,
            deadline: end,
            retry: ATTEMPT.min(options.window / 4),
            client,
            pinned,
            tasks,
        })
    }

    /// A one-use reply; pass it privately to the host before the original deadline.
    #[must_use]
    pub fn reply(&self) -> &Reply {
        &self.reply
    }

    /// Connects and proves the credential only after certificate pin and signature checks.
    /// This consumes the owner, so cancellation/failure automatically tears down the sockets.
    ///
    /// # Errors
    /// Returns [`LinkError`] for expiry, wrong host, replay or transport failure.
    pub async fn connect(self) -> Result<Connection, LinkError> {
        self.connect_resumable()
            .await
            .map(|(connection, _)| connection)
    }

    /// Connects like [`Joiner::connect`], also keeping what reaching the host again needs.
    pub(crate) async fn connect_resumable(mut self) -> Result<(Connection, Resume), LinkError> {
        loop {
            let round = Instant::now();
            if round >= self.deadline {
                return Err(if self.pinned.mismatched() {
                    LinkError::WrongHost
                } else {
                    LinkError::Timeout
                });
            }
            let local = self.local.as_ref().ok_or(LinkError::Lost)?;
            let end = (round + self.retry).min(self.deadline);
            if let Some(quic) = reach(local, &self.invite, &self.client, self.retry, end).await {
                match authenticate(&quic, &self.invite, self.deadline).await {
                    Ok(ACK_OK) => {
                        self.tasks.abort_all();
                        let local = Arc::new(self.local.take().ok_or(LinkError::Lost)?);
                        let resume = Resume {
                            local: local.clone(),
                            invite: self.invite.clone(),
                            client: self.client.clone(),
                            retry: self.retry,
                        };
                        return Ok((live(quic, self.deadline, local), resume));
                    }
                    Ok(ACK_WAIT) => {
                        quic.close(VarInt::from_u32(0), b"waiting for reply");
                    }
                    Ok(ACK_REPLAY | ACK_EXPIRED) => {
                        quic.close(VarInt::from_u32(1), b"admission failed");
                        return Err(LinkError::Replay);
                    }
                    Ok(_) => {
                        quic.close(VarInt::from_u32(1), b"admission failed");
                        return Err(LinkError::Lost);
                    }
                    Err(error) => {
                        quic.close(VarInt::from_u32(1), b"admission failed");
                        return Err(error);
                    }
                }
            }
            sleep_until((round + self.retry).min(self.deadline)).await;
        }
    }
}

fn live(quic: quinn::Connection, deadline: Instant, local: Arc<Local>) -> Connection {
    Connection(Arc::new(LiveConnection {
        quic,
        client: None,
        deadline,
        ended: AtomicU8::new(OPEN),
        _permit: None,
        _owner: Some(local),
    }))
}

async fn reach(
    local: &Local,
    invite: &Invite,
    client: &quinn::ClientConfig,
    retry: Duration,
    end: Instant,
) -> Option<quinn::Connection> {
    let mut attempts = JoinSet::new();
    let stagger = (retry / u32::try_from(invite.candidates.len() + 1).expect("bounded candidates"))
        .min(Duration::from_millis(250));
    for (index, candidate) in invite.candidates.iter().enumerate() {
        let Some(endpoint) = local.endpoint(candidate.addr()) else {
            continue;
        };
        let endpoint = endpoint.clone();
        let client = client.clone();
        let address = candidate.addr();
        let delay = stagger * u32::try_from(index).expect("bounded candidates");
        attempts.spawn(async move {
            // Give a working path time to win before spending another TLS/rate permit.
            sleep_until(Instant::now() + delay).await;
            if Instant::now() >= end {
                return Err(LinkError::Timeout);
            }
            let connecting = endpoint
                .connect_with(client, address, tls::SERVER_NAME)
                .map_err(|_| LinkError::Lost)?;
            until(end, connecting).await
        });
    }
    while let Some(result) = attempts.join_next().await {
        if let Ok(Ok(connection)) = result {
            return Some(connection);
        }
    }
    None
}

async fn authenticate(
    connection: &quinn::Connection,
    invite: &Invite,
    deadline: Instant,
) -> Result<u8, LinkError> {
    let (mut send, mut recv) = until(deadline, connection.open_bi()).await?;
    until(deadline, send.write_all(&auth_message(invite))).await?;
    send.finish().map_err(|_| LinkError::Lost)?;
    match until(deadline, recv.read_to_end(1)).await?.as_slice() {
        [ack] => Ok(*ack),
        _ => Err(LinkError::Lost),
    }
}

/// What a joiner keeps to reach the same host again after a lost connection (#434): the
/// invitation, its own per-ride certificate and its sockets. No new codes or discovery.
pub(crate) struct Resume {
    local: Arc<Local>,
    invite: Invite,
    client: quinn::ClientConfig,
    retry: Duration,
}

impl Resume {
    /// One attempt to reconnect, within at most one connection window from now.
    ///
    /// # Errors
    /// [`LinkError::Replay`] once the host no longer holds this rider's seat, so retrying is
    /// pointless; [`LinkError::Timeout`] if the host was unreachable or has not yet noticed
    /// the loss; [`LinkError::Lost`] for an invalid answer.
    pub(crate) async fn connect(&self) -> Result<Connection, LinkError> {
        let end = Instant::now() + self.retry;
        let quic = reach(&self.local, &self.invite, &self.client, self.retry, end)
            .await
            .ok_or(LinkError::Timeout)?;
        let deadline = Instant::now() + PUNCH_WINDOW;
        match authenticate(&quic, &self.invite, end.max(Instant::now() + AUTH_WAIT)).await {
            Ok(ACK_OK) => Ok(live(quic, deadline, self.local.clone())),
            // The host still counts the old connection, or never knew this one.
            Ok(ACK_REPLAY | ACK_WAIT) => {
                quic.close(VarInt::from_u32(0), b"waiting for the host");
                Err(LinkError::Timeout)
            }
            Ok(ACK_EXPIRED) => {
                quic.close(VarInt::from_u32(0), b"seat released");
                Err(LinkError::Replay)
            }
            Ok(_) | Err(_) => {
                quic.close(VarInt::from_u32(1), b"admission failed");
                Err(LinkError::Lost)
            }
        }
    }
}

impl Drop for Joiner {
    fn drop(&mut self) {
        self.tasks.abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::ConnectionMode;
    use crate::policy::AddressPolicy;
    use tokio::time::{sleep, timeout};

    fn options() -> ConnectOptions {
        ConnectOptions {
            gather: GatherOptions {
                policy: AddressPolicy {
                    lan: false,
                    loopback: true,
                },
                interface: None,
                stun_servers: Vec::new(),
            },
            window: Duration::from_secs(1),
            ..ConnectOptions::default()
        }
    }
    fn route() -> RouteHash {
        RouteHash([7; 32])
    }

    #[test]
    fn home_and_internet_choices_keep_discovery_and_local_permissions_explicit() {
        let internet = ConnectOptions::new(Setup::new(ConnectionMode::Internet, true), &[]);
        assert!(!internet.gather.policy.lan && !internet.gather.policy.loopback);
        assert_ne!(internet.gather.stun_servers, Vec::<String>::new());
        let home = ConnectOptions::new(Setup::new(ConnectionMode::Home, true), &[]);
        assert!(home.gather.policy.lan && !home.gather.policy.loopback);
        assert_eq!(home.gather.stun_servers, Vec::<String>::new());
        let mixed = ConnectOptions::new(Setup::new(ConnectionMode::Mixed, true), &[]);
        assert!(mixed.gather.policy.lan && !mixed.gather.stun_servers.is_empty());
        for mode in [
            ConnectionMode::Internet,
            ConnectionMode::Home,
            ConnectionMode::Mixed,
        ] {
            assert_eq!(
                ConnectOptions::new(Setup::new(mode, false), &[])
                    .gather
                    .stun_servers,
                Vec::<String>::new()
            );
            assert!(
                !ConnectOptions::new(Setup::new(mode, false), &[])
                    .gather
                    .policy
                    .loopback
            );
        }
        assert!(ConnectionMode::from_key("unexpected").is_none());
    }

    #[test]
    fn named_lookup_servers_replace_the_defaults_only_where_a_lookup_is_made() {
        let named = vec!["stun.example.org:3478".to_owned()];
        for mode in [ConnectionMode::Internet, ConnectionMode::Mixed] {
            assert_eq!(
                ConnectOptions::new(Setup::new(mode, true), &named)
                    .gather
                    .stun_servers,
                named
            );
            assert_eq!(
                ConnectOptions::new(Setup::new(mode, false), &named)
                    .gather
                    .stun_servers,
                Vec::<String>::new()
            );
        }
        assert_eq!(
            ConnectOptions::new(Setup::new(ConnectionMode::Home, true), &named)
                .gather
                .stun_servers,
            Vec::<String>::new()
        );
    }
    async fn host() -> Host {
        Host::open("Anna", "Galibier", route(), &options())
            .await
            .unwrap()
    }
    async fn joiner(host: &Host) -> Joiner {
        Joiner::open(host.invite().clone(), route(), &options())
            .await
            .unwrap()
    }
    async fn joined(host: &mut Host) -> Connection {
        match timeout(Duration::from_secs(2), host.next())
            .await
            .unwrap()
            .unwrap()
        {
            HostEvent::Joined(connection) => connection,
            HostEvent::Expired => panic!("reply expired"),
        }
    }
    fn ipv4(host: &Host) -> &Path {
        host.local
            .paths
            .iter()
            .find(|p| p.endpoint.local_addr().unwrap().is_ipv4())
            .expect("IPv4 loopback path")
    }

    #[tokio::test]
    async fn an_admitted_host_connection_keeps_the_whole_admission_window_for_the_lobby() {
        let mut opts = options();
        opts.window = Duration::from_secs(10);
        let mut host = Host::open("Anna", "Galibier", route(), &opts)
            .await
            .unwrap();
        let joiner = Joiner::open(host.invite().clone(), route(), &opts)
            .await
            .unwrap();
        let admitted = Instant::now();
        host.admit(joiner.reply()).unwrap();
        let client = joiner.connect().await.unwrap();
        let peer = joined(&mut host).await;
        assert!(peer.admission_deadline() >= admitted + opts.window);
        drop((client, peer));
        host.close().await;
    }

    // At most one refresh may already be on its way when the condition changes.
    async fn falls_silent(server: &tokio::net::UdpSocket, every: Duration) -> bool {
        let mut packet = [0; 64];
        for _ in 0..2 {
            if timeout(every * 4, server.recv_from(&mut packet))
                .await
                .is_err()
            {
                return true;
            }
        }
        false
    }

    #[tokio::test]
    async fn an_advertised_mapping_is_refreshed_only_while_waiting_for_a_rider() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut host = host().await;
        let path = ipv4(&host);
        let every = Duration::from_millis(30);
        host.tasks.spawn(keep_mapping(
            path.prober.try_clone().unwrap(),
            server.local_addr().unwrap(),
            path.endpoint.clone(),
            host.shared.clone(),
            every,
        ));
        let mut packet = [0; 64];
        let (len, _) = timeout(every * 4, server.recv_from(&mut packet))
            .await
            .expect("a refresh while nobody is connected")
            .unwrap();
        assert_eq!(len, 20);
        assert_eq!(packet[..2], [0x00, 0x11], "a Binding indication");
        assert_eq!(packet[4..8], [0x21, 0x12, 0xa4, 0x42]);

        // Each path refreshes its own mapping, so connect through the refreshed one.
        let mut invite = host.invite().clone();
        invite.candidates.retain(|c| c.addr().is_ipv4());
        let joiner = Joiner::open(invite, route(), &options()).await.unwrap();
        host.admit(joiner.reply()).unwrap();
        let client = joiner.connect().await.unwrap();
        let peer = joined(&mut host).await;
        assert!(
            falls_silent(&server, every).await,
            "QUIC keep-alives refresh the mapping once a rider is connected"
        );
        drop((client, peer));
        host.seal();
        assert!(
            falls_silent(&server, every).await,
            "a sealed invite needs no refresh"
        );
        host.close().await;
    }

    #[tokio::test]
    async fn only_an_admitted_joiner_gets_a_connection_and_both_can_talk() {
        let mut host = host().await;
        let joiner = joiner(&host).await;
        host.admit(joiner.reply()).unwrap();
        let client = joiner.connect().await.unwrap();
        let peer = joined(&mut host).await;
        client.send_datagram(&[7]).unwrap();
        assert_eq!(peer.read_datagram().await.unwrap(), vec![7]);
        peer.send_datagram(&[8]).unwrap();
        assert_eq!(client.read_datagram().await.unwrap(), vec![8]);
        drop(client);
        timeout(Duration::from_secs(1), peer.closed())
            .await
            .unwrap();
        host.close().await;
    }

    #[tokio::test]
    async fn a_reply_may_be_pasted_after_the_joiner_has_started_connecting() {
        let mut host = host().await;
        let joiner = joiner(&host).await;
        let reply = joiner.reply().clone();
        let task = tokio::spawn(joiner.connect());
        sleep(Duration::from_millis(40)).await;
        host.admit(&reply).unwrap();
        let client = task.await.unwrap().unwrap();
        let peer = joined(&mut host).await;
        peer.close();
        drop(client);
        host.close().await;
    }

    #[tokio::test]
    async fn disconnect_diagnostics_never_copy_peer_supplied_close_text() {
        let mut host = host().await;
        let joiner = joiner(&host).await;
        host.admit(joiner.reply()).unwrap();
        let client = joiner.connect().await.unwrap();
        let peer = joined(&mut host).await;
        peer.0.quic.close(
            VarInt::from_u32(42),
            b"private peer-controlled text\nforged log line",
        );
        timeout(Duration::from_secs(1), client.closed())
            .await
            .unwrap();
        assert!(!client.ended_normally());
        assert!(matches!(client.failure(), LinkError::PeerClosed));
        assert_eq!(
            client.failure().to_string(),
            "The peer closed the group connection."
        );
        host.close().await;
    }

    #[tokio::test]
    async fn wrong_course_and_wrong_ride_replies_are_refused_before_network_work() {
        let mut first = host().await;
        let second = host().await;
        let joiner = joiner(&second).await;
        assert!(matches!(
            first.admit(joiner.reply()),
            Err(LinkError::Code(CodeError::WrongRide))
        ));
        assert!(matches!(
            Joiner::open(first.invite().clone(), RouteHash([9; 32]), &options()).await,
            Err(LinkError::Code(CodeError::WrongRide))
        ));
        first.close().await;
        second.close().await;
    }

    #[tokio::test]
    async fn a_different_certificate_cannot_receive_the_admission_credential() {
        let mut host = host().await;
        let legitimate = joiner(&host).await;
        host.admit(legitimate.reply()).unwrap();
        let mut invite = host.invite().clone();
        invite.fingerprint.0[0] ^= 1;
        let mut opts = options();
        opts.window = Duration::from_millis(180);
        let impostor = Joiner::open(invite, route(), &opts).await.unwrap();
        assert!(matches!(
            impostor.connect().await,
            Err(LinkError::WrongHost)
        ));
        assert_eq!(
            host.shared
                .admissions()
                .get(&legitimate.reply().client)
                .unwrap()
                .state,
            AdmissionState::Pending
        );
        drop(legitimate);
        host.close().await;
    }

    #[tokio::test]
    async fn a_used_joiner_key_and_repeated_reply_cannot_admit_another_connection() {
        let mut host = host().await;
        let first = joiner(&host).await;
        let used = first.reply().clone();
        let same_key = first.client.clone();
        host.admit(&used).unwrap();
        let client = first.connect().await.unwrap();
        let peer = joined(&mut host).await;
        assert!(matches!(host.admit(&used), Err(LinkError::Replay)));
        let pending = joiner(&host).await;
        host.admit(pending.reply()).unwrap();
        let mut replay = joiner(&host).await;
        replay.client = same_key;
        assert!(matches!(replay.connect().await, Err(LinkError::Replay)));
        assert!(
            timeout(Duration::from_millis(20), host.next())
                .await
                .is_err()
        );
        drop((client, peer, pending));
        host.close().await;
    }

    #[tokio::test]
    async fn a_copied_reply_admits_nobody_but_the_joiner_that_created_it() {
        let mut host = host().await;
        let legitimate = joiner(&host).await;
        host.admit(legitimate.reply()).unwrap();
        // Someone who saw the reply in a chat reuses it from another computer.
        let mut opts = options();
        opts.window = Duration::from_millis(300);
        let mut copier = Joiner::open(host.invite().clone(), route(), &opts)
            .await
            .unwrap();
        copier.reply = legitimate.reply().clone();
        assert!(matches!(copier.connect().await, Err(LinkError::Timeout)));
        assert!(
            timeout(Duration::from_millis(20), host.next())
                .await
                .is_err(),
            "the copier was never admitted"
        );
        assert_eq!(
            host.shared
                .admissions()
                .get(&legitimate.reply().client)
                .unwrap()
                .state,
            AdmissionState::Pending
        );
        let expected = legitimate.reply().client;
        let client = legitimate.connect().await.unwrap();
        let peer = joined(&mut host).await;
        assert_eq!(peer.client(), Some(expected));
        drop((client, peer));
        host.close().await;
    }

    #[tokio::test]
    async fn a_joiner_without_a_certificate_is_never_admitted() {
        let mut host = host().await;
        let pending = joiner(&host).await;
        host.admit(pending.reply()).unwrap();
        // Pins the real host, like a joiner, but offers no certificate of its own.
        let identity = tls::Identity::generate().unwrap();
        let (_, pinned) = tls::client(host.invite().fingerprint, &identity).unwrap();
        let mut without_certificate =
            rustls::ClientConfig::builder_with_provider(crate::crypto::provider())
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(pinned)
                .with_no_client_auth();
        without_certificate.alpn_protocols = vec![tls::ALPN.to_vec()];
        let config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(without_certificate).unwrap(),
        ));
        let address = ipv4(&host).endpoint.local_addr().unwrap();
        let endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        let attempt = endpoint
            .connect_with(config, address, tls::SERVER_NAME)
            .unwrap();
        let result = timeout(Duration::from_secs(2), async {
            let connection = attempt.await?;
            // TLS 1.3 clients finish before the server checks their certificate.
            connection.accept_uni().await.map(|_| ())
        })
        .await
        .unwrap();
        assert!(result.is_err());
        assert!(
            timeout(Duration::from_millis(20), host.next())
                .await
                .is_err()
        );
        drop(pending);
        host.close().await;
    }

    #[tokio::test]
    async fn revoked_replies_remain_tombstones_until_added_again_and_the_table_is_bounded() {
        let mut host = host().await;
        let mut kept = Vec::new();
        for _ in 0..MAX_ADMISSIONS {
            let joiner = joiner(&host).await;
            let reply = joiner.reply().clone();
            host.admit(&reply).unwrap();
            host.revoke(&reply);
            kept.push(joiner);
        }
        let extra = joiner(&host).await;
        assert!(matches!(host.admit(extra.reply()), Err(LinkError::Full)));
        // A revoked reply admits nobody until the organizer adds it again, which takes no
        // further room in the table (#536).
        let revoked = kept.pop().unwrap();
        let reply = revoked.reply().clone();
        let other = kept.pop().unwrap();
        host.admit(other.reply()).unwrap();
        assert!(matches!(revoked.connect().await, Err(LinkError::Replay)));
        host.admit(&reply).unwrap();
        assert!(matches!(host.admit(&reply), Err(LinkError::Replay)));
        assert_eq!(host.shared.admissions().len(), MAX_ADMISSIONS);
        drop((kept, other, extra));
        host.close().await;
    }

    #[tokio::test]
    async fn a_rider_released_and_added_again_gets_in_with_the_same_certificate() {
        let mut host = host().await;
        let credential = Credential::generate().unwrap();
        let first = Joiner::open_as(
            host.invite().clone(),
            route(),
            &options(),
            credential.0.clone(),
        )
        .await
        .unwrap();
        let reply = first.reply().clone();
        assert_eq!(reply.client, credential.pin());
        host.admit(&reply).unwrap();
        let (client, resume) = first.connect_resumable().await.unwrap();
        let peer = joined(&mut host).await;
        let mut others = Vec::new();
        for _ in 1..MAX_ADMISSIONS {
            let other = joiner(&host).await;
            host.admit(other.reply()).unwrap();
            others.push(other);
        }
        // While connected, the same reply admits no second connection.
        assert!(matches!(host.admit(&reply), Err(LinkError::Replay)));
        // While the seat is held, a reply for it is welcome even with the table full: the
        // rider who gave up reconnecting a moment early comes back to it (#536).
        host.hold(
            credential.pin(),
            Instant::now() + Duration::from_secs(10),
            peer.peer_address(),
        );
        host.admit(&reply).unwrap();
        drop((client, peer));
        host.release(credential.pin());
        assert!(matches!(resume.connect().await, Err(LinkError::Replay)));
        // Released, the organizer adds the rider's reply again and only that key gets in.
        host.admit(&reply).unwrap();
        let again = resume.connect().await.unwrap();
        let back = joined(&mut host).await;
        assert_eq!(back.client(), Some(credential.pin()));
        assert_eq!(host.shared.admissions().len(), MAX_ADMISSIONS);
        drop((again, back, resume, others));
        host.close().await;
    }

    async fn ports_are_released(addresses: Vec<SocketAddr>) {
        for address in addresses {
            timeout(Duration::from_secs(1), async {
                loop {
                    if StdSocket::bind(address).is_ok() {
                        break;
                    }
                    sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("owner drop releases its UDP socket");
        }
    }

    #[tokio::test]
    async fn dropping_the_host_closes_its_listener_and_releases_the_port() {
        let mut host = host().await;
        let joiner = joiner(&host).await;
        host.admit(joiner.reply()).unwrap();
        let addresses = host
            .local
            .paths
            .iter()
            .map(|p| p.endpoint.local_addr().unwrap())
            .collect();
        drop(host);
        assert!(joiner.connect().await.is_err());
        ports_are_released(addresses).await;
    }

    #[tokio::test]
    async fn cancelling_connect_drops_the_joiners_endpoint_and_owned_probe_tasks() {
        let host = host().await;
        let joiner = joiner(&host).await;
        let addresses = joiner
            .local
            .as_ref()
            .unwrap()
            .paths
            .iter()
            .map(|p| p.endpoint.local_addr().unwrap())
            .collect();
        let task = tokio::spawn(joiner.connect());
        sleep(Duration::from_millis(20)).await;
        task.abort();
        let _ = task.await;
        ports_are_released(addresses).await;
        host.close().await;
    }

    #[tokio::test]
    async fn the_original_deadline_covers_a_pinned_host_that_never_acknowledges_admission() {
        let mut host = host().await;
        host.tasks.abort_all();
        while host.tasks.join_next().await.is_some() {}
        let endpoint = ipv4(&host).endpoint.clone();
        let mut invite = host.invite().clone();
        invite.candidates.retain(|c| c.addr().is_ipv4());
        let task = tokio::spawn(async move {
            if let Some(incoming) = endpoint.accept().await {
                let connection = incoming.await.unwrap();
                let _ = connection.closed().await;
            }
        });
        let mut opts = options();
        opts.window = Duration::from_millis(100);
        let start = Instant::now();
        let joiner = Joiner::open(invite, route(), &opts).await.unwrap();
        assert!(matches!(joiner.connect().await, Err(LinkError::Timeout)));
        assert!(start.elapsed() < Duration::from_millis(500));
        host.close().await;
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn live_incoming_handshakes_obey_the_global_and_source_capacity_limits() {
        let mut host = host().await;
        let pending = joiner(&host).await;
        host.admit(pending.reply()).unwrap();
        let seed = joiner(&host).await;
        let endpoint = seed
            .local
            .as_ref()
            .unwrap()
            .endpoint(host.invite().candidates[0].addr())
            .unwrap();
        let connection = timeout(
            Duration::from_millis(350),
            endpoint
                .connect_with(
                    seed.client.clone(),
                    host.invite().candidates[0].addr(),
                    tls::SERVER_NAME,
                )
                .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        let mut held = vec![connection];
        let mut clients = vec![seed];
        let mut attempts = JoinSet::new();
        for _ in 0..32 {
            let client = joiner(&host).await;
            let endpoint = client
                .local
                .as_ref()
                .unwrap()
                .endpoint(host.invite().candidates[0].addr())
                .unwrap();
            let connecting = endpoint
                .connect_with(
                    client.client.clone(),
                    host.invite().candidates[0].addr(),
                    tls::SERVER_NAME,
                )
                .unwrap();
            attempts.spawn(async move { timeout(Duration::from_millis(350), connecting).await });
            clients.push(client);
        }
        while let Some(result) = attempts.join_next().await {
            if let Ok(Ok(Ok(connection))) = result {
                held.push(connection);
            }
        }
        assert!(held.len() <= crate::limits::MAX_PER_SOURCE);
        assert!(
            host.local
                .paths
                .iter()
                .map(|p| p.endpoint.open_connections())
                .sum::<usize>()
                <= MAX_CONNECTIONS
        );
        assert!(
            timeout(Duration::from_millis(20), host.next())
                .await
                .is_err()
        );
        drop((held, clients, pending));
        host.close().await;
    }

    #[tokio::test]
    async fn a_held_credential_lets_only_its_rider_back_once_until_released() {
        let mut host = host().await;
        let first = joiner(&host).await;
        host.admit(first.reply()).unwrap();
        let (client, resume) = first.connect_resumable().await.unwrap();
        let peer = joined(&mut host).await;
        let pin = peer.client().unwrap();
        // While the host still counts the connection, a second one only waits (#434).
        assert!(matches!(resume.connect().await, Err(LinkError::Timeout)));
        host.hold(
            pin,
            Instant::now() + Duration::from_secs(10),
            peer.peer_address(),
        );
        drop((client, peer));
        let again = resume.connect().await.unwrap();
        let back = joined(&mut host).await;
        assert_eq!(back.client(), Some(pin));
        again.send_datagram(&[9]).unwrap();
        assert_eq!(back.read_datagram().await.unwrap(), vec![9]);
        // Used again: another attempt waits, and a released seat turns it away for good.
        assert!(matches!(resume.connect().await, Err(LinkError::Timeout)));
        host.release(pin);
        // A host waiting for nobody accepts no handshake at all, so the rider cannot tell yet.
        assert!(matches!(resume.connect().await, Err(LinkError::Timeout)));
        let other = joiner(&host).await;
        host.admit(other.reply()).unwrap();
        assert!(matches!(resume.connect().await, Err(LinkError::Replay)));
        drop((again, back, resume, other));
        host.close().await;
    }

    #[tokio::test]
    async fn a_connection_closed_for_reconnecting_counts_as_lost_and_a_goodbye_does_not() {
        for reconnect in [true, false] {
            let mut host = host().await;
            let joiner = joiner(&host).await;
            host.admit(joiner.reply()).unwrap();
            let client = joiner.connect().await.unwrap();
            let peer = joined(&mut host).await;
            assert!(!client.lost() && !peer.lost());
            if reconnect {
                peer.close_for_reconnect();
            } else {
                peer.close();
            }
            timeout(Duration::from_secs(1), client.closed())
                .await
                .unwrap();
            assert_eq!(client.lost(), reconnect);
            assert_eq!(peer.lost(), reconnect);
            // Closing again later cannot change why it ended.
            client.close_for_reconnect();
            assert_eq!(client.lost(), reconnect);
            drop((client, peer));
            host.close().await;
        }
    }
}
