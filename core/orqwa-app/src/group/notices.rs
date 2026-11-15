//! Who dropped out of the group and who came back, told during a ride like its moments (R249).
//! Friends who drop or come back together are told together, and a drop shorter than
//! [`TOGETHER`] not at all.

use std::time::{Duration, Instant};

use orqwa_net::state::MAX_RIDERS;

use super::{Ending, Phase, Snapshot};

/// How long friends' drops, returns and departures are gathered before they are told in one:
/// several friends usually drop at once, and a shorter drop is not worth a card.
const TOGETHER: Duration = Duration::from_secs(2);

/// A change in who is connected to the group, for the ride screen to tell with the moments'
/// chime (R249).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionNotice {
    /// Friends whose connection dropped; the organizer holds their seats while they
    /// reconnect. `everyone`: at the organizer, nobody else is connected any more, so its own
    /// network probably dropped.
    Away {
        /// Who dropped, in seat order.
        names: Vec<String>,
        /// Nobody else is connected to the organizer.
        everyone: bool,
    },
    /// Friends whose connection dropped are back, after at most this long away.
    Back(Vec<String>, Duration),
    /// Friends whose connection dropped left the group: they did not come back in time, or
    /// the organizer removed them.
    Gone(Vec<String>),
    /// This rider's connection to the group dropped; it reconnects by itself.
    Lost,
    /// This rider is back with the group.
    Reconnected,
    /// This rider's group ended, and why; the ride goes on alone.
    Ended(Ending),
}

impl ConnectionNotice {
    /// Stable presentation key.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Away { .. } => "away",
            Self::Back(..) => "back",
            Self::Gone(_) => "gone",
            Self::Lost => "lost",
            Self::Reconnected => "reconnected",
            Self::Ended(_) => "ended",
        }
    }
    /// The friends it is about, none for this rider.
    #[must_use]
    pub fn names(&self) -> &[String] {
        match self {
            Self::Away { names, .. } | Self::Back(names, _) | Self::Gone(names) => names,
            Self::Lost | Self::Reconnected | Self::Ended(_) => &[],
        }
    }
}

/// Friends' changes gathered for [`TOGETHER`], by seat.
#[derive(Default)]
struct Gathered {
    since: Option<Instant>,
    away: Vec<(usize, String)>,
    back: Vec<(usize, String, Duration)>,
    gone: Vec<(usize, String)>,
}

/// What the last snapshot showed of everyone's connection, to tell what changed since.
#[derive(Default)]
pub(crate) struct Seen {
    /// Each friend away: their name, and since when.
    away: [Option<(String, Instant)>; MAX_RIDERS],
    gathered: Gathered,
    reconnecting: bool,
    connected: bool,
}

impl Seen {
    /// What changed since the last snapshot. Joining, leaving on purpose and the lobby's own
    /// comings and goings stay untold: only connections that dropped, and what became of them.
    pub(crate) fn notices(&mut self, snapshot: &Snapshot) -> Vec<ConnectionNotice> {
        self.notices_at(snapshot, Instant::now())
    }

    fn notices_at(&mut self, snapshot: &Snapshot, now: Instant) -> Vec<ConnectionNotice> {
        let mut notices = Vec::new();
        for (id, rider) in snapshot.roster.iter().enumerate() {
            if id == usize::from(snapshot.me) {
                continue;
            }
            let was = self.away[id].take();
            match (&was, rider) {
                (Some((name, _)), None) => self.gather(now).gone.push((id, name.clone())),
                (Some((name, since)), Some(_)) if !snapshot.away[id] => {
                    let gathered = self.gather(now);
                    // Back before anyone heard of the drop: nothing to tell.
                    let before = gathered.away.len();
                    gathered.away.retain(|(seat, _)| *seat != id);
                    if gathered.away.len() == before {
                        let away = now.saturating_duration_since(*since);
                        gathered.back.push((id, name.clone(), away));
                    }
                }
                (None, Some(rider)) if snapshot.away[id] => {
                    self.gather(now).away.push((id, rider.name.clone()));
                }
                _ => {}
            }
            if snapshot.away[id] {
                let since = was.map_or(now, |(_, since)| since);
                self.away[id] = rider.as_ref().map(|rider| (rider.name.clone(), since));
            }
        }
        if self
            .gathered
            .since
            .is_some_and(|since| now.saturating_duration_since(since) >= TOGETHER)
        {
            notices.extend(self.tell(snapshot));
        }
        let reconnecting = snapshot.reconnecting.is_some();
        let connected = !matches!(
            snapshot.phase,
            Phase::Idle | Phase::Connecting | Phase::Ended | Phase::Failed
        );
        if reconnecting && !self.reconnecting {
            notices.push(ConnectionNotice::Lost);
        } else if !reconnecting && self.reconnecting && connected {
            notices.push(ConnectionNotice::Reconnected);
        }
        if self.connected && snapshot.phase == Phase::Ended && !snapshot.host {
            notices.push(ConnectionNotice::Ended(snapshot.ending));
        }
        if snapshot.phase == Phase::Ended {
            self.away = Default::default();
            self.gathered = Gathered::default();
        }
        self.reconnecting = reconnecting;
        self.connected = connected;
        notices
    }

    fn gather(&mut self, now: Instant) -> &mut Gathered {
        self.gathered.since.get_or_insert(now);
        &mut self.gathered
    }

    /// The friends' changes gathered, each kind in one notice.
    fn tell(&mut self, snapshot: &Snapshot) -> Vec<ConnectionNotice> {
        let gathered = std::mem::take(&mut self.gathered);
        let names = |mut seats: Vec<(usize, String)>| {
            seats.sort_by_key(|(seat, _)| *seat);
            seats.into_iter().map(|(_, name)| name).collect::<Vec<_>>()
        };
        let mut notices = Vec::new();
        if !gathered.away.is_empty() {
            let others = snapshot
                .roster
                .iter()
                .enumerate()
                .filter(|(id, rider)| rider.is_some() && *id != usize::from(snapshot.me));
            let everyone = snapshot.host
                && others.clone().count() > 1
                && others.clone().all(|(id, _)| snapshot.away[id]);
            notices.push(ConnectionNotice::Away {
                names: names(gathered.away),
                everyone,
            });
        }
        if !gathered.back.is_empty() {
            let longest = gathered.back.iter().map(|(_, _, away)| *away).max();
            let back = gathered
                .back
                .into_iter()
                .map(|(seat, name, _)| (seat, name));
            notices.push(ConnectionNotice::Back(
                names(back.collect()),
                longest.unwrap_or_default(),
            ));
        }
        if !gathered.gone.is_empty() {
            notices.push(ConnectionNotice::Gone(names(gathered.gone)));
        }
        notices
    }
    /// How long the friend in seat `id` has been away, `None` while connected.
    pub(crate) fn away(&self, id: usize) -> Option<Duration> {
        self.away
            .get(id)?
            .as_ref()
            .map(|(_, since)| since.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::Rider;
    use orqwa_domain::profile::Avatar;
    use orqwa_domain::units::Kilograms;

    const NONE: [ConnectionNotice; 0] = [];

    fn rider(name: &str) -> Rider {
        Rider {
            name: name.into(),
            mass: Kilograms(83.0),
            weight: Kilograms(75.0),
            avatar: Avatar::default(),
        }
    }

    fn riding(host: bool) -> Snapshot {
        let mut snapshot = Snapshot {
            phase: Phase::Riding,
            host,
            ..Snapshot::default()
        };
        for (seat, name) in ["Alex", "Ben", "Carla", "Dana"].into_iter().enumerate() {
            snapshot.roster[seat] = Some(rider(name));
        }
        snapshot.me = if host { 0 } else { 3 };
        snapshot
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn friends_who_drop_together_are_told_once_together_and_short_drops_not_at_all() {
        let start = Instant::now();
        let at = |seconds: f64| start + Duration::from_secs_f64(seconds);
        let mut seen = Seen::default();
        let mut snapshot = riding(false);
        assert_eq!(
            seen.notices_at(&snapshot, at(0.0)),
            NONE,
            "joining is not told"
        );
        // A drop shorter than the gathering is not worth a card.
        snapshot.away[1] = true;
        assert_eq!(seen.notices_at(&snapshot, at(1.0)), NONE);
        snapshot.away[1] = false;
        assert_eq!(seen.notices_at(&snapshot, at(1.5)), NONE);
        assert_eq!(seen.notices_at(&snapshot, at(4.0)), NONE);
        // Carla, then Ben a moment later: one card, in seat order.
        snapshot.away[2] = true;
        assert_eq!(seen.notices_at(&snapshot, at(5.0)), NONE);
        snapshot.away[1] = true;
        assert_eq!(seen.notices_at(&snapshot, at(5.5)), NONE);
        assert!(seen.away(1).is_some(), "how long Ben is away");
        assert_eq!(
            seen.notices_at(&snapshot, at(7.0)),
            [ConnectionNotice::Away {
                names: names(&["Ben", "Carla"]),
                everyone: false
            }]
        );
        assert_eq!(seen.notices_at(&snapshot, at(8.0)), NONE, "told once");
        // Both back: one card, with the longer time away.
        snapshot.away = [false; MAX_RIDERS];
        seen.notices_at(&snapshot, at(15.0));
        assert_eq!(
            seen.notices_at(&snapshot, at(17.0)),
            [ConnectionNotice::Back(
                names(&["Ben", "Carla"]),
                Duration::from_secs(10)
            )]
        );
        assert_eq!(seen.away(1), None);
        // Gone after dropping; a friend who leaves on purpose was never away.
        snapshot.away[1] = true;
        seen.notices_at(&snapshot, at(20.0));
        seen.notices_at(&snapshot, at(22.0));
        snapshot.away[1] = false;
        snapshot.roster[1] = None;
        snapshot.roster[0] = None;
        seen.notices_at(&snapshot, at(30.0));
        assert_eq!(
            seen.notices_at(&snapshot, at(32.0)),
            [ConnectionNotice::Gone(names(&["Ben"]))]
        );
    }

    #[test]
    fn the_organizer_whom_everyone_dropped_hears_its_own_network_probably_did() {
        let start = Instant::now();
        let mut seen = Seen::default();
        let mut snapshot = riding(true);
        seen.notices_at(&snapshot, start);
        snapshot.away[1..4].fill(true);
        seen.notices_at(&snapshot, start);
        assert_eq!(
            seen.notices_at(&snapshot, start + TOGETHER),
            [ConnectionNotice::Away {
                names: names(&["Ben", "Carla", "Dana"]),
                everyone: true
            }]
        );
    }

    #[test]
    fn this_rider_hears_of_losing_the_group_coming_back_and_why_it_ended() {
        let mut seen = Seen::default();
        let mut snapshot = riding(false);
        seen.notices(&snapshot);
        snapshot.reconnecting = Some(Instant::now());
        assert_eq!(seen.notices(&snapshot), [ConnectionNotice::Lost]);
        snapshot.reconnecting = None;
        assert_eq!(seen.notices(&snapshot), [ConnectionNotice::Reconnected]);
        snapshot.phase = Phase::Ended;
        snapshot.ending = Ending::Removed;
        assert_eq!(
            seen.notices(&snapshot),
            [ConnectionNotice::Ended(Ending::Removed)]
        );
        assert_eq!(seen.notices(&snapshot), NONE, "told once");
        // Giving up reconnecting ends the group: told as the ending, not as being back.
        let mut seen = Seen::default();
        let mut snapshot = riding(false);
        snapshot.reconnecting = Some(Instant::now());
        seen.notices(&snapshot);
        snapshot.reconnecting = None;
        snapshot.phase = Phase::Ended;
        assert_eq!(
            seen.notices(&snapshot),
            [ConnectionNotice::Ended(Ending::Lost)]
        );
    }
}
