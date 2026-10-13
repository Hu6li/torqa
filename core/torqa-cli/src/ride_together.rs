//! `ride-together`: a group ride along a GPX route (R67, ADR 0015), hosted or joined from an
//! invite. Each rides their own trainer and physics; the others' gaps are printed alongside.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use clap::Args;
use tokio::time::{Instant, interval, sleep_until};
use torqa_devices::DeviceEvent;
use torqa_domain::units::{Kilograms, Percent};
use torqa_net::{
    Course, Host, HostLobby, Invite, JoinError, Joiner, JoinerLobby, LobbyEvent, Reply, Rider,
    RiderId, RiderState, STATE_EVERY,
};
use torqa_physics::RiderSetup;
use torqa_session::{Ride, RideConfig};

use crate::devices::{DeviceArgs, Devices, stdin_lines};
use crate::net_ping::{NetArgs, hint_text, print_candidates};
use crate::recorded_ride::{format_state, save};

const TICK: Duration = Duration::from_millis(250);

#[derive(Args)]
pub(crate) struct RideTogetherArgs {
    #[command(flatten)]
    devices: DeviceArgs,
    /// GPX route to ride; everyone rides the same file.
    #[arg(long)]
    route: PathBuf,
    /// Join the ride of this invite instead of hosting one.
    #[arg(long)]
    join: Option<String>,
    /// Your name, as the others see it.
    #[arg(long, default_value = "Rider")]
    name: String,
    /// Seconds from the host's `go` to the start.
    #[arg(long, default_value_t = 10)]
    countdown: u64,
    /// Trainer difficulty in percent: how much of the road gradient you feel.
    #[arg(long, default_value_t = 50.0)]
    difficulty: f64,
    /// Rider plus bike mass in kg.
    #[arg(long, default_value_t = 83.0)]
    mass: f64,
    /// Only use cached terrain data.
    #[arg(long)]
    offline: bool,
    /// Where to save the FIT activity (default: torqa-<date>-<time>.fit).
    #[arg(long)]
    output: Option<PathBuf>,
    #[command(flatten)]
    net: NetArgs,
}

/// The rider's side of the ride.
enum Group {
    Host(HostLobby),
    Joiner(Box<JoinerLobby>),
}

impl Group {
    async fn next(&mut self) -> Option<LobbyEvent> {
        match self {
            Self::Host(lobby) => lobby.next().await,
            Self::Joiner(lobby) => lobby.next().await,
        }
    }

    fn send_state(&self, state: &RiderState) {
        match self {
            Self::Host(lobby) => lobby.send_state(state),
            Self::Joiner(lobby) => lobby.send_state(state),
        }
    }

    async fn close(self) {
        match self {
            Self::Host(lobby) => lobby.close().await,
            Self::Joiner(lobby) => lobby.leave().await,
        }
    }
}

pub(crate) async fn run(args: &RideTogetherArgs) -> Result<()> {
    let bytes = std::fs::read(&args.route)
        .with_context(|| format!("cannot read {}", args.route.display()))?;
    let route = crate::load_route(&args.route, args.offline).await?;
    crate::print_route(&route);
    let name = route.name().map_or_else(
        || args.route.file_stem().unwrap_or_default().to_string_lossy(),
        Into::into,
    );
    let course = Course::new(&name, &bytes);
    let me = Rider {
        name: args.name.clone(),
        mass: Kilograms(args.mass),
    };
    let options = args.net.options();

    let mut group = match &args.join {
        None => {
            let host = Host::open(&options).await?;
            let invite = host.invite(&me.name, course);
            print_candidates(&invite.candidates);
            println!(
                "\nInvite — pass it on to whoever joins:\n\n{}\n",
                invite.to_code()
            );
            println!("Paste each reply and press Enter; type go to start the countdown.");
            Group::Host(HostLobby::open(host, me))
        }
        Some(code) => {
            let invite = Invite::from_code(code)?;
            if invite.course.route_hash != course.route_hash {
                bail!(
                    "{} rides {}; --route must be the same GPX file.",
                    invite.host,
                    invite.course.name
                );
            }
            let host = invite.host.clone();
            let joiner = Joiner::open(invite, &options).await?;
            let reply = joiner.reply();
            print_candidates(&reply.candidates);
            println!(
                "\nReply — pass it on to {host} within {} s:\n\n{}\n",
                options.window.as_secs(),
                reply.to_code()
            );
            match JoinerLobby::join(joiner, &me).await {
                Ok(lobby) => {
                    println!("In. Waiting for {host} to start…");
                    Group::Joiner(Box::new(lobby))
                }
                Err(torqa_net::LobbyError::Join(JoinError::Unreachable(hint))) => {
                    bail!("{host} could not be reached: {}", hint_text(hint))
                }
                Err(e) => bail!("{e}"),
            }
        }
    };

    let mut devices = crate::devices::connect(&args.devices).await?;
    let config = RideConfig {
        setup: RiderSetup {
            mass: Kilograms(args.mass),
            ..RiderSetup::default()
        },
        difficulty: Percent(args.difficulty),
        ..RideConfig::default()
    };
    let result = ride(Ride::new(route, config), args, &mut group, &mut devices).await;
    group.close().await;
    devices.close().await;
    result
}

async fn ride(
    mut ride: Ride,
    args: &RideTogetherArgs,
    group: &mut Group,
    devices: &mut Devices,
) -> Result<()> {
    let mut input = stdin_lines();
    let mut ticker = interval(TICK);
    let mut sending = interval(STATE_EVERY);
    let mut display = interval(Duration::from_secs(1));
    let mut others = Others::default();
    let mut start: Option<Instant> = None;
    let mut started: Option<SystemTime> = None;
    let mut last_tick = Instant::now();
    let mut connected = true;

    loop {
        let begun = started.is_some();
        tokio::select! {
            event = devices.trainer.next_event() => {
                match event.context("trainer driver stopped")? {
                    DeviceEvent::Connected => println!("{}: connected", devices.trainer.name()),
                    DeviceEvent::Disconnected => {
                        println!("{}: disconnected, reconnecting…", devices.trainer.name());
                        ride.on_power_source_lost();
                    }
                    DeviceEvent::Telemetry(telemetry) => ride.on_telemetry(&telemetry),
                    DeviceEvent::Buttons(_) => {}
                }
            }
            event = group.next(), if connected => match event {
                Some(LobbyEvent::Start { at }) => {
                    start = Some(at);
                    let left = at.saturating_duration_since(Instant::now());
                    println!("Starting in {:.0} s", left.as_secs_f64().ceil());
                }
                Some(event) => others.on(event),
                None => connected = false,
            },
            () = sleep_until(start.unwrap_or_else(Instant::now)), if start.is_some() && !begun => {
                started = Some(SystemTime::now());
                last_tick = Instant::now();
                println!("Go!");
            }
            _ = ticker.tick(), if begun => {
                let now = Instant::now();
                if let Some(control) = ride.tick(now - last_tick) {
                    devices.trainer.control(control).await?;
                }
                last_tick = now;
                if ride.is_finished() {
                    println!("{}\nFinished!", format_state(&ride.state()));
                    break;
                }
            }
            _ = sending.tick(), if begun => {
                let state = ride.state();
                group.send_state(&RiderState {
                    at: state.elapsed,
                    distance: state.distance,
                    speed: state.speed,
                    power: state.telemetry.power.unwrap_or_default(),
                    cadence: state.telemetry.cadence.unwrap_or_default(),
                });
            }
            _ = display.tick(), if begun => {
                let state = ride.state();
                println!("{}{}", format_state(&state), others.gaps(state.distance.0));
            }
            line = input.recv() => match line.as_deref().map(str::trim) {
                None | Some("q") => break,
                Some("go") if start.is_none() => match group {
                    Group::Host(lobby) => {
                        start = Some(lobby.start(Duration::from_secs(args.countdown)));
                        println!("Starting in {} s", args.countdown);
                    }
                    Group::Joiner(_) => println!("The host starts the ride."),
                },
                Some("") => {}
                Some(code) => match group {
                    Group::Host(lobby) if start.is_none() => match Reply::from_code(code) {
                        Ok(reply) => {
                            lobby.admit(&reply);
                            println!("Punching towards them…");
                        }
                        Err(e) => println!("{e}"),
                    },
                    _ => {}
                },
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    match started {
        Some(start) if !ride.samples().is_empty() => save(&ride, start, args.output.as_deref()),
        _ => {
            println!("Nothing recorded.");
            Ok(())
        }
    }
}

/// The other riders, as far as this rider knows.
#[derive(Default)]
struct Others {
    names: BTreeMap<RiderId, String>,
    distances: BTreeMap<RiderId, f64>,
}

impl Others {
    fn on(&mut self, event: LobbyEvent) {
        match event {
            LobbyEvent::Joined { id, rider } => {
                println!("{} is in", rider.name);
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
            LobbyEvent::GaveUp { hint } => println!("Gave up on a reply: {}", hint_text(hint)),
            LobbyEvent::HostLeft => {
                println!("The host left; riding on alone.");
                self.names.clear();
                self.distances.clear();
            }
            LobbyEvent::Start { .. } => {}
        }
    }

    /// Each other rider's gap: ahead is +.
    fn gaps(&self, mine: f64) -> String {
        let mut gaps = String::new();
        for (id, distance) in &self.distances {
            let name = self.names.get(id).map_or("?", String::as_str);
            let _ = write!(gaps, "  {name} {:+.0} m", distance - mine);
        }
        gaps
    }
}
