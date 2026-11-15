//! Who dropped out of the group and who came back, told during a ride like its moments (R249).

use std::time::{Duration, Instant};

use orqwa_net::state::MAX_RIDERS;

use super::{Ending, Phase, Snapshot};

/// A change in who is connected to the group, for the ride screen to tell with the moments'
/// chime (R249).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionNotice {
    /// A friend's connection dropped; the organizer holds their seat while they reconnect.
    Away(String),
    /// A friend whose connection dropped is back, after this long away.
    Back(String, Duration),
    /// A friend whose connection dropped left the group: they did not come back in time, or
    /// the organizer removed them.
    Gone(String),
    /// This rider's connection to the group dropped; ORQWA reconnects by itself.
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
            Self::Away(_) => "away",
            Self::Back(..) => "back",
            Self::Gone(_) => "gone",
            Self::Lost => "lost",
            Self::Reconnected => "reconnected",
            Self::Ended(_) => "ended",
        }
    }
    /// The friend it is about, "" for this rider.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Away(name) | Self::Back(name, _) | Self::Gone(name) => name,
            Self::Lost | Self::Reconnected | Self::Ended(_) => "",
        }
    }
}

/// What the last snapshot showed of everyone's connection, to tell what changed since.
#[derive(Default)]
pub(crate) struct Seen {
    /// Each friend away: their name, and since when.
    away: [Option<(String, Instant)>; MAX_RIDERS],
    reconnecting: bool,
    connected: bool,
}

impl Seen {
    /// What changed since the last snapshot. Joining, leaving on purpose and the lobby's own
    /// comings and goings stay untold: only connections that dropped, and what became of them.
    pub(crate) fn notices(&mut self, snapshot: &Snapshot) -> Vec<ConnectionNotice> {
        let mut notices = Vec::new();
        for (id, rider) in snapshot.roster.iter().enumerate() {
            if id == usize::from(snapshot.me) {
                continue;
            }
            let was = self.away[id].take();
            match (&was, rider) {
                (Some((name, _)), None) => notices.push(ConnectionNotice::Gone(name.clone())),
                (Some((name, since)), Some(_)) if !snapshot.away[id] => {
                    notices.push(ConnectionNotice::Back(name.clone(), since.elapsed()));
                }
                (None, Some(rider)) if snapshot.away[id] => {
                    notices.push(ConnectionNotice::Away(rider.name.clone()));
                }
                _ => {}
            }
            if snapshot.away[id] {
                let since = was.map_or_else(Instant::now, |(_, since)| since);
                self.away[id] = rider.as_ref().map(|rider| (rider.name.clone(), since));
            }
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
        }
        self.reconnecting = reconnecting;
        self.connected = connected;
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

    fn riding() -> Snapshot {
        let mut snapshot = Snapshot {
            phase: Phase::Riding,
            ..Snapshot::default()
        };
        snapshot.roster[0] = Some(rider("Alex"));
        snapshot.roster[1] = Some(rider("Ben"));
        snapshot.roster[2] = Some(rider("Carla"));
        snapshot.me = 2;
        snapshot
    }

    #[test]
    fn a_friend_who_drops_is_told_once_and_again_when_back_or_gone() {
        let mut seen = Seen::default();
        let mut snapshot = riding();
        assert_eq!(seen.notices(&snapshot), NONE, "joining is not told");
        snapshot.away[1] = true;
        assert_eq!(
            seen.notices(&snapshot),
            [ConnectionNotice::Away("Ben".into())]
        );
        assert_eq!(seen.notices(&snapshot), NONE, "told once");
        assert!(seen.away(1).is_some(), "how long Ben is away");
        snapshot.away[1] = false;
        let back = seen.notices(&snapshot);
        assert!(matches!(&back[..], [ConnectionNotice::Back(name, _)] if name == "Ben"));
        assert_eq!(seen.away(1), None);
        snapshot.away[1] = true;
        seen.notices(&snapshot);
        snapshot.away[1] = false;
        snapshot.roster[1] = None;
        assert_eq!(
            seen.notices(&snapshot),
            [ConnectionNotice::Gone("Ben".into())]
        );
        // A friend who leaves on purpose was never away.
        snapshot.roster[0] = None;
        assert_eq!(seen.notices(&snapshot), NONE);
    }

    #[test]
    fn this_rider_hears_of_losing_the_group_coming_back_and_why_it_ended() {
        let mut seen = Seen::default();
        let mut snapshot = riding();
        seen.notices(&snapshot);
        snapshot.reconnecting = Some(std::time::Instant::now());
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
        let mut snapshot = riding();
        snapshot.reconnecting = Some(std::time::Instant::now());
        seen.notices(&snapshot);
        snapshot.reconnecting = None;
        snapshot.phase = Phase::Ended;
        assert_eq!(
            seen.notices(&snapshot),
            [ConnectionNotice::Ended(Ending::Lost)]
        );
    }
}
