//! Small, length-prefixed lobby controls. No general-purpose peer-controlled deserializer.

use std::time::Duration;

use quinn::{RecvStream, SendStream};
use tokio::time::timeout;

use orqwa_domain::profile::Avatar;

use crate::code::RouteHash;
use crate::lobby::{LobbyError, Rider, Roster};
use crate::state::{MAX_RIDERS, RiderId};

const MAX_FRAME: usize = 1024;

// Controls are decoded and handled one at a time: a welcome's full roster, about 400 bytes, on
// the stack costs less than an allocation for every control.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub(crate) enum Message {
    Hello(RouteHash, Rider),
    Welcome(RiderId, RiderId, crate::code::RideId, u64, Roster),
    Joined(RiderId, Rider),
    Left(RiderId),
    Start(u32),
    VoiceEnabled(bool),
    VoiceRoster([u64; MAX_RIDERS]),
    Selection(crate::activity::Selection),
    Ready(u64, bool),
    Readiness(RiderId, u64, bool),
    Running(i64),
    Workout(crate::lobby::timing::WorkoutSample),
    Handover(crate::handover::Message),
    Enter(u64),
    Entry(u64, Option<crate::lobby::RideProgress>),
}

pub(crate) async fn write(send: &mut SendStream, message: &Message) -> Result<(), LobbyError> {
    let bytes = encode(message)?;
    send.write_all(
        &u16::try_from(bytes.len())
            .map_err(|_| LobbyError::Protocol)?
            .to_be_bytes(),
    )
    .await
    .map_err(|_| LobbyError::Lost)?;
    send.write_all(&bytes).await.map_err(|_| LobbyError::Lost)
}

// Waiting for a whole control is allowed while in the lobby. Once the first byte
// arrives, a partial frame must complete in two seconds, including its length header.
pub(crate) async fn read(recv: &mut RecvStream) -> Result<Message, LobbyError> {
    let mut first = [0];
    recv.read_exact(&mut first)
        .await
        .map_err(|_| LobbyError::Lost)?;
    timeout(Duration::from_secs(2), async {
        let mut second = [0];
        recv.read_exact(&mut second)
            .await
            .map_err(|_| LobbyError::Lost)?;
        let len = usize::from(u16::from_be_bytes([first[0], second[0]]));
        if len == 0 || len > MAX_FRAME {
            return Err(LobbyError::Protocol);
        }
        let mut bytes = [0; MAX_FRAME];
        recv.read_exact(&mut bytes[..len])
            .await
            .map_err(|_| LobbyError::Lost)?;
        decode(&bytes[..len])
    })
    .await
    .map_err(|_| LobbyError::Lost)?
}

fn rider(bytes: &mut Vec<u8>, rider: &Rider) -> Result<(), LobbyError> {
    rider.validate()?;
    bytes.push(u8::try_from(rider.name.len()).map_err(|_| LobbyError::Protocol)?);
    bytes.extend_from_slice(rider.name.as_bytes());
    bytes.extend_from_slice(&rider.mass.0.to_be_bytes());
    bytes.extend_from_slice(&rider.weight.0.to_be_bytes());
    bytes.push(match rider.avatar {
        Avatar::Female => 0,
        Avatar::Male => 1,
    });
    Ok(())
}

pub(crate) fn encode(message: &Message) -> Result<Vec<u8>, LobbyError> {
    let mut bytes = vec![crate::code::VERSION];
    match message {
        Message::Hello(route, who) => {
            bytes.push(1);
            bytes.extend_from_slice(&route.0);
            rider(&mut bytes, who)?;
        }
        Message::Welcome(me, organizer, root, epoch, roster) => {
            bytes.extend_from_slice(&[2, me.value(), organizer.value()]);
            bytes.extend_from_slice(&root.0);
            bytes.extend_from_slice(&epoch.to_be_bytes());
            bytes.push(
                u8::try_from(roster.iter().flatten().count()).map_err(|_| LobbyError::Protocol)?,
            );
            for (id, who) in roster
                .iter()
                .enumerate()
                .filter_map(|(id, who)| who.as_ref().map(|who| (id, who)))
            {
                bytes.push(u8::try_from(id).map_err(|_| LobbyError::Protocol)?);
                rider(&mut bytes, who)?;
            }
        }
        Message::Joined(id, who) => {
            bytes.extend_from_slice(&[3, id.value()]);
            rider(&mut bytes, who)?;
        }
        Message::Left(id) => bytes.extend_from_slice(&[4, id.value()]),
        Message::Start(millis) => {
            bytes.push(5);
            bytes.extend_from_slice(&millis.to_be_bytes());
        }
        Message::VoiceEnabled(enabled) => bytes.extend_from_slice(&[6, u8::from(*enabled)]),
        Message::VoiceRoster(epochs) => {
            bytes.push(7);
            for epoch in epochs {
                bytes.extend_from_slice(&epoch.to_be_bytes());
            }
        }
        Message::Selection(selection) => {
            bytes.push(8);
            bytes.extend_from_slice(&selection.encode().map_err(|_| LobbyError::Protocol)?);
        }
        Message::Ready(revision, ready) => {
            bytes.extend_from_slice(&[9, u8::from(*ready)]);
            bytes.extend_from_slice(&revision.to_be_bytes());
        }
        Message::Readiness(id, revision, ready) => {
            bytes.extend_from_slice(&[10, id.value(), u8::from(*ready)]);
            bytes.extend_from_slice(&revision.to_be_bytes());
        }
        Message::Handover(message) => {
            bytes.push(15);
            bytes.extend_from_slice(&message.encode()?);
        }
        Message::Workout(sample) => {
            sample.validate()?;
            bytes.extend_from_slice(&[14, u8::from(sample.paused)]);
            for value in [
                sample.revision,
                sample.sequence,
                u64::try_from(sample.group.as_millis()).map_err(|_| LobbyError::Protocol)?,
                u64::try_from(sample.elapsed.as_millis()).map_err(|_| LobbyError::Protocol)?,
            ] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
        }
        Message::Running(age) => {
            crate::lobby::entry::clock(*age)?;
            bytes.push(11);
            bytes.extend_from_slice(&age.to_be_bytes());
        }
        Message::Enter(revision) => {
            bytes.push(12);
            bytes.extend_from_slice(&revision.to_be_bytes());
        }
        Message::Entry(revision, progress) => {
            bytes.extend_from_slice(&[13, u8::from(progress.is_some())]);
            bytes.extend_from_slice(&revision.to_be_bytes());
            if let Some(progress) = progress {
                progress.validate()?;
                bytes.extend_from_slice(&progress.distance.0.to_be_bytes());
                let millis = progress.workout.map_or(u64::MAX, |time| {
                    u64::try_from(time.as_millis()).unwrap_or(u64::MAX)
                });
                bytes.extend_from_slice(&millis.to_be_bytes());
            }
        }
    }
    if bytes.len() > MAX_FRAME {
        return Err(LobbyError::Protocol);
    }
    Ok(bytes)
}

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], LobbyError> {
        let bytes = self.0.get(..N).ok_or(LobbyError::Protocol)?;
        let result = bytes.try_into().map_err(|_| LobbyError::Protocol)?;
        self.0 = &self.0[N..];
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8, LobbyError> {
        Ok(self.take::<1>()?[0])
    }
    fn id(&mut self) -> Result<RiderId, LobbyError> {
        Ok(RiderId::new(self.byte()?)?)
    }
    fn rider(&mut self) -> Result<Rider, LobbyError> {
        let len = usize::from(self.byte()?);
        if len == 0 || len > crate::code::MAX_NAME_BYTES {
            return Err(LobbyError::Protocol);
        }
        let name = std::str::from_utf8(self.0.get(..len).ok_or(LobbyError::Protocol)?)
            .map_err(|_| LobbyError::Protocol)?;
        self.0 = &self.0[len..];
        let rider = Rider {
            name: name.into(),
            mass: orqwa_domain::units::Kilograms(f64::from_be_bytes(self.take()?)),
            weight: orqwa_domain::units::Kilograms(f64::from_be_bytes(self.take()?)),
            avatar: match self.byte()? {
                0 => Avatar::Female,
                1 => Avatar::Male,
                _ => return Err(LobbyError::Protocol),
            },
        };
        rider.validate()?;
        Ok(rider)
    }

    fn entry(&mut self) -> Result<Message, LobbyError> {
        let present = self.byte()?;
        let revision = u64::from_be_bytes(self.take()?);
        let progress = match present {
            0 => None,
            1 => {
                let distance = orqwa_domain::units::Meters(f64::from_be_bytes(self.take()?));
                let millis = u64::from_be_bytes(self.take()?);
                let progress = crate::lobby::RideProgress {
                    distance,
                    workout: (millis != u64::MAX).then(|| Duration::from_millis(millis)),
                };
                progress.validate()?;
                Some(progress)
            }
            _ => return Err(LobbyError::Protocol),
        };
        Ok(Message::Entry(revision, progress))
    }

    fn workout(&mut self) -> Result<Message, LobbyError> {
        let paused = match self.byte()? {
            0 => false,
            1 => true,
            _ => return Err(LobbyError::Protocol),
        };
        let sample = crate::lobby::timing::WorkoutSample {
            revision: u64::from_be_bytes(self.take()?),
            sequence: u64::from_be_bytes(self.take()?),
            group: Duration::from_millis(u64::from_be_bytes(self.take()?)),
            elapsed: Duration::from_millis(u64::from_be_bytes(self.take()?)),
            paused,
        };
        sample.validate()?;
        Ok(Message::Workout(sample))
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Message, LobbyError> {
    if bytes.len() > MAX_FRAME {
        return Err(LobbyError::Protocol);
    }
    let mut reader = Reader(bytes);
    if reader.byte()? != crate::code::VERSION {
        return Err(LobbyError::Protocol);
    }
    let message = match reader.byte()? {
        1 => Message::Hello(RouteHash(reader.take()?), reader.rider()?),
        2 => {
            let me = reader.id()?;
            let organizer = reader.id()?;
            let root = crate::code::RideId(reader.take()?);
            let epoch = u64::from_be_bytes(reader.take()?);
            if root.0 == [0; 16] || epoch == 0 {
                return Err(LobbyError::Protocol);
            }
            let count = usize::from(reader.byte()?);
            if !(2..=MAX_RIDERS).contains(&count) || me == organizer {
                return Err(LobbyError::Protocol);
            }
            let mut roster: Roster = std::array::from_fn(|_| None);
            for _ in 0..count {
                let id = usize::from(reader.id()?.value());
                if roster[id].is_some() {
                    return Err(LobbyError::Protocol);
                }
                roster[id] = Some(reader.rider()?);
            }
            if roster[usize::from(organizer.value())].is_none()
                || roster[usize::from(me.value())].is_none()
            {
                return Err(LobbyError::Protocol);
            }
            Message::Welcome(me, organizer, root, epoch, roster)
        }
        3 => Message::Joined(reader.id()?, reader.rider()?),
        4 => Message::Left(reader.id()?),
        5 => {
            let millis = u32::from_be_bytes(reader.take()?);
            if millis > 60_000 {
                return Err(LobbyError::Protocol);
            }
            Message::Start(millis)
        }
        6 => Message::VoiceEnabled(match reader.byte()? {
            0 => false,
            1 => true,
            _ => return Err(LobbyError::Protocol),
        }),
        7 => {
            let mut epochs = [0; MAX_RIDERS];
            for epoch in &mut epochs {
                *epoch = u64::from_be_bytes(reader.take()?);
            }
            Message::VoiceRoster(epochs)
        }
        8 => {
            let selection =
                crate::activity::Selection::decode(reader.0).map_err(|_| LobbyError::Protocol)?;
            reader.0 = &[];
            Message::Selection(selection)
        }
        9 => {
            let ready = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(LobbyError::Protocol),
            };
            Message::Ready(u64::from_be_bytes(reader.take()?), ready)
        }
        10 => {
            let id = reader.id()?;
            let ready = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(LobbyError::Protocol),
            };
            Message::Readiness(id, u64::from_be_bytes(reader.take()?), ready)
        }
        11 => {
            let age = i64::from_be_bytes(reader.take()?);
            crate::lobby::entry::clock(age)?;
            Message::Running(age)
        }
        12 => Message::Enter(u64::from_be_bytes(reader.take()?)),
        13 => reader.entry()?,
        14 => reader.workout()?,
        15 => {
            let message = crate::handover::Message::decode(reader.0)?;
            reader.0 = &[];
            Message::Handover(message)
        }
        _ => return Err(LobbyError::Protocol),
    };
    if !reader.0.is_empty() {
        return Err(LobbyError::Protocol);
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orqwa_domain::units::Kilograms;

    #[test]
    fn untrusted_controls_reject_truncation_trailing_data_duplicate_ids_and_invalid_riders() {
        let who = Rider {
            name: "Anna".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(74.0),
            avatar: Avatar::Male,
        };
        let valid = encode(&Message::Hello(RouteHash([1; 32]), who.clone())).unwrap();
        assert!(matches!(decode(&valid), Ok(Message::Hello(_, ref sent)) if *sent == who));
        for len in 0..valid.len() {
            assert!(decode(&valid[..len]).is_err());
        }
        let mut extra = valid.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        // Mass, then weight, then the avatar end a rider.
        let (mass_at, weight_at, avatar_at) = (valid.len() - 17, valid.len() - 9, valid.len() - 1);
        for mass in [f64::NAN, f64::INFINITY, -1.0, 1000.0, 70.0] {
            let mut invalid = valid.clone();
            invalid[mass_at..weight_at].copy_from_slice(&mass.to_be_bytes());
            assert!(decode(&invalid).is_err(), "mass {mass}");
        }
        // A weight above the rider-plus-bicycle mass is as invalid as one out of range.
        for weight in [f64::NAN, f64::INFINITY, 10.0, 90.0] {
            let mut invalid = valid.clone();
            invalid[weight_at..avatar_at].copy_from_slice(&weight.to_be_bytes());
            assert!(decode(&invalid).is_err(), "weight {weight}");
        }
        let mut unknown = valid.clone();
        unknown[avatar_at] = 2;
        assert!(decode(&unknown).is_err(), "an avatar nobody knows");
        let mut roster: Roster = std::array::from_fn(|_| None);
        roster[0] = Some(who.clone());
        roster[1] = Some(who);
        let welcome = encode(&Message::Welcome(
            RiderId::new(1).unwrap(),
            RiderId::HOST,
            crate::code::RideId([1; 16]),
            1,
            roster,
        ))
        .unwrap();
        assert!(decode(&welcome).is_ok());
        let mut duplicate = welcome;
        // The second rider's slot, after the first's id, name, mass, weight and avatar.
        duplicate[52] = 0;
        assert!(decode(&duplicate).is_err());
        assert!(decode(&vec![0; MAX_FRAME + 1]).is_err());
        assert!(decode(&[crate::code::VERSION, 5, 0, 1, 0, 0]).is_err());
    }

    #[test]
    fn late_entry_frames_bound_coordinates_clocks_and_optional_workout_time() {
        for message in [
            Message::Running(172_800_000),
            Message::Running(-60_000),
            Message::Enter(1),
            Message::Entry(1, None),
            Message::Entry(
                1,
                Some(crate::lobby::RideProgress {
                    distance: orqwa_domain::units::Meters(4200.0),
                    workout: Some(Duration::from_secs(700)),
                }),
            ),
        ] {
            let bytes = encode(&message).unwrap();
            assert!(decode(&bytes).is_ok());
            for len in 0..bytes.len() {
                assert!(decode(&bytes[..len]).is_err());
            }
            let mut extra = bytes;
            extra.push(0);
            assert!(decode(&extra).is_err());
        }
        for age in [i64::MIN, -60_001, 172_800_001, i64::MAX] {
            assert!(encode(&Message::Running(age)).is_err());
        }
        for distance in [f64::NAN, f64::INFINITY, -1.0, 10_000_001.0] {
            assert!(
                encode(&Message::Entry(
                    1,
                    Some(crate::lobby::RideProgress {
                        distance: orqwa_domain::units::Meters(distance),
                        workout: None
                    })
                ))
                .is_err()
            );
        }
        assert!(
            encode(&Message::Entry(
                1,
                Some(crate::lobby::RideProgress {
                    distance: orqwa_domain::units::Meters(0.0),
                    workout: Some(Duration::from_hours(49))
                })
            ))
            .is_err()
        );
    }

    #[test]
    fn shared_workout_frames_bound_every_counter_clock_and_boolean() {
        let sample = crate::lobby::timing::WorkoutSample {
            revision: 1,
            sequence: 5,
            group: Duration::from_secs(100),
            elapsed: Duration::from_secs(700),
            paused: true,
        };
        let bytes = encode(&Message::Workout(sample)).unwrap();
        for len in 0..bytes.len() {
            assert!(decode(&bytes[..len]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        let mut boolean = bytes;
        boolean[2] = 2;
        assert!(decode(&boolean).is_err());
        for invalid in [
            crate::lobby::timing::WorkoutSample {
                revision: 0,
                ..sample
            },
            crate::lobby::timing::WorkoutSample {
                sequence: 0,
                ..sample
            },
            crate::lobby::timing::WorkoutSample {
                group: Duration::from_hours(49),
                ..sample
            },
            crate::lobby::timing::WorkoutSample {
                elapsed: Duration::from_hours(49),
                ..sample
            },
        ] {
            assert!(encode(&Message::Workout(invalid)).is_err());
        }
    }
}
