//! Headless command-line interface to the Torqa core.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use tokio::sync::mpsc;
use torqa_devices::ble::{Bluetooth, DeviceKind, DiscoveredDevice};
use torqa_devices::fake::{self, FakeRider};
use torqa_devices::{DeviceEvent, DeviceHandle};
use torqa_domain::telemetry::{SimulationParameters, Telemetry, TrainerControl};
use torqa_domain::units::{GradePercent, MetersPerSecond, Percent, Rpm, Watts};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "torqa-cli",
    version,
    about = "Headless Torqa: find and ride smart trainers"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List nearby trainers and heart-rate sensors.
    Scan {
        /// Scan duration in seconds.
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
    /// Ride a trainer and change its resistance from the keyboard.
    Ride(RideArgs),
}

#[derive(Args)]
struct RideArgs {
    /// Trainer name (case-insensitive substring); defaults to the strongest signal.
    #[arg(long, conflicts_with = "fake")]
    trainer: Option<String>,
    /// Use the fake trainer instead of Bluetooth.
    #[arg(long)]
    fake: bool,
    /// Also connect a heart-rate sensor, optionally selected by name.
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    hr: Option<String>,
    /// Power of the fake rider in watts.
    #[arg(long, default_value_t = 200.0)]
    fake_power: f64,
    /// Cadence of the fake rider in rpm.
    #[arg(long, default_value_t = 90.0)]
    fake_cadence: f64,
    /// Bluetooth scan duration in seconds.
    #[arg(long, default_value_t = 5)]
    scan_seconds: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match Cli::parse().command {
        Command::Scan { seconds } => scan(seconds).await,
        Command::Ride(args) => ride(args).await,
    }
}

async fn scan(seconds: u64) -> Result<()> {
    let bluetooth = Bluetooth::new().await?;
    println!("Scanning for {seconds} s…");
    let devices = bluetooth.scan(Duration::from_secs(seconds)).await?;
    if devices.is_empty() {
        println!("No trainers or heart-rate sensors found.");
    }
    for device in &devices {
        let rssi = device
            .rssi
            .map_or_else(|| "?".to_owned(), |rssi| format!("{rssi} dBm"));
        println!(
            "{:<10} {:>8}  {}",
            kind_label(device.kind),
            rssi,
            device.name
        );
    }
    Ok(())
}

async fn ride(args: RideArgs) -> Result<()> {
    let bluetooth = if !args.fake || args.hr.is_some() {
        let bluetooth = Bluetooth::new().await?;
        println!("Scanning for {} s…", args.scan_seconds);
        let devices = bluetooth
            .scan(Duration::from_secs(args.scan_seconds))
            .await?;
        Some((bluetooth, devices))
    } else {
        None
    };

    let mut trainer = match &bluetooth {
        _ if args.fake => fake::spawn(
            FakeRider {
                power: Watts(args.fake_power),
                cadence: Rpm(args.fake_cadence),
            },
            Duration::from_millis(250),
        ),
        Some((bluetooth, devices)) => bluetooth.connect(pick(
            devices,
            DeviceKind::Trainer,
            args.trainer.as_deref().unwrap_or(""),
        )?),
        None => unreachable!("Bluetooth is opened whenever the trainer is not fake"),
    };
    // Straps only advertise while worn and not connected elsewhere; ride on without one.
    let mut sensor = match (&bluetooth, &args.hr) {
        (Some((bluetooth, devices)), Some(name)) => {
            match pick(devices, DeviceKind::HeartRateSensor, name) {
                Ok(device) => Some(bluetooth.connect(device)),
                Err(error) => {
                    eprintln!("Continuing without heart rate: {error:#}");
                    None
                }
            }
        }
        _ => None,
    };

    let result = ride_loop(&mut trainer, sensor.as_mut()).await;

    println!("Disconnecting… (Ctrl+C again to quit immediately)");
    let close_all = async {
        trainer.close(CLOSE_TIMEOUT).await;
        if let Some(sensor) = sensor {
            sensor.close(CLOSE_TIMEOUT).await;
        }
    };
    tokio::select! {
        () = close_all => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    result
}

const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

async fn ride_loop(
    trainer: &mut DeviceHandle,
    mut sensor: Option<&mut DeviceHandle>,
) -> Result<()> {
    println!("{HELP}");
    let mut state = Telemetry::default();
    let mut input = stdin_lines();
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            event = trainer.next_event() => {
                let event = event.context("trainer driver stopped")?;
                report(trainer.name(), &event, &mut state);
            }
            event = next_event(sensor.as_deref_mut()) => {
                let (name, event) = event.context("heart-rate driver stopped")?;
                report(&name, &event, &mut state);
            }
            line = input.recv() => {
                let Some(line) = line else { break };
                match parse_input(&line) {
                    Ok(Input::Control(control)) => trainer.control(control).await?,
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

/// Reads stdin lines on a plain thread.
///
/// Tokio's stdin reads on a blocking-pool thread that runtime shutdown waits for, so the
/// process could not exit while a read is pending.
fn stdin_lines() -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel(16);
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if tx.blocking_send(line).is_err() {
                break;
            }
        }
    });
    rx
}

const HELP: &str = "Commands: g <grade %> | p <watts> (ERG) | r <resistance %> | q";

fn kind_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Trainer => "trainer",
        DeviceKind::HeartRateSensor => "heart rate",
    }
}

/// Selects the strongest device of `kind` whose name contains `name` (case-insensitive).
fn pick(devices: &[DiscoveredDevice], kind: DeviceKind, name: &str) -> Result<DiscoveredDevice> {
    let wanted = name.to_lowercase();
    devices
        .iter()
        .find(|d| d.kind == kind && d.name.to_lowercase().contains(&wanted))
        .cloned()
        .with_context(|| {
            let found: Vec<_> = devices.iter().map(|d| d.name.as_str()).collect();
            format!(
                "no {} matching {name:?} found (found: {found:?})",
                kind_label(kind)
            )
        })
}

/// Waits for the next event of an optional device; never completes without one.
async fn next_event(device: Option<&mut DeviceHandle>) -> Option<(String, DeviceEvent)> {
    match device {
        Some(device) => {
            let event = device.next_event().await?;
            Some((device.name().to_owned(), event))
        }
        None => std::future::pending().await,
    }
}

fn report(name: &str, event: &DeviceEvent, state: &mut Telemetry) {
    match event {
        DeviceEvent::Connected => println!("{name}: connected"),
        DeviceEvent::Disconnected => println!("{name}: disconnected, reconnecting…"),
        DeviceEvent::Telemetry(telemetry) => state.merge(telemetry),
    }
}

fn format_state(state: &Telemetry) -> String {
    let field = |value: Option<f64>, unit: &str, precision: usize| {
        value.map_or_else(
            || format!("{:>9}", format!("-- {unit}")),
            |v| format!("{:>9}", format!("{v:.precision$} {unit}")),
        )
    };
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

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
