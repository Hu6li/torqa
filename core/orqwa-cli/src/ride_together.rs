//! CLI group ride: networking supplies only a countdown and others' display states.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, ensure};
use clap::Args;
use orqwa_devices::DeviceEvent;
use orqwa_domain::shifting::ButtonMap;
use orqwa_domain::units::{Kilograms, Percent};
use orqwa_net::code::{Invite, Reply};
use orqwa_net::link::{ConnectOptions, Host, Joiner};
use orqwa_net::lobby::{HostLobby, JoinerLobby, LobbyEvent, Rider};
use orqwa_net::state::{RiderId, RiderState, STATE_EVERY};
use orqwa_physics::RiderSetup;
use orqwa_session::{Ride, RideConfig};
use tokio::time::{Instant, interval, sleep_until};

use crate::devices::{self, DeviceArgs, Devices, next_event};
use crate::network::{self, Input, NetworkArgs};
use crate::recorded_ride::{format_state, save};

#[derive(Args)]
pub(crate) struct RideTogetherArgs {
    #[command(flatten)]
    voice: crate::voice::VoiceArgs,
    #[command(flatten)]
    devices: DeviceArgs,
    /// Everyone must have exactly the same GPX file locally.
    #[arg(long)]
    route: PathBuf,
    /// Join this private invite instead of creating a host lobby.
    #[arg(long)]
    join: Option<String>,
    /// Name shown to the others (at most 64 UTF-8 bytes, no controls).
    #[arg(long, default_value = "Rider")]
    name: String,
    /// Seconds from the host's go command to the common start.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=60))]
    countdown: u64,
    /// Share of the gradient felt on your own trainer, in percent.
    #[arg(long, default_value_t = 50.0)]
    difficulty: f64,
    /// Your own rider plus bicycle mass in kilograms (20–300).
    #[arg(long, default_value_t = 83.0)]
    mass: f64,
    /// Your own weight without the bicycle in kilograms, for the others' W/kg (at most --mass).
    #[arg(long, default_value_t = 75.0)]
    weight: f64,
    /// Use cached terrain/map data only; peer networking is controlled separately.
    #[arg(long)]
    offline: bool,
    /// Save your own FIT activity here.
    #[arg(long)]
    output: Option<PathBuf>,
    #[command(flatten)]
    network: NetworkArgs,
}

enum Group {
    Host(HostLobby),
    Joiner(JoinerLobby),
    Offline,
}
impl Group {
    fn take_voice(&mut self) -> Option<orqwa_net::voice::VoicePort> {
        match self {
            Self::Host(lobby) => lobby.take_voice(),
            Self::Joiner(lobby) => lobby.take_voice(),
            Self::Offline => None,
        }
    }
    async fn next(&mut self) -> Option<LobbyEvent> {
        match self {
            Self::Host(lobby) => lobby.next().await,
            Self::Joiner(lobby) => lobby.next().await,
            Self::Offline => std::future::pending().await,
        }
    }
    fn send(&self, state: RiderState) -> Result<()> {
        match self {
            Self::Host(lobby) => lobby.send_state(state)?,
            Self::Joiner(lobby) => lobby.send_state(state)?,
            Self::Offline => {}
        }
        Ok(())
    }
    async fn close(self) {
        match self {
            Self::Host(lobby) => lobby.close().await,
            Self::Joiner(lobby) => lobby.close().await,
            Self::Offline => {}
        }
    }
    async fn disconnect(&mut self) {
        std::mem::replace(self, Self::Offline).close().await;
    }
}

pub(crate) async fn run(args: &RideTogetherArgs) -> Result<()> {
    ensure!(
        args.mass.is_finite() && (20.0..=300.0).contains(&args.mass),
        "mass must be from 20 to 300 kg"
    );
    ensure!(
        args.weight.is_finite() && (20.0..=args.mass).contains(&args.weight),
        "weight must be from 20 kg up to the rider-plus-bike mass"
    );
    ensure!(
        args.difficulty.is_finite() && (0.0..=100.0).contains(&args.difficulty),
        "difficulty must be from 0 to 100 percent"
    );
    // Route preparation and device discovery happen before the one-minute join window.
    let bytes = std::fs::read(&args.route)
        .with_context(|| format!("cannot read {}", args.route.display()))?;
    let hash = orqwa_net::crypto::route_hash(&bytes);
    let route = crate::load_route(&args.route, args.offline).await?;
    crate::print_route(&route);
    let config = RideConfig {
        setup: RiderSetup {
            mass: Kilograms(args.mass),
            ..RiderSetup::default()
        },
        difficulty: Percent(args.difficulty),
        ..RideConfig::default()
    };
    let mut devices = devices::connect(&args.devices).await?;
    let result = with_devices(Ride::new(route, config), hash, args, &mut devices).await;
    devices.close().await;
    result
}

async fn with_devices(
    ride: Ride,
    hash: orqwa_net::code::RouteHash,
    args: &RideTogetherArgs,
    devices: &mut Devices,
) -> Result<()> {
    let options = ConnectOptions {
        gather: args.network.options(),
        ..ConnectOptions::default()
    };
    let me = Rider {
        name: args.name.clone(),
        mass: Kilograms(args.mass),
        weight: Kilograms(args.weight),
        avatar: orqwa_domain::profile::Avatar::default(),
    };
    let mut group = if let Some(code) = &args.join {
        let invite = Invite::from_code(code, options.gather.policy)?;
        ensure!(
            invite.route == hash,
            "Everyone must use exactly the same GPX file."
        );
        let joiner = Joiner::open(invite, hash, &options).await?;
        println!("Reply: {}", joiner.reply().to_code(options.gather.policy)?);
        println!("Send the reply privately to the host within one minute.");
        let lobby = tokio::select! {
            result = JoinerLobby::join(joiner, me) => result?,
            _ = tokio::signal::ctrl_c() => return Ok(()),
        };
        println!("Joined. Waiting for the host's countdown.");
        Group::Joiner(lobby)
    } else {
        let host = Host::open(&args.name, "Shared GPX route", hash, &options).await?;
        let lobby = HostLobby::open(host, me)?;
        println!("Invite: {}", lobby.invite().to_code(options.gather.policy)?);
        println!(
            "Share codes privately. Paste replies here; wait until everyone has joined, then type go."
        );
        Group::Host(lobby)
    };
    let mut voice = crate::voice::Voice::open(&args.voice, group.take_voice());
    let result = ride_loop(ride, args, &mut group, devices, &mut voice).await;
    voice.close().await;
    group.close().await;
    result
}

async fn ride_loop(
    mut ride: Ride,
    args: &RideTogetherArgs,
    group: &mut Group,
    devices: &mut Devices,
    voice: &mut crate::voice::Voice,
) -> Result<()> {
    let buttons = ButtonMap::shifting(args.devices.up_channel, args.devices.down_channel);
    let mut input = network::input();
    let mut tick = interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sending = interval(STATE_EVERY);
    sending.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut display = interval(Duration::from_secs(1));
    display.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut others = Others::default();
    let mut start = None;
    let mut started = None;
    let mut last_tick = Instant::now();
    let mut connected = true;
    println!("q and Enter (or Ctrl+C) saves your own activity and leaves.");
    let result: Result<()> = async {
        loop {
            tokio::select! {
                () = voice.next(), if voice.active() => {},
                event = devices.trainer.next_event() => match event.context("trainer driver stopped")? {
                    DeviceEvent::Connected => println!("{}: connected", devices.trainer.name()),
                    DeviceEvent::Disconnected => { println!("Trainer disconnected; reconnecting."); ride.on_trainer_lost(); }
                    DeviceEvent::Telemetry(telemetry) => ride.on_telemetry(&telemetry),
                    DeviceEvent::Failed(reason) => println!("Trainer: {reason}; trying again"),
                    DeviceEvent::Battery(battery) => println!("Trainer battery {battery}"),
                    DeviceEvent::Buttons(_) => {},
                },
                event = next_event(devices.sensor.as_mut()) => match event.context("heart-rate driver stopped")? {
                    (_, DeviceEvent::Telemetry(telemetry)) => ride.on_telemetry(&telemetry),
                    (name, DeviceEvent::Failed(reason)) => println!("{name}: {reason}; trying again"),
                    (name, DeviceEvent::Battery(battery)) => println!("{name}: battery {battery}"),
                    _ => {},
                },
                event = next_event(devices.controller.as_mut()) => match event.context("controller driver stopped")? {
                    (_, DeviceEvent::Buttons(presses)) => for action in buttons.actions(&presses) { for &shift in action.shifts() { ride.shift(shift); } },
                    (name, DeviceEvent::Failed(reason)) => println!("{name}: {reason}; trying again"),
                    (name, DeviceEvent::Battery(battery)) => println!("{name}: battery {battery}"),
                    _ => {},
                },
                event = group.next(), if connected => match event {
                    Some(LobbyEvent::Start { at }) => if start.is_none() { start = Some(at); println!("Starting in {:.0} s", at.saturating_duration_since(Instant::now()).as_secs_f64().ceil()); },
                    Some(LobbyEvent::HostLeft) | None => { connected = false; others = Others::default(); println!("Group connection ended; continue riding alone. If waiting, type go to start locally."); },
                    Some(event) => { if voice.active() && let LobbyEvent::Joined { id, rider } = &event { println!("Voice rider ID {}: {}",id.value(),rider.name); } others.on(event); },
                },
                () = sleep_until(start.unwrap_or_else(Instant::now)), if start.is_some() && started.is_none() => { started = Some(SystemTime::now()); last_tick = Instant::now(); println!("Go!"); },
                _ = tick.tick(), if started.is_some() => {
                    let now = Instant::now();
                    if let Some(control) = ride.tick(now - last_tick) { devices.trainer.control(control).await?; }
                    last_tick = now;
                    if ride.is_finished() { println!("{}\nFinished!", format_state(&ride.state())); break; }
                },
                _ = sending.tick(), if started.is_some() && connected => {
                    let state = ride.state();
                    if let Err(error) = group.send(RiderState { at: state.elapsed, distance: state.distance, speed: state.speed, power: state.telemetry.power.unwrap_or_default(), cadence: state.telemetry.cadence.unwrap_or_default() }) {
                        connected = false; others = Others::default(); group.disconnect().await; eprintln!("Group state stopped: {error}. Continuing alone.");
                    }
                },
                _ = display.tick(), if started.is_some() => { let state = ride.state(); println!("{}{}", format_state(&state), others.gaps(state.distance.0)); },
                line = input.recv() => match line {
                    None => break,
                    Some(Input::Invalid) => eprintln!("Input refused: invalid text or more than 2048 bytes."),
                    Some(Input::Line(line)) if line == "q" => break,
                    Some(Input::Line(line)) if voice.command(&line) => {},
                    Some(Input::Line(line)) if line == "go" && start.is_none() => {
                        if !connected { start = Some(Instant::now() + Duration::from_secs(1)); }
                        else if let Group::Host(lobby) = group { match lobby.start(Duration::from_secs(args.countdown)).await { Ok(at) => { start = Some(at); println!("Starting in {} s", args.countdown); }, Err(error) => eprintln!("Cannot start: {error}"), } }
                        else { println!("The host starts the countdown."); }
                    }
                    Some(Input::Line(line)) if !line.is_empty() && start.is_none() => if let Group::Host(lobby) = group {
                        match Reply::from_code(&line, args.network.options().policy) {
                            Ok(reply) => match lobby.admit(&reply).await { Ok(()) => println!("Reply admitted; waiting for the rider to join."), Err(error) => eprintln!("Reply refused: {error}"), },
                            Err(error) => eprintln!("Reply refused: {error}"),
                        }
                    },
                    Some(Input::Line(_)) => {},
                },
                _ = tokio::signal::ctrl_c() => break,
            }
        }
        Ok(())
    }.await;
    if let Some(start) = started.filter(|_| !ride.samples().is_empty()) {
        save(&ride, start, args.output.as_deref())?;
    } else {
        println!("Nothing recorded.");
    }
    result
}

#[derive(Default)]
struct Others {
    names: BTreeMap<RiderId, String>,
    distances: BTreeMap<RiderId, f64>,
}
impl Others {
    fn on(&mut self, event: LobbyEvent) {
        match event {
            LobbyEvent::Joined { id, rider } => {
                println!("{} joined", rider.name);
                self.names.insert(id, rider.name);
            }
            LobbyEvent::Left { id } => {
                self.distances.remove(&id);
                if let Some(name) = self.names.remove(&id) {
                    println!("{name} left");
                }
            }
            LobbyEvent::State { id, state } => {
                self.distances.insert(id, state.distance.0);
            }
            LobbyEvent::Expired => println!("A reply expired; ask for a fresh reply."),
            LobbyEvent::Failed {
                peer,
                error,
                operation,
            } => {
                eprintln!("Group networking failed while {operation} (rider {peer:?}): {error}");
            }
            LobbyEvent::Start { .. }
            | LobbyEvent::HostLeft
            | LobbyEvent::Selection(_)
            | LobbyEvent::Readiness { .. }
            | LobbyEvent::Preparation { .. }
            | LobbyEvent::Running { .. }
            | LobbyEvent::Entry { .. }
            | LobbyEvent::Workout(_)
            | LobbyEvent::Handover { .. }
            // CLI rides are course lobbies, which never hold a seat for reconnecting (#434).
            | LobbyEvent::Away { .. }
            | LobbyEvent::Back { .. }
            | LobbyEvent::Reconnecting { .. }
            | LobbyEvent::Reconnected => {}
        }
    }
    fn gaps(&self, mine: f64) -> String {
        let mut text = String::new();
        for (id, distance) in &self.distances {
            if let Some(name) = self.names.get(id) {
                let _ = write!(text, "  {name} {:+.0} m", distance - mine);
            }
        }
        text
    }
}
