//! The lobby and the ride (ADR 0015): riders in a star around the host. Joiners tell the host
//! who they are on a reliable stream and learn from it who else is in, who leaves and when the
//! ride starts; everyone's state goes as datagrams through the host to everyone else.
//!
//! Each side reads the network in tasks of its own and hands what happened to the rider as
//! [`LobbyEvent`]s; neither a rider dropping out nor the host leaving stops anyone's ride
//! (R69).

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use quinn::{RecvStream, SendStream, VarInt};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tokio::time::{Instant, timeout};

use crate::code::Reply;
use crate::link::{Connection, Hint, Host, HostEvent, JoinError, Joiner};
use crate::wire::{Message, Refusal, Rider, RiderId, RiderState, parse_state, state_datagram};

/// Riders in one group ride, the host included.
pub const MAX_RIDERS: usize = 8;

/// How long the host waits for a connected joiner to say who they are.
const HELLO_WAIT: Duration = Duration::from_secs(10);

/// Connections close with these codes; the link's own start at 1.
const LEFT: u32 = 10;
const REFUSED: u32 = 11;

/// What happens in a group ride, as a rider learns it.
#[derive(Debug, PartialEq)]
pub enum LobbyEvent {
    /// A rider is in. A joiner learns this way, too, of everyone who was in before them.
    Joined {
        /// Their number in this ride.
        id: RiderId,
        /// Who they are.
        rider: Rider,
    },
    /// A rider left or dropped out.
    Left {
        /// Their number in this ride.
        id: RiderId,
    },
    /// Where a rider is now. An older state arriving late is not passed on.
    State {
        /// Their number in this ride.
        id: RiderId,
        /// Their state.
        state: RiderState,
    },
    /// At a joiner: the host started the countdown, and the ride starts at this moment.
    Start {
        /// When the ride starts here, with half the round trip to the host taken off.
        at: Instant,
    },
    /// At the host: a joiner whose reply it took did not get through within the window.
    GaveUp {
        /// What may have kept them apart.
        hint: Hint,
    },
    /// At a joiner: the host left or dropped out. The ride goes on alone.
    HostLeft,
}

/// Why a joiner did not get into the ride.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LobbyError {
    /// No connection to the host.
    #[error(transparent)]
    Join(#[from] JoinError),
    /// [`MAX_RIDERS`] are in already.
    #[error("The ride is full.")]
    Full,
    /// The ride has started; nobody joins after the start.
    #[error("The ride has started already.")]
    Started,
    /// The connection broke off while getting in.
    #[error("The connection to the host broke off: {0}")]
    Lost(String),
}

/// The host's side of a group ride: lets joiners in, starts the ride and passes everyone's
/// state on to everyone else.
pub struct HostLobby {
    commands: mpsc::UnboundedSender<Command>,
    events: mpsc::UnboundedReceiver<LobbyEvent>,
    hub: JoinHandle<()>,
}

enum Command {
    Admit(Reply),
    Start(Instant),
    State(RiderState),
    Close,
}

impl HostLobby {
    /// Opens the lobby with the host as its first rider; make the invite before.
    #[must_use]
    pub fn open(host: Host, me: Rider) -> Self {
        let (commands, commands_rx) = mpsc::unbounded_channel();
        let (events_tx, events) = mpsc::unbounded_channel();
        let hub = Hub {
            riders: BTreeMap::from([(RiderId::HOST, me)]),
            peers: HashMap::new(),
            started: false,
            latest: HashMap::new(),
            events: events_tx,
        };
        let hub = tokio::spawn(hub.run(host, commands_rx));
        Self {
            commands,
            events,
            hub,
        }
    }

    /// Takes a joiner's reply ([`Host::admit`]); after the start, no more are taken.
    pub fn admit(&self, reply: &Reply) {
        let _ = self.commands.send(Command::Admit(reply.clone()));
    }

    /// Starts the countdown: the ride starts `countdown` from now, for everyone in. Nobody
    /// joins after this.
    #[must_use = "the ride starts at this moment"]
    pub fn start(&self, countdown: Duration) -> Instant {
        let at = Instant::now() + countdown;
        let _ = self.commands.send(Command::Start(at));
        at
    }

    /// Sends the host's own state to everyone.
    pub fn send_state(&self, state: &RiderState) {
        let _ = self.commands.send(Command::State(*state));
    }

    /// What happened next; `None` once the lobby is closed.
    pub async fn next(&mut self) -> Option<LobbyEvent> {
        self.events.recv().await
    }

    /// Leaves the ride: the joiners ride on alone.
    pub async fn close(self) {
        let _ = self.commands.send(Command::Close);
        let _ = self.hub.await;
    }
}

/// The host's view of the ride, kept by one task.
struct Hub {
    riders: BTreeMap<RiderId, Rider>,
    peers: HashMap<RiderId, Peer>,
    started: bool,
    latest: HashMap<RiderId, Duration>,
    events: mpsc::UnboundedSender<LobbyEvent>,
}

/// A joiner who is in.
struct Peer {
    connection: Connection,
    outbox: mpsc::UnboundedSender<Message>,
}

enum Inbound {
    Hello {
        connection: Connection,
        send: SendStream,
        rider: Rider,
    },
    State(RiderId, RiderState),
    Gone(RiderId),
}

impl Hub {
    async fn run(mut self, mut host: Host, mut commands: mpsc::UnboundedReceiver<Command>) {
        let (inbound_tx, mut inbound) = mpsc::unbounded_channel();
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None | Some(Command::Close) => break,
                    Some(Command::Admit(reply)) => {
                        if !self.started {
                            host.admit(&reply);
                        }
                    }
                    Some(Command::Start(at)) => self.start(at),
                    Some(Command::State(state)) => self.forward(RiderId::HOST, &state),
                },
                event = host.next() => match event {
                    Some(HostEvent::Joined { connection, .. }) => {
                        tokio::spawn(hello(connection, inbound_tx.clone()));
                    }
                    Some(HostEvent::GaveUp { hint, .. }) => self.emit(LobbyEvent::GaveUp { hint }),
                    None => break,
                },
                Some(inbound) = inbound.recv() => match inbound {
                    Inbound::Hello { connection, send, rider } => {
                        self.welcome(connection, send, rider, &inbound_tx);
                    }
                    Inbound::State(id, state) => {
                        if self.is_newer(id, &state) {
                            self.forward(id, &state);
                            self.emit(LobbyEvent::State { id, state });
                        }
                    }
                    Inbound::Gone(id) => self.gone(id),
                },
            }
        }
        for peer in self.peers.values() {
            peer.connection.close(VarInt::from_u32(LEFT), b"host left");
        }
        host.close().await;
    }

    fn emit(&self, event: LobbyEvent) {
        let _ = self.events.send(event);
    }

    fn welcome(
        &mut self,
        connection: Connection,
        send: SendStream,
        rider: Rider,
        inbound: &mpsc::UnboundedSender<Inbound>,
    ) {
        let id = (1..)
            .map(RiderId)
            .take(MAX_RIDERS - 1)
            .find(|id| !self.riders.contains_key(id));
        let id = match (self.started, id) {
            (true, _) => return refuse(connection, send, Refusal::Started),
            (false, None) => return refuse(connection, send, Refusal::Full),
            (false, Some(id)) => id,
        };
        let (outbox, outgoing) = mpsc::unbounded_channel();
        tokio::spawn(write(send, outgoing));
        let riders = self
            .riders
            .iter()
            .map(|(id, rider)| (*id, rider.clone()))
            .collect();
        let _ = outbox.send(Message::Welcome { you: id, riders });
        for peer in self.peers.values() {
            let _ = peer.outbox.send(Message::Joined(id, rider.clone()));
        }
        tokio::spawn(listen(id, connection.clone(), inbound.clone()));
        self.riders.insert(id, rider.clone());
        self.peers.insert(id, Peer { connection, outbox });
        self.emit(LobbyEvent::Joined { id, rider });
    }

    fn gone(&mut self, id: RiderId) {
        if self.riders.remove(&id).is_none() {
            return;
        }
        self.peers.remove(&id);
        self.latest.remove(&id);
        for peer in self.peers.values() {
            let _ = peer.outbox.send(Message::Left(id));
        }
        self.emit(LobbyEvent::Left { id });
    }

    fn start(&mut self, at: Instant) {
        self.started = true;
        let after = at.saturating_duration_since(Instant::now());
        for peer in self.peers.values() {
            let _ = peer.outbox.send(Message::Countdown(after));
        }
    }

    /// Passes a rider's state on to every joiner but them; a datagram that does not fit is
    /// dropped, as a lost one would be.
    fn forward(&self, from: RiderId, state: &RiderState) {
        let datagram = state_datagram(from, state);
        for (id, peer) in &self.peers {
            if *id != from {
                let _ = peer.connection.send_datagram(datagram.clone().into());
            }
        }
    }

    fn is_newer(&mut self, id: RiderId, state: &RiderState) -> bool {
        newer(&mut self.latest, id, state)
    }
}

/// Whether `state` is later than the last one taken from `id`, and takes it if so: datagrams
/// may arrive out of order.
fn newer(latest: &mut HashMap<RiderId, Duration>, id: RiderId, state: &RiderState) -> bool {
    match latest.get(&id) {
        Some(last) if *last >= state.at => false,
        _ => {
            latest.insert(id, state.at);
            true
        }
    }
}

/// Waits for a connected joiner to say who they are.
async fn hello(connection: Connection, inbound: mpsc::UnboundedSender<Inbound>) {
    let said = timeout(HELLO_WAIT, async {
        let (send, mut recv) = connection.accept_bi().await.ok()?;
        match read(&mut recv).await? {
            Message::Hello(rider) => Some((send, rider)),
            _ => None,
        }
    })
    .await;
    match said {
        Ok(Some((send, rider))) => {
            let _ = inbound.send(Inbound::Hello {
                connection,
                send,
                rider,
            });
        }
        _ => connection.close(VarInt::from_u32(REFUSED), b"no hello"),
    }
}

/// Tells a joiner why they cannot ride, and lets them close the connection once they read it.
fn refuse(connection: Connection, mut send: SendStream, refusal: Refusal) {
    tokio::spawn(async move {
        let _ = send.write_all(&Message::Refused(refusal).frame()).await;
        let _ = send.finish();
        let _ = timeout(HELLO_WAIT, connection.closed()).await;
        connection.close(VarInt::from_u32(REFUSED), b"refused");
    });
}

/// Writes a joiner's lobby messages in order, apart from the hub so a slow joiner holds up no
/// one else.
async fn write(mut send: SendStream, mut outbox: mpsc::UnboundedReceiver<Message>) {
    while let Some(message) = outbox.recv().await {
        if send.write_all(&message.frame()).await.is_err() {
            break;
        }
    }
}

/// Takes a joiner's states until their connection ends.
async fn listen(id: RiderId, connection: Connection, inbound: mpsc::UnboundedSender<Inbound>) {
    while let Ok(datagram) = connection.read_datagram().await {
        // Whom a state is from is the host's to say, not the sender's.
        if let Some((_, state)) = parse_state(&datagram)
            && inbound.send(Inbound::State(id, state)).is_err()
        {
            return;
        }
    }
    let _ = inbound.send(Inbound::Gone(id));
}

/// The next lobby message on a stream; `None` once it ends or carries something else.
async fn read(recv: &mut RecvStream) -> Option<Message> {
    let mut prefix = [0; 2];
    recv.read_exact(&mut prefix).await.ok()?;
    let mut body = vec![0; Message::length(prefix)?];
    recv.read_exact(&mut body).await.ok()?;
    Message::parse(&body)
}

/// A joiner's side of a group ride.
pub struct JoinerLobby {
    joiner: Joiner,
    connection: Connection,
    me: RiderId,
    events: mpsc::UnboundedReceiver<LobbyEvent>,
    listening: [AbortHandle; 2],
}

impl JoinerLobby {
    /// Connects to the host ([`Joiner::connect`]) and gets into the ride as `me`.
    ///
    /// # Errors
    ///
    /// No connection to the host, or the host turned the joiner away ([`LobbyError`]).
    pub async fn join(joiner: Joiner, me: &Rider) -> Result<Self, LobbyError> {
        let connection = joiner.connect().await?;
        let lost = |e: &dyn std::fmt::Display| LobbyError::Lost(e.to_string());
        let (mut send, mut recv) = connection.open_bi().await.map_err(|e| lost(&e))?;
        send.write_all(&Message::Hello(me.clone()).frame())
            .await
            .map_err(|e| lost(&e))?;
        send.finish().map_err(|e| lost(&e))?;
        let answer = timeout(HELLO_WAIT, read(&mut recv)).await.ok().flatten();
        let (you, riders) = match answer {
            Some(Message::Welcome { you, riders }) => (you, riders),
            other => {
                connection.close(VarInt::from_u32(LEFT), b"");
                return Err(match other {
                    Some(Message::Refused(Refusal::Full)) => LobbyError::Full,
                    Some(Message::Refused(Refusal::Started)) => LobbyError::Started,
                    _ => LobbyError::Lost("the host did not let us in".into()),
                });
            }
        };
        let (events_tx, events) = mpsc::unbounded_channel();
        for (id, rider) in riders {
            let _ = events_tx.send(LobbyEvent::Joined { id, rider });
        }
        let messages = tokio::spawn(host_messages(connection.clone(), recv, events_tx.clone()));
        let states = tokio::spawn(host_states(connection.clone(), you, events_tx));
        Ok(Self {
            joiner,
            connection,
            me: you,
            events,
            listening: [messages.abort_handle(), states.abort_handle()],
        })
    }

    /// The joiner's number in this ride.
    #[must_use]
    pub fn me(&self) -> RiderId {
        self.me
    }

    /// Sends the joiner's state to the host, who passes it on.
    pub fn send_state(&self, state: &RiderState) {
        let _ = self
            .connection
            .send_datagram(state_datagram(self.me, state).into());
    }

    /// What happened next; `None` once the host is gone and everything from it is read.
    pub async fn next(&mut self) -> Option<LobbyEvent> {
        self.events.recv().await
    }

    /// Leaves the ride; the others ride on.
    pub async fn leave(self) {
        for task in &self.listening {
            task.abort();
        }
        self.connection.close(VarInt::from_u32(LEFT), b"left");
        self.joiner.close().await;
    }
}

async fn host_messages(
    connection: Connection,
    mut recv: RecvStream,
    events: mpsc::UnboundedSender<LobbyEvent>,
) {
    while let Some(message) = read(&mut recv).await {
        let event = match message {
            Message::Joined(id, rider) => LobbyEvent::Joined { id, rider },
            Message::Left(id) => LobbyEvent::Left { id },
            // The host's "in n ms" took half a round trip to get here.
            Message::Countdown(after) => LobbyEvent::Start {
                at: Instant::now() + after.saturating_sub(connection.rtt() / 2),
            },
            Message::Hello(_) | Message::Welcome { .. } | Message::Refused(_) => continue,
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

/// Takes everyone's states from the host until its connection ends, then says the host left.
async fn host_states(
    connection: Connection,
    me: RiderId,
    events: mpsc::UnboundedSender<LobbyEvent>,
) {
    let mut latest = HashMap::new();
    while let Ok(datagram) = connection.read_datagram().await {
        if let Some((id, state)) = parse_state(&datagram)
            && id != me
            && newer(&mut latest, id, &state)
            && events.send(LobbyEvent::State { id, state }).is_err()
        {
            return;
        }
    }
    let _ = events.send(LobbyEvent::HostLeft);
}

#[cfg(test)]
mod tests {
    use torqa_domain::units::{Kilograms, Meters};

    use super::*;
    use crate::candidates::GatherOptions;
    use crate::code::Course;
    use crate::link::ConnectOptions;

    const WAIT: Duration = Duration::from_secs(10);

    fn options() -> ConnectOptions {
        ConnectOptions {
            gather: GatherOptions {
                stun_servers: Vec::new(),
                router: false,
                timeout: Duration::from_secs(1),
                loopback: true,
            },
            window: WAIT,
        }
    }

    fn rider(name: &str) -> Rider {
        Rider {
            name: name.into(),
            mass: Kilograms(80.0),
        }
    }

    fn at(seconds: u64, distance: f64) -> RiderState {
        RiderState {
            at: Duration::from_secs(seconds),
            distance: Meters(distance),
            ..RiderState::default()
        }
    }

    /// A host with an invite to join it by.
    async fn host() -> (HostLobby, crate::code::Invite) {
        let host = Host::open(&options()).await.unwrap();
        let course = Course {
            name: "Col du Galibier".into(),
            route_hash: [7; 32],
        };
        let invite = host.invite("Anna", course);
        (HostLobby::open(host, rider("Anna")), invite)
    }

    async fn join(
        host: &HostLobby,
        invite: &crate::code::Invite,
        name: &str,
    ) -> Result<JoinerLobby, LobbyError> {
        let joiner = Joiner::open(invite.clone(), &options()).await.unwrap();
        host.admit(&joiner.reply());
        JoinerLobby::join(joiner, &rider(name)).await
    }

    async fn next(events: impl Future<Output = Option<LobbyEvent>>) -> LobbyEvent {
        timeout(WAIT, events)
            .await
            .expect("an event in time")
            .expect("the lobby is open")
    }

    /// The next event that is not a state: states keep coming while others join and leave.
    async fn next_news(lobby: &mut JoinerLobby) -> LobbyEvent {
        loop {
            match next(lobby.next()).await {
                LobbyEvent::State { .. } => {}
                event => return event,
            }
        }
    }

    #[tokio::test]
    async fn riders_see_who_is_in_and_who_leaves() {
        let (mut host, invite) = host().await;
        let mut ben = join(&host, &invite, "Ben").await.unwrap();
        assert_eq!(
            next(host.next()).await,
            LobbyEvent::Joined {
                id: ben.me(),
                rider: rider("Ben")
            }
        );
        assert_eq!(
            next_news(&mut ben).await,
            LobbyEvent::Joined {
                id: RiderId::HOST,
                rider: rider("Anna")
            }
        );

        let mut chloe = join(&host, &invite, "Chloé").await.unwrap();
        assert_ne!(chloe.me(), ben.me());
        next(host.next()).await;
        // Chloé hears of everyone before her, Ben of her.
        let mut before = vec![next_news(&mut chloe).await, next_news(&mut chloe).await];
        before.sort_by_key(|e| match e {
            LobbyEvent::Joined { id, .. } => *id,
            _ => RiderId(u8::MAX),
        });
        assert_eq!(
            before,
            [
                LobbyEvent::Joined {
                    id: RiderId::HOST,
                    rider: rider("Anna")
                },
                LobbyEvent::Joined {
                    id: ben.me(),
                    rider: rider("Ben")
                }
            ]
        );
        assert_eq!(
            next_news(&mut ben).await,
            LobbyEvent::Joined {
                id: chloe.me(),
                rider: rider("Chloé")
            }
        );

        let ben_id = ben.me();
        ben.leave().await;
        assert_eq!(next(host.next()).await, LobbyEvent::Left { id: ben_id });
        assert_eq!(next_news(&mut chloe).await, LobbyEvent::Left { id: ben_id });
        host.close().await;
        chloe.leave().await;
    }

    #[tokio::test]
    async fn everyone_gets_everyone_s_state_through_the_host() {
        let (mut host, invite) = host().await;
        let mut ben = join(&host, &invite, "Ben").await.unwrap();
        let mut chloe = join(&host, &invite, "Chloé").await.unwrap();
        for _ in 0..2 {
            next(host.next()).await;
        }

        ben.send_state(&at(1, 12.5));
        assert_eq!(
            next(host.next()).await,
            LobbyEvent::State {
                id: ben.me(),
                state: at(1, 12.5)
            }
        );
        let from_ben = loop {
            if let LobbyEvent::State { id, state } = next(chloe.next()).await {
                break (id, state);
            }
        };
        assert_eq!(from_ben, (ben.me(), at(1, 12.5)));

        host.send_state(&at(1, 20.0));
        for lobby in [&mut ben, &mut chloe] {
            let from_host = loop {
                if let LobbyEvent::State { id, state } = next(lobby.next()).await {
                    break (id, state);
                }
            };
            assert_eq!(from_host, (RiderId::HOST, at(1, 20.0)));
        }
        host.close().await;
    }

    #[test]
    fn a_state_arriving_after_a_later_one_is_dropped() {
        let mut latest = HashMap::new();
        let ben = RiderId(1);
        assert!(newer(&mut latest, ben, &at(2, 20.0)));
        assert!(!newer(&mut latest, ben, &at(1, 10.0)));
        assert!(!newer(&mut latest, ben, &at(2, 20.0)));
        assert!(newer(&mut latest, RiderId(2), &at(1, 10.0)));
        assert!(newer(&mut latest, ben, &at(3, 30.0)));
    }

    #[tokio::test]
    async fn the_countdown_starts_everyone_together() {
        let (host, invite) = host().await;
        let mut ben = join(&host, &invite, "Ben").await.unwrap();
        let mut chloe = join(&host, &invite, "Chloé").await.unwrap();
        let start = host.start(Duration::from_secs(3));
        for lobby in [&mut ben, &mut chloe] {
            let at = loop {
                match next_news(lobby).await {
                    LobbyEvent::Start { at } => break at,
                    LobbyEvent::Joined { .. } => {}
                    other => panic!("expected the countdown, got {other:?}"),
                }
            };
            let apart = at.max(start) - at.min(start);
            assert!(apart < Duration::from_millis(50), "{apart:?} apart");
        }
        host.close().await;
    }

    #[tokio::test]
    async fn nobody_joins_after_the_start() {
        let (host, invite) = host().await;
        let joiner = Joiner::open(invite.clone(), &options()).await.unwrap();
        host.admit(&joiner.reply());
        let _ = host.start(Duration::from_secs(3));
        // Taken before the start, connecting after it.
        let late = JoinerLobby::join(joiner, &rider("Ben")).await;
        assert_eq!(late.err(), Some(LobbyError::Started));
        host.close().await;
    }

    #[tokio::test]
    async fn a_ninth_rider_is_turned_away() {
        let (host, invite) = host().await;
        let mut riders = Vec::new();
        for i in 1..MAX_RIDERS {
            riders.push(join(&host, &invite, &format!("Rider {i}")).await.unwrap());
        }
        let ninth = join(&host, &invite, "Ninth").await;
        assert_eq!(ninth.err(), Some(LobbyError::Full));

        // A place that frees up before the start is taken again.
        riders.pop().unwrap().leave().await;
        let mut tries = 0;
        while join(&host, &invite, "Ninth").await.is_err() {
            tries += 1;
            assert!(tries < 10, "the freed place was not given again");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        host.close().await;
    }

    #[tokio::test]
    async fn the_ride_goes_on_when_the_host_leaves() {
        let (host, invite) = host().await;
        let mut ben = join(&host, &invite, "Ben").await.unwrap();
        host.close().await;
        assert_eq!(
            next_news(&mut ben).await,
            LobbyEvent::Joined {
                id: RiderId::HOST,
                rider: rider("Anna"),
            }
        );
        assert_eq!(next_news(&mut ben).await, LobbyEvent::HostLeft);
        // Sending into the void does not fail the rider's ride.
        ben.send_state(&at(1, 5.0));
        ben.leave().await;
    }
}
