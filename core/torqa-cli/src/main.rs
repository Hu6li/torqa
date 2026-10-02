//! Headless command-line interface to the Torqa core.

mod devices;
mod free_ride;
mod route_ride;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use torqa_app::paths;
use torqa_devices::ble::Bluetooth;
use torqa_physics::DescentMode;
use torqa_routes::{ElevationSource, Route};
use torqa_terrain::{Terrain, TileSource};
use tracing_subscriber::EnvFilter;

use devices::DeviceArgs;

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
    /// Ride a GPX route, or control the trainer from the keyboard without one.
    Ride(RideArgs),
    /// Show length, climbing and elevation source of a GPX route.
    Route(RouteArgs),
}

#[derive(Args)]
struct RideArgs {
    #[command(flatten)]
    devices: DeviceArgs,
    /// GPX route to ride; without it, resistance is set from the keyboard.
    #[arg(long)]
    route: Option<PathBuf>,
    /// Trainer difficulty in percent: how much of the road gradient you feel.
    #[arg(long, default_value_t = 50.0)]
    difficulty: f64,
    /// How descents behave.
    #[arg(long, value_enum, default_value_t = Descent::Coast)]
    descent: Descent,
    /// Rider plus bike mass in kg.
    #[arg(long, default_value_t = 83.0)]
    mass: f64,
    /// Only use cached terrain data.
    #[arg(long)]
    offline: bool,
    /// Where to save the FIT activity (default: torqa-<date>-<time>.fit).
    #[arg(long)]
    output: Option<PathBuf>,
    /// Runs simulated time faster, for testing routes quickly with the fake trainer.
    #[arg(long, default_value_t = 1.0, requires = "fake")]
    time_scale: f64,
}

#[derive(Args)]
struct RouteArgs {
    /// GPX file.
    file: PathBuf,
    /// Only use cached terrain data.
    #[arg(long)]
    offline: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Descent {
    /// Gravity builds speed and the trainer goes light.
    Coast,
    /// Descents are ridden like flat roads.
    Flat,
}

impl From<Descent> for DescentMode {
    fn from(descent: Descent) -> Self {
        match descent {
            Descent::Coast => Self::Coast,
            Descent::Flat => Self::Flat,
        }
    }
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
        Command::Route(args) => {
            let route = load_route(&args.file, args.offline).await?;
            print_route(&route);
            Ok(())
        }
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
            devices::kind_label(device.kind),
            rssi,
            device.name
        );
    }
    Ok(())
}

async fn ride(args: RideArgs) -> Result<()> {
    let route = match &args.route {
        Some(path) => {
            let route = load_route(path, args.offline).await?;
            print_route(&route);
            Some(route)
        }
        None => None,
    };

    let mut devices = devices::connect(&args.devices).await?;
    let result = match route {
        Some(route) => route_ride::run(route, &args, &mut devices).await,
        None => free_ride::run(&mut devices).await,
    };
    devices.close().await;
    result
}

async fn load_route(path: &std::path::Path, offline: bool) -> Result<Route> {
    let xml =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut terrain = Terrain::new(TileSource::defaults(), paths::cache_dir().join("terrain"));
    if offline {
        terrain = terrain.offline();
    }
    Route::from_gpx(&xml, Some(&mut terrain))
        .await
        .with_context(|| format!("cannot import {}", path.display()))
}

fn print_route(route: &Route) {
    let source = match route.elevation_source() {
        ElevationSource::Terrain => "terrain model (Mapterhorn / AWS Terrain Tiles)",
        ElevationSource::File => "GPX file",
    };
    println!(
        "{}: {:.2} km, {:.0} m climbing, elevation from {source}",
        route.name().unwrap_or("Route"),
        route.length().0 / 1000.0,
        route.elevation_gain().0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn time_scale_requires_the_fake_trainer() {
        assert!(Cli::try_parse_from(["torqa-cli", "ride", "--time-scale", "10"]).is_err());
        assert!(Cli::try_parse_from(["torqa-cli", "ride", "--fake", "--time-scale", "10"]).is_ok());
    }
}
