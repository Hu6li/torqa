//! What riders send each other once connected (ADR 0015): lobby messages on a reliable stream,
//! each a big-endian `u16` length and the message, and each rider's state in a datagram.
//!
//! A lobby message is a kind byte and the kind's fields; names are a length byte and UTF-8, as
//! in the codes. A state datagram is a format byte, the rider, the time since the start in
//! milliseconds (`u32`) and distance, speed, power and cadence as `f32`: 22 bytes.

use std::time::Duration;

use torqa_domain::units::{Kilograms, Meters, MetersPerSecond, Rpm, Watts};

use crate::code::cut_name;

/// A rider in a group ride, as the host numbers them: the host is 0, joiners from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RiderId(pub u8);

impl RiderId {
    /// The host.
    pub const HOST: Self = Self(0);
}

/// Who rides, as the others are told once.
#[derive(Clone, Debug, PartialEq)]
pub struct Rider {
    /// The rider's name.
    pub name: String,
    /// Rider plus bike, for drawing them.
    pub mass: Kilograms,
}

/// Where a rider is and how they ride, sent 5 times a second ([`STATE_EVERY`]). Heart rate
/// stays private.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RiderState {
    /// Time since the common start: the later of two states replaces the earlier.
    pub at: Duration,
    /// Distance covered along the course.
    pub distance: Meters,
    /// Virtual speed.
    pub speed: MetersPerSecond,
    /// Power.
    pub power: Watts,
    /// Cadence.
    pub cadence: Rpm,
}

/// How often each rider sends their state.
pub const STATE_EVERY: Duration = Duration::from_millis(200);

/// A message longer than this is not one of ours.
const MAX_MESSAGE: usize = 1024;

const STATE_FORMAT: u8 = 1;
const STATE_LEN: usize = 22;

const HELLO: u8 = 1;
const WELCOME: u8 = 2;
const REFUSED: u8 = 3;
const JOINED: u8 = 4;
const LEFT: u8 = 5;
const COUNTDOWN: u8 = 6;

/// Why the host turned a joiner away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Full,
    Started,
}

/// What goes over the lobby's stream.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Message {
    /// A joiner's first message.
    Hello(Rider),
    /// The host's answer: the joiner's number and who is in already.
    Welcome {
        you: RiderId,
        riders: Vec<(RiderId, Rider)>,
    },
    Refused(Refusal),
    Joined(RiderId, Rider),
    Left(RiderId),
    /// The ride starts this long after the message was sent.
    Countdown(Duration),
}

impl Message {
    /// The message with its length in front, to write on the stream.
    pub(crate) fn frame(&self) -> Vec<u8> {
        let mut out = vec![0, 0];
        match self {
            Self::Hello(rider) => {
                out.push(HELLO);
                put_rider(&mut out, rider);
            }
            Self::Welcome { you, riders } => {
                out.extend([WELCOME, you.0]);
                #[allow(clippy::cast_possible_truncation)] // at most MAX_RIDERS
                out.push(riders.len() as u8);
                for (id, rider) in riders {
                    out.push(id.0);
                    put_rider(&mut out, rider);
                }
            }
            Self::Refused(refusal) => out.extend([
                REFUSED,
                match refusal {
                    Refusal::Full => 1,
                    Refusal::Started => 2,
                },
            ]),
            Self::Joined(id, rider) => {
                out.extend([JOINED, id.0]);
                put_rider(&mut out, rider);
            }
            Self::Left(id) => out.extend([LEFT, id.0]),
            Self::Countdown(after) => {
                out.push(COUNTDOWN);
                out.extend(millis(*after).to_be_bytes());
            }
        }
        let len = u16::try_from(out.len() - 2).expect("messages are short");
        out[..2].copy_from_slice(&len.to_be_bytes());
        out
    }

    /// How long the message after a length prefix is, if it may be one of ours.
    pub(crate) fn length(prefix: [u8; 2]) -> Option<usize> {
        let len = usize::from(u16::from_be_bytes(prefix));
        (1..=MAX_MESSAGE).contains(&len).then_some(len)
    }

    /// Reads a message without its length prefix.
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let message = match r.byte()? {
            HELLO => Self::Hello(r.rider()?),
            WELCOME => {
                let you = RiderId(r.byte()?);
                let count = r.byte()?;
                let riders = (0..count)
                    .map(|_| Some((RiderId(r.byte()?), r.rider()?)))
                    .collect::<Option<_>>()?;
                Self::Welcome { you, riders }
            }
            REFUSED => Self::Refused(match r.byte()? {
                1 => Refusal::Full,
                2 => Refusal::Started,
                _ => return None,
            }),
            JOINED => Self::Joined(RiderId(r.byte()?), r.rider()?),
            LEFT => Self::Left(RiderId(r.byte()?)),
            COUNTDOWN => Self::Countdown(Duration::from_millis(u64::from(r.u32()?))),
            _ => return None,
        };
        r.0.is_empty().then_some(message)
    }
}

/// A rider's state as a datagram.
pub(crate) fn state_datagram(id: RiderId, state: &RiderState) -> Vec<u8> {
    let mut out = Vec::with_capacity(STATE_LEN);
    out.extend([STATE_FORMAT, id.0]);
    out.extend(millis(state.at).to_be_bytes());
    #[allow(clippy::cast_possible_truncation)] // f32 keeps centimetres over 200 km
    for value in [
        state.distance.0,
        state.speed.0,
        state.power.0,
        state.cadence.0,
    ] {
        out.extend((value as f32).to_be_bytes());
    }
    out
}

/// Reads a state datagram; `None` for anything else, e.g. one of a newer format.
pub(crate) fn parse_state(bytes: &[u8]) -> Option<(RiderId, RiderState)> {
    if bytes.len() != STATE_LEN || bytes[0] != STATE_FORMAT {
        return None;
    }
    let mut r = Reader(&bytes[1..]);
    let id = RiderId(r.byte()?);
    let at = Duration::from_millis(u64::from(r.u32()?));
    let mut value = || Some(f64::from(f32::from_be_bytes(r.array()?)));
    let state = RiderState {
        at,
        distance: Meters(value()?),
        speed: MetersPerSecond(value()?),
        power: Watts(value()?),
        cadence: Rpm(value()?),
    };
    Some((id, state))
}

fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

fn put_rider(out: &mut Vec<u8>, rider: &Rider) {
    let name = cut_name(&rider.name);
    #[allow(clippy::cast_possible_truncation)] // cut to MAX_NAME_BYTES
    out.push(name.len() as u8);
    out.extend(name.as_bytes());
    #[allow(clippy::cast_possible_truncation)] // a rider's mass fits f32 well
    out.extend((rider.mass.0 as f32).to_be_bytes());
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.array()?))
    }

    fn rider(&mut self) -> Option<Rider> {
        let len = usize::from(self.byte()?);
        let name = String::from_utf8(self.take(len)?.to_vec()).ok()?;
        let mass = Kilograms(f64::from(f32::from_be_bytes(self.array()?)));
        Some(Rider { name, mass })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rider(name: &str) -> Rider {
        Rider {
            name: name.into(),
            mass: Kilograms(82.5),
        }
    }

    fn round_trip(message: &Message) -> Option<Message> {
        let frame = message.frame();
        let len = Message::length([frame[0], frame[1]])?;
        assert_eq!(len, frame.len() - 2);
        Message::parse(&frame[2..])
    }

    #[test]
    fn lobby_messages_arrive_as_they_were_sent() {
        let messages = [
            Message::Hello(rider("Anna")),
            Message::Welcome {
                you: RiderId(2),
                riders: vec![(RiderId::HOST, rider("Ben")), (RiderId(1), rider("Chloé"))],
            },
            Message::Refused(Refusal::Full),
            Message::Refused(Refusal::Started),
            Message::Joined(RiderId(3), rider("Dmitri")),
            Message::Left(RiderId(1)),
            Message::Countdown(Duration::from_millis(9_750)),
        ];
        for message in &messages {
            assert_eq!(round_trip(message).as_ref(), Some(message));
        }
    }

    #[test]
    fn long_names_are_cut_between_characters() {
        let long = "é".repeat(100);
        let Some(Message::Hello(sent)) = round_trip(&Message::Hello(rider(&long))) else {
            panic!("a hello");
        };
        assert!(sent.name.len() <= crate::code::MAX_NAME_BYTES);
        assert!(long.starts_with(&sent.name));
    }

    #[test]
    fn refuses_messages_cut_short_or_with_more_than_they_should_have() {
        let frame = Message::Joined(RiderId(1), rider("Anna")).frame();
        let body = &frame[2..];
        assert_eq!(Message::parse(&body[..body.len() - 1]), None);
        assert_eq!(Message::parse(&[body, &[0]].concat()), None);
        assert_eq!(Message::parse(&[99]), None);
        assert_eq!(Message::length([0, 0]), None);
        assert_eq!(Message::length([0xff, 0xff]), None);
    }

    #[test]
    fn a_state_fits_a_small_datagram_and_keeps_its_precision() {
        let state = RiderState {
            at: Duration::from_millis(3_723_400),
            distance: Meters(187_654.32),
            speed: MetersPerSecond(9.87),
            power: Watts(245.0),
            cadence: Rpm(88.0),
        };
        let datagram = state_datagram(RiderId(4), &state);
        assert_eq!(datagram.len(), STATE_LEN);
        let (id, got) = parse_state(&datagram).unwrap();
        assert_eq!(id, RiderId(4));
        assert_eq!(got.at, state.at);
        assert!((got.distance.0 - state.distance.0).abs() < 0.02);
        assert!((got.speed.0 - state.speed.0).abs() < 1e-5);
        assert!((got.power.0 - state.power.0).abs() < 1e-5);
        assert!((got.cadence.0 - state.cadence.0).abs() < 1e-5);
    }

    #[test]
    fn ignores_datagrams_of_another_format() {
        let mut datagram = state_datagram(RiderId(1), &RiderState::default());
        datagram[0] = STATE_FORMAT + 1;
        assert_eq!(parse_state(&datagram), None);
        assert_eq!(parse_state(&[STATE_FORMAT, 1]), None);
        // A link-level ping, as net-ping sends.
        assert_eq!(parse_state(&[0]), None);
    }
}
