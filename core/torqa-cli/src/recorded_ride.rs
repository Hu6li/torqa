//! Recorded rides, saved as FIT: along a GPX route the trainer follows the road gradient, in a
//! workout it holds the power the workout asks for.

use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use torqa_app::paths;
use torqa_devices::DeviceEvent;
use torqa_domain::units::MetersPerSecond;
use torqa_session::{Ride, RideState};

use crate::RideArgs;
use crate::devices::{Devices, field, next_event, stdin_lines};

const TICK: Duration = Duration::from_millis(250);

pub(crate) async fn run(mut ride: Ride, args: &RideArgs, devices: &mut Devices) -> Result<()> {
    let mut input = stdin_lines();
    let mut ticker = tokio::time::interval(TICK);
    let mut display = tokio::time::interval(Duration::from_secs(1));
    // The clock starts once the trainer is connected, so waiting is not recorded.
    let mut started: Option<(SystemTime, Instant)> = None;
    let mut last_tick = Instant::now();

    println!("Waiting for the trainer… type q (or Ctrl+C) to stop and save.");
    loop {
        tokio::select! {
            event = devices.trainer.next_event() => {
                match event.context("trainer driver stopped")? {
                    DeviceEvent::Connected => {
                        println!("{}: connected", devices.trainer.name());
                        if started.is_none() {
                            started = Some((SystemTime::now(), Instant::now()));
                            last_tick = Instant::now();
                            println!("Go!");
                        }
                    }
                    DeviceEvent::Disconnected => {
                        println!("{}: disconnected, reconnecting…", devices.trainer.name());
                        ride.on_power_source_lost();
                    }
                    DeviceEvent::Telemetry(telemetry) => ride.on_telemetry(&telemetry),
                }
            }
            event = next_event(devices.sensor.as_mut()) => {
                match event.context("heart-rate driver stopped")? {
                    (_, DeviceEvent::Telemetry(telemetry)) => ride.on_telemetry(&telemetry),
                    (name, DeviceEvent::Connected) => println!("{name}: connected"),
                    (name, DeviceEvent::Disconnected) => println!("{name}: disconnected"),
                }
            }
            _ = ticker.tick(), if started.is_some() => {
                let now = Instant::now();
                let dt = (now - last_tick).mul_f64(args.time_scale);
                last_tick = now;
                if let Some(control) = ride.tick(dt) {
                    devices.trainer.control(control).await?;
                }
                if ride.is_finished() {
                    println!("{}\nFinished!", format_state(&ride.state()));
                    break;
                }
            }
            _ = display.tick(), if started.is_some() => println!("{}", format_state(&ride.state())),
            line = input.recv() => {
                if line.as_deref().is_none_or(|l| l.trim() == "q") {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    match started {
        Some((start, _)) if !ride.samples().is_empty() => {
            save(&ride, start, args.output.as_deref())
        }
        _ => {
            println!("Nothing recorded.");
            Ok(())
        }
    }
}

fn save(ride: &Ride, start: SystemTime, output: Option<&std::path::Path>) -> Result<()> {
    let path = output.map_or_else(|| paths::activity_file_name(start), ToOwned::to_owned);
    let fit = torqa_storage::encode_fit(start, ride.samples())?;
    std::fs::write(&path, fit).with_context(|| format!("cannot write {}", path.display()))?;
    println!("Saved {}", path.display());
    Ok(())
}

fn format_state(state: &RideState) -> String {
    let t = state.telemetry;
    let readings = [
        field(
            Some(MetersPerSecond::as_kilometers_per_hour(state.speed)),
            "km/h",
            1,
        ),
        field(t.power.map(|p| p.0), "W", 0),
        field(t.cadence.map(|c| c.0), "rpm", 0),
        field(t.heart_rate.map(|h| h.0), "bpm", 0),
    ]
    .join("  ");
    let km = state.distance.0 / 1000.0;
    match (state.position, state.remaining, state.workout) {
        (Some(position), Some(remaining), _) => format!(
            "{km:>6.2}/{:.2} km  {:>+5.1} %  {readings}  {:>5.0} m",
            km + remaining.0 / 1000.0,
            position.grade.0,
            position.elevation.0,
        ),
        (_, _, Some(workout)) => {
            let heart_rate = workout
                .target_heart_rate
                .map(|h| format!(" for {:.0} bpm", h.0))
                .unwrap_or_default();
            format!(
                "{km:>6.2} km  {readings}  target {:.0} W{heart_rate}",
                workout.target_power.0
            )
        }
        _ => format!("{km:>6.2} km  {readings}"),
    }
}
