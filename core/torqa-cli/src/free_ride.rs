//! Riding without a route: resistance is set from the keyboard.

use std::time::Duration;

use anyhow::{Context, Result};
use torqa_devices::DeviceEvent;
use torqa_domain::telemetry::{SimulationParameters, Telemetry, TrainerControl};
use torqa_domain::units::{GradePercent, MetersPerSecond, Percent, Watts};

use crate::devices::{Devices, field, next_event, stdin_lines};

const HELP: &str = "Commands: g <grade %> | p <watts> (ERG) | r <resistance %> | q";

pub(crate) async fn run(devices: &mut Devices) -> Result<()> {
    println!("{HELP}");
    let mut state = Telemetry::default();
    let mut input = stdin_lines();
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            event = devices.trainer.next_event() => {
                let event = event.context("trainer driver stopped")?;
                report(devices.trainer.name(), &event, &mut state);
            }
            event = next_event(devices.sensor.as_mut()) => {
                let (name, event) = event.context("heart-rate driver stopped")?;
                report(&name, &event, &mut state);
            }
            line = input.recv() => {
                let Some(line) = line else { break };
                match parse_input(&line) {
                    Ok(Input::Control(control)) => devices.trainer.control(control).await?,
                    Ok(Input::Quit) => break,
                    Ok(Input::Nothing) => {}
                    Err(message) => eprintln!("{message}\n{HELP}"),
                }
            }
            _ = ticker.tick() => println!("{}", format_state(&state)),
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}

fn report(name: &str, event: &DeviceEvent, state: &mut Telemetry) {
    match event {
        DeviceEvent::Connected => println!("{name}: connected"),
        DeviceEvent::Disconnected => println!("{name}: disconnected, reconnecting…"),
        DeviceEvent::Telemetry(telemetry) => state.merge(telemetry),
        DeviceEvent::Buttons(presses) => println!("{name}: {presses:?}"),
    }
}

fn format_state(state: &Telemetry) -> String {
    [
        field(state.power.map(|p| p.0), "W", 0),
        field(state.cadence.map(|c| c.0), "rpm", 0),
        field(
            state.speed.map(MetersPerSecond::as_kilometers_per_hour),
            "km/h",
            1,
        ),
        field(state.heart_rate.map(|h| h.0), "bpm", 0),
    ]
    .join("  ")
}

#[derive(Debug, PartialEq)]
enum Input {
    Control(TrainerControl),
    Quit,
    Nothing,
}

fn parse_input(line: &str) -> Result<Input, String> {
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return Ok(Input::Nothing);
    };
    if command == "q" {
        return Ok(Input::Quit);
    }
    let value: f64 = parts
        .next()
        .ok_or_else(|| format!("missing value for {command:?}"))?
        .parse()
        .map_err(|_| "value must be a number".to_owned())?;
    let control = match command {
        "g" => TrainerControl::Simulation(SimulationParameters {
            grade: GradePercent(value),
            ..SimulationParameters::default()
        }),
        "p" => TrainerControl::TargetPower(Watts(value)),
        "r" => TrainerControl::Resistance(Percent(value)),
        other => return Err(format!("unknown command {other:?}")),
    };
    Ok(Input::Control(control))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keyboard_commands() {
        assert_eq!(
            parse_input("g -3.5"),
            Ok(Input::Control(TrainerControl::Simulation(
                SimulationParameters {
                    grade: GradePercent(-3.5),
                    ..SimulationParameters::default()
                }
            )))
        );
        assert_eq!(
            parse_input("p 250"),
            Ok(Input::Control(TrainerControl::TargetPower(Watts(250.0))))
        );
        assert_eq!(
            parse_input(" r 40 "),
            Ok(Input::Control(TrainerControl::Resistance(Percent(40.0))))
        );
        assert_eq!(parse_input("q"), Ok(Input::Quit));
        assert_eq!(parse_input(""), Ok(Input::Nothing));
    }

    #[test]
    fn rejects_invalid_commands() {
        assert!(parse_input("g").is_err());
        assert!(parse_input("p fast").is_err());
        assert!(parse_input("x 1").is_err());
    }
}
