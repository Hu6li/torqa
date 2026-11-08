//! Properties the fuzz targets in `core/fuzz` check on untrusted input (ADR 0035): a decoder
//! never panics, and whatever it accepts encodes again into something it accepts in the same
//! form, so a host never fails to forward what it admitted. Not a supported API: this module
//! exists only with the `fuzzing` feature and in this crate's tests.
#![allow(clippy::missing_panics_doc)] // a broken property must panic: that is the fuzz finding

use std::time::Duration;

use orqwa_domain::units::{Kilograms, Meters, MetersPerSecond, Rpm, Watts};
use orqwa_domain::workout::{Cue, Intensity, Plan, Step, Target};

use crate::activity::{Assistance, ContentId, ContentKind, CourseKey, Selection};
use crate::code::{
    Candidate, CandidateKind, ConnectionMode, Fingerprint, Invite, NatObservation, Reply, RideId,
    RouteHash, Setup,
};
use crate::lobby::Rider;
use crate::policy::AddressPolicy;
use crate::state::{RiderId, RiderState};
use crate::voice::VoiceFrame;
use crate::wire::Message;

// Decoding grants no network access; the widest policy reaches the most parser paths.
const ANY: AddressPolicy = AddressPolicy {
    lan: true,
    loopback: true,
};

/// Pasted text codes and their binary payloads; binary codes have one canonical encoding.
pub fn codes(data: &[u8]) {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(invite) = Invite::from_code(text, ANY) {
            let again = invite.to_code(ANY).expect("an accepted invite encodes");
            assert_eq!(Invite::from_code(&again, ANY), Ok(invite));
        }
        if let Ok(reply) = Reply::from_code(text, ANY) {
            let again = reply.to_code(ANY).expect("an accepted reply encodes");
            assert_eq!(Reply::from_code(&again, ANY), Ok(reply));
        }
    }
    if let Ok(invite) = Invite::decode(data, ANY) {
        assert_eq!(invite.encode(ANY).as_deref(), Ok(data));
    }
    if let Ok(reply) = Reply::decode(data, ANY) {
        assert_eq!(reply.encode(ANY).as_deref(), Ok(data));
    }
}

/// Length-prefixed lobby controls, after their frame length was checked.
pub fn controls(data: &[u8]) {
    if let Ok(message) = crate::wire::decode(data) {
        let bytes = crate::wire::encode(&message).expect("an accepted control encodes");
        let again = crate::wire::decode(&bytes).expect("an encoded control decodes");
        assert_eq!(crate::wire::encode(&again).ok(), Some(bytes));
    }
}

/// Planned handover controls, including plans, replies and commit proofs.
pub fn handover(data: &[u8]) {
    if let Ok(message) = crate::handover::Message::decode(data) {
        let bytes = message
            .encode()
            .expect("an accepted handover control encodes");
        let again = crate::handover::Message::decode(&bytes).expect("it decodes again");
        assert_eq!(again.encode().ok(), Some(bytes));
    }
}

/// Voice datagrams; the first byte picks the authenticated speaker the transport supplies.
pub fn voice(data: &[u8]) {
    let Some((&speaker, packet)) = data.split_first() else {
        return;
    };
    let Ok(speaker) = RiderId::new(speaker % 8) else {
        return;
    };
    if let Ok(frame) = VoiceFrame::parse(packet, speaker) {
        let (bytes, len) = frame.datagram();
        assert_eq!(&bytes[..len], packet, "voice datagrams are canonical");
    }
}

/// Ride state datagrams between a byte picking the sender and one for the receiver's seconds.
pub fn state(data: &[u8]) {
    let [rider, datagram @ .., seconds] = data else {
        return;
    };
    let Ok(rider) = RiderId::new(rider % 8) else {
        return;
    };
    let elapsed = Duration::from_secs(u64::from(*seconds));
    if let Ok(state) = RiderState::from_datagram(datagram, rider, elapsed) {
        let bytes = state.datagram(rider).expect("an accepted state encodes");
        RiderState::from_datagram(&bytes, rider, elapsed).expect("it decodes again");
    }
}

/// Activity selections announced by the host.
pub fn selection(data: &[u8]) {
    if let Ok(selection) = Selection::decode(data) {
        let bytes = selection.encode().expect("an accepted selection encodes");
        assert_eq!(Selection::decode(&bytes), Ok(selection));
    }
}

/// Normalized shared workouts, which the session later runs.
pub fn workout(data: &[u8]) {
    if let Ok(plan) = crate::workout::decode(data) {
        crate::workout::validate(&plan).expect("a decoded workout is valid");
        let bytes = crate::workout::encode(&plan).expect("an accepted workout encodes");
        assert_eq!(crate::workout::decode(&bytes), Ok(plan));
    }
}

/// Previews of what the organizer is choosing (#418), which riders fetch and show.
pub fn preview(data: &[u8]) {
    if let Ok(preview) = crate::preview::Preview::decode(data) {
        let bytes = preview.encode().expect("an accepted preview encodes");
        assert_eq!(crate::preview::Preview::decode(&bytes), Ok(preview));
    }
}

/// STUN responses; the first twelve bytes are the transaction the reader expects.
pub fn stun(data: &[u8]) {
    if let Some((transaction, response)) = data.split_first_chunk::<12>() {
        let _ = crate::stun::parse_binding_response(response, transaction);
    }
}

/// Valid inputs for each property, as starting points for the fuzzer: `(target, bytes)`.
#[must_use]
#[allow(clippy::too_many_lines)] // one seed catalogue covers every bounded control and fuzz target
pub fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    let invite = invite();
    let reply = Reply {
        ride: invite.ride,
        fingerprint: invite.fingerprint,
        route: invite.route,
        client: Fingerprint([4; 32]),
        candidates: invite.candidates.clone(),
        nat: NatObservation::OneServer,
    };
    let rider = Rider {
        name: "Anna".into(),
        mass: Kilograms(80.0),
        weight: Kilograms(70.0),
        avatar: orqwa_domain::profile::Avatar::Female,
    };
    let anna = RiderId::new(1).expect("valid rider");
    let mut roster: crate::lobby::Roster = std::array::from_fn(|_| None);
    roster[0] = Some(rider.clone());
    roster[1] = Some(rider.clone());
    let selection = selection_seed();
    let controls = [
        Message::Hello(RouteHash::LOBBY, rider.clone()),
        Message::Welcome(anna, RiderId::HOST, invite.ride, 2, roster),
        Message::Joined(anna, rider),
        Message::Left(anna),
        Message::Start(5000),
        Message::Lobby(1),
        Message::VoiceEnabled(true),
        Message::VoiceRoster([1, 2, 0, 0, 0, 0, 0, 0]),
        Message::Selection(selection.clone()),
        Message::Ready(1, true),
        Message::Readiness(anna, 1, true),
        Message::Preparation(1, crate::lobby::Stage::Downloading, 40),
        Message::Preparations(anna, 1, crate::lobby::Stage::Preparing, 75),
        Message::Enter(1),
        Message::Away(anna),
        Message::Back(anna),
        Message::Rejoined(true),
        Message::Preview(Some(preview_id())),
        Message::Preview(None),
        Message::Latencies(latency_seed()),
    ];
    let handover = [
        crate::handover::Message::Offer {
            epoch: 2,
            successor: anna,
            standby: true,
        },
        crate::handover::Message::Standby { epoch: 2 },
        crate::handover::Message::Lost { epoch: 2 },
        crate::handover::Message::Found { epoch: 2 },
        crate::handover::Message::Decision {
            epoch: 2,
            accepted: true,
        },
        crate::handover::Message::Reply {
            epoch: 2,
            rider: anna,
            reply: reply.clone(),
        },
    ];
    let voice = VoiceFrame::new(anna, 1, 7, &[0x78; 60]).expect("valid frame");
    let (voice_bytes, voice_len) = voice.datagram();
    let mut voice_seed = vec![anna.value()];
    voice_seed.extend_from_slice(&voice_bytes[..voice_len]);
    let mut state_seed = vec![anna.value()];
    state_seed.extend_from_slice(
        &RiderState {
            at: Duration::from_millis(1500),
            distance: Meters(1200.0),
            speed: MetersPerSecond(9.5),
            power: Watts(210.0),
            cadence: Rpm(88.0),
        }
        .datagram(anna)
        .expect("valid state"),
    );
    state_seed.push(0);
    let mut seeds = vec![
        ("codes", invite.encode(ANY).expect("valid invite")),
        ("codes", reply.encode(ANY).expect("valid reply")),
        (
            "codes",
            invite.to_code(ANY).expect("valid invite").into_bytes(),
        ),
        ("voice", voice_seed),
        ("state", state_seed),
        ("selection", selection.encode().expect("valid selection")),
        (
            "workout",
            crate::workout::encode(&plan()).expect("valid workout"),
        ),
        ("stun", stun_seed()),
        ("preview", preview_seed()),
    ];
    for message in &controls {
        seeds.push(("controls", crate::wire::encode(message).expect("valid")));
    }
    for message in &handover {
        seeds.push(("handover", message.encode().expect("valid")));
    }
    seeds
}

fn invite() -> Invite {
    Invite {
        ride: RideId([1; 16]),
        fingerprint: Fingerprint([2; 32]),
        route: RouteHash::LOBBY,
        host: "Chloé".into(),
        course: "Group ride".into(),
        candidates: vec![
            Candidate::new(
                CandidateKind::Stun,
                "1.1.1.1:50000".parse().expect("ip"),
                ANY,
            )
            .expect("public"),
            Candidate::new(
                CandidateKind::Local,
                "192.168.1.2:50000".parse().expect("ip"),
                ANY,
            )
            .expect("local"),
        ],
        nat: NatObservation::SameMapping,
        setup: Setup::new(ConnectionMode::Mixed, true),
    }
}

fn latency_seed() -> [Option<Duration>; crate::state::MAX_RIDERS] {
    let mut values = [None; crate::state::MAX_RIDERS];
    values[1] = Some(Duration::from_millis(45));
    values[2] = Some(Duration::ZERO);
    values
}

fn selection_seed() -> Selection {
    Selection {
        revision: 1,
        name: "Col du Galibier".into(),
        course: Some(ContentId::new(ContentKind::Course, 4096, [5; 32]).expect("course")),
        course_key: Some(CourseKey([7; 32])),
        workout: Some(ContentId::new(ContentKind::Workout, 512, [6; 32]).expect("workout")),
        assistance: Assistance {
            drafting: true,
            keep_together: false,
        },
    }
}

fn preview_id() -> ContentId {
    ContentId::new(ContentKind::Preview, 2048, [8; 32]).expect("preview")
}

fn preview_seed() -> Vec<u8> {
    crate::preview::Preview {
        course: Some(crate::preview::CoursePreview {
            name: "Col du Galibier".into(),
            length: Meters(34_800.0),
            climbing: Meters(2120.0),
            steepest: 10.1,
            track: vec![[0.0, 0.0], [850.5, 1200.0], [-40.0, 2400.0]],
            profile: vec![[0.0, 720.0], [17_000.0, 1400.0], [34_800.0, 2642.0]],
            picture: b"\x89PNG\r\n\x1a\n picture".to_vec(),
        }),
        workout: Some(Plan {
            cues: Vec::new(),
            ..plan()
        }),
    }
    .encode()
    .expect("valid preview")
}

fn plan() -> Plan {
    Plan {
        name: "Together".into(),
        description: "A shared workout".into(),
        steps: vec![
            Step {
                duration: Duration::from_secs(60),
                target: Target::Power {
                    from: Intensity::Ftp(0.5),
                    to: Intensity::Ftp(0.9),
                },
                cadence: Some(Rpm(90.0)),
            },
            Step {
                duration: Duration::from_secs(30),
                target: Target::steady(Intensity::Watts(Watts(180.0))),
                cadence: None,
            },
            Step {
                duration: Duration::from_secs(10),
                target: Target::Free,
                cadence: None,
            },
        ],
        cues: vec![Cue {
            at: Duration::from_secs(50),
            text: "Almost there".into(),
        }],
    }
}

// RFC 5769 section 2.2's response, preceded by its transaction ID.
fn stun_seed() -> Vec<u8> {
    let hex = concat!(
        "b7e7a701bc34d686fa87dfae",
        "0101003c2112a442b7e7a701bc34d686fa87dfae",
        "8022000b7465737420766563746f7220",
        "002000080001a147e112a643",
        "000800142b91f599fd9e90c38c7489f92af9ba53f06be7d7",
        "80280004c07d4c96"
    );
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property(target: &str) -> fn(&[u8]) {
        match target {
            "codes" => codes,
            "controls" => controls,
            "handover" => handover,
            "voice" => voice,
            "state" => state,
            "selection" => selection,
            "workout" => workout,
            "stun" => stun,
            "preview" => preview,
            _ => panic!("unknown fuzz target {target}"),
        }
    }

    #[test]
    fn fuzz_properties_hold_for_valid_seeds_their_truncations_and_bit_flips() {
        for (target, seed) in seeds() {
            let check = property(target);
            check(&seed);
            for end in 0..seed.len() {
                check(&seed[..end]);
            }
            for at in 0..seed.len() {
                for bit in 0..8 {
                    let mut flipped = seed.clone();
                    flipped[at] ^= 1 << bit;
                    check(&flipped);
                }
            }
        }
    }

    // scripts/fuzz.sh runs this before fuzzing, so the corpus always starts from current seeds.
    #[test]
    #[ignore = "writes the fuzz corpus named by ORQWA_FUZZ_CORPUS"]
    fn write_fuzz_seeds() {
        let root = std::path::PathBuf::from(
            std::env::var_os("ORQWA_FUZZ_CORPUS").expect("ORQWA_FUZZ_CORPUS"),
        );
        for (index, (target, seed)) in seeds().into_iter().enumerate() {
            let directory = root.join(target);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("seed-{index:02}")), seed).unwrap();
        }
    }

    #[test]
    fn every_seed_is_accepted_by_its_own_decoder() {
        let accepted = |target: &str, seed: &[u8]| match target {
            "codes" => {
                Invite::decode(seed, ANY).is_ok()
                    || Reply::decode(seed, ANY).is_ok()
                    || std::str::from_utf8(seed)
                        .is_ok_and(|text| Invite::from_code(text, ANY).is_ok())
            }
            "controls" => crate::wire::decode(seed).is_ok(),
            "handover" => crate::handover::Message::decode(seed).is_ok(),
            "voice" => RiderId::new(seed[0] % 8)
                .is_ok_and(|speaker| VoiceFrame::parse(&seed[1..], speaker).is_ok()),
            "state" => RiderId::new(seed[0] % 8).is_ok_and(|rider| {
                RiderState::from_datagram(&seed[1..seed.len() - 1], rider, Duration::ZERO).is_ok()
            }),
            "selection" => Selection::decode(seed).is_ok(),
            "workout" => crate::workout::decode(seed).is_ok(),
            "stun" => crate::stun::parse_binding_response(
                &seed[12..],
                seed[..12].try_into().expect("transaction"),
            )
            .is_some(),
            "preview" => crate::preview::Preview::decode(seed).is_ok(),
            _ => false,
        };
        for (target, seed) in seeds() {
            assert!(accepted(target, &seed), "{target} seed is rejected");
        }
    }
}
