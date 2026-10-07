//! Finding, connecting and closing the trainer and heart-rate sensor.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args;
use tokio::sync::mpsc;
use torqa_devices::ble::{Bluetooth, DeviceKind, DiscoveredDevice};
use torqa_devices::fake::{self, FakeHeart, FakeRider};
use torqa_devices::{DeviceEvent, DeviceHandle};
use torqa_domain::units::{Rpm, Watts};

const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Args)]
pub(crate) struct DeviceArgs {
    /// Trainer name (case-insensitive substring); defaults to the strongest signal.
    #[arg(long, conflicts_with = "fake")]
    trainer: Option<String>,
    /// Use the fake trainer instead of Bluetooth.
    #[arg(long)]
    fake: bool,
    /// Also connect a heart-rate sensor, optionally selected by name.
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    hr: Option<String>,
    /// Also connect a Shimano Di2 shifter whose D-Fly buttons shift, optionally by name.
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    controller: Option<String>,
    /// The Di2 shifter's D-Fly channel that shifts up.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=4))]
    pub(crate) up_channel: u8,
    /// The Di2 shifter's D-Fly channel that shifts down.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=4))]
    pub(crate) down_channel: u8,
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

impl DeviceArgs {
    /// Whether a heart rate is asked for: from a strap, or the fake rider's simulated heart.
    pub(crate) fn heart_rate(&self) -> bool {
        self.fake || self.hr.is_some()
    }
}

/// The connected trainer and optional heart-rate sensor and controller.
pub(crate) struct Devices {
    pub(crate) trainer: DeviceHandle,
    pub(crate) sensor: Option<DeviceHandle>,
    pub(crate) controller: Option<DeviceHandle>,
}

pub(crate) async fn connect(args: &DeviceArgs) -> Result<Devices> {
    let bluetooth = if !args.fake || args.hr.is_some() || args.controller.is_some() {
        let bluetooth = Bluetooth::new().await?;
        println!("Scanning for {} s…", args.scan_seconds);
        let devices = bluetooth
            .scan(Duration::from_secs(args.scan_seconds))
            .await?;
        Some((bluetooth, devices))
    } else {
        None
    };

    let trainer = match &bluetooth {
        _ if args.fake => fake::spawn(
            FakeRider {
                power: Watts(args.fake_power),
                cadence: Rpm(args.fake_cadence),
                // A real strap's heart rate is the one to use.
                heart: args.hr.is_none().then(FakeHeart::default),
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
    let sensor = match (&bluetooth, &args.hr) {
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
    let controller = match (&bluetooth, &args.controller) {
        (Some((bluetooth, devices)), Some(name)) => {
            match pick(devices, DeviceKind::Controller, name) {
                Ok(device) => Some(bluetooth.connect(device)),
                Err(error) => {
                    eprintln!("Continuing without a controller: {error:#}");
                    None
                }
            }
        }
        _ => None,
    };
    Ok(Devices {
        trainer,
        sensor,
        controller,
    })
}

impl Devices {
    /// Disconnects all devices; a second Ctrl+C skips waiting.
    pub(crate) async fn close(self) {
        println!("Disconnecting… (Ctrl+C again to quit immediately)");
        let close_all = async {
            self.trainer.close(CLOSE_TIMEOUT).await;
            if let Some(sensor) = self.sensor {
                sensor.close(CLOSE_TIMEOUT).await;
            }
            if let Some(controller) = self.controller {
                controller.close(CLOSE_TIMEOUT).await;
            }
        };
        tokio::select! {
            () = close_all => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
}

pub(crate) fn kind_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Trainer => "trainer",
        DeviceKind::HeartRateSensor => "heart rate",
        DeviceKind::Controller => "controller",
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
pub(crate) async fn next_event(device: Option<&mut DeviceHandle>) -> Option<(String, DeviceEvent)> {
    match device {
        Some(device) => {
            let event = device.next_event().await?;
            Some((device.name().to_owned(), event))
        }
        None => std::future::pending().await,
    }
}

/// Reads stdin lines on a plain thread.
///
/// Tokio's stdin reads on a blocking-pool thread that runtime shutdown waits for, so the
/// process could not exit while a read is pending.
pub(crate) fn stdin_lines() -> mpsc::Receiver<String> {
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

/// Formats an optional value right-aligned with its unit, `--` when unknown.
pub(crate) fn field(value: Option<f64>, unit: &str, precision: usize) -> String {
    let text = value.map_or_else(
        || format!("-- {unit}"),
        |v| format!("{v:.precision$} {unit}"),
    );
    format!("{text:>9}")
}
