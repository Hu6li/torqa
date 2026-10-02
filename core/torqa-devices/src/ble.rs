//! Bluetooth LE discovery and drivers for FTMS trainers and heart-rate sensors.

use std::time::Duration;

use btleplug::api::bleuuid::uuid_from_u16;
use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter,
    ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use tokio::time::sleep;
use torqa_domain::telemetry::{Telemetry, TrainerControl};
use tracing::{debug, info, warn};

use crate::ftms::{self, ControlResult, ResistanceRange};
use crate::handle::{DeviceError, DeviceEvent, DeviceHandle, DriverChannels};
use crate::heart_rate;

const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// What a discovered device can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// A controllable trainer (FTMS).
    Trainer,
    /// A heart-rate sensor.
    HeartRateSensor,
}

/// A device found by [`Bluetooth::scan`].
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    /// Advertised name.
    pub name: String,
    /// Device capability.
    pub kind: DeviceKind,
    /// Signal strength in dBm at discovery, if known.
    pub rssi: Option<i16>,
    peripheral: Peripheral,
}

/// Access to the system's Bluetooth adapter.
#[derive(Clone)]
pub struct Bluetooth {
    adapter: Adapter,
}

impl Bluetooth {
    /// Opens the first Bluetooth adapter.
    ///
    /// # Errors
    /// [`DeviceError::NoAdapter`] if there is none, or the Bluetooth stack's error.
    pub async fn new() -> Result<Self, DeviceError> {
        let manager = Manager::new().await?;
        let adapter = manager
            .adapters()
            .await?
            .into_iter()
            .next()
            .ok_or(DeviceError::NoAdapter)?;
        Ok(Self { adapter })
    }

    /// Scans for trainers and heart-rate sensors for `duration`.
    ///
    /// # Errors
    /// Returns the Bluetooth stack's error.
    pub async fn scan(&self, duration: Duration) -> Result<Vec<DiscoveredDevice>, DeviceError> {
        let filter = ScanFilter {
            services: vec![
                uuid_from_u16(ftms::SERVICE),
                uuid_from_u16(heart_rate::SERVICE),
            ],
        };
        self.adapter.start_scan(filter).await?;
        sleep(duration).await;
        self.adapter.stop_scan().await?;

        let mut devices = Vec::new();
        for peripheral in self.adapter.peripherals().await? {
            let Some(properties) = peripheral.properties().await? else {
                continue;
            };
            // Scan filters are advisory on some platforms, so check the services again.
            let kind = if properties.services.contains(&uuid_from_u16(ftms::SERVICE)) {
                DeviceKind::Trainer
            } else if properties
                .services
                .contains(&uuid_from_u16(heart_rate::SERVICE))
            {
                DeviceKind::HeartRateSensor
            } else {
                continue;
            };
            let name = properties
                .local_name
                .or(properties.advertisement_name)
                .unwrap_or_else(|| "Unnamed device".to_owned());
            devices.push(DiscoveredDevice {
                name,
                kind,
                rssi: properties.rssi,
                peripheral,
            });
        }
        devices.sort_by_key(|d| std::cmp::Reverse(d.rssi));
        Ok(devices)
    }

    /// Connects to a discovered device and keeps it connected until the handle is dropped.
    ///
    /// Must be called within a Tokio runtime.
    #[must_use]
    pub fn connect(&self, device: DiscoveredDevice) -> DeviceHandle {
        let controllable = device.kind == DeviceKind::Trainer;
        let (handle, channels) = DeviceHandle::new(device.name.clone(), controllable);
        let driver = Driver {
            adapter: self.adapter.clone(),
            device,
            channels,
            last_control: None,
            resistance_range: ResistanceRange::default(),
        };
        tokio::spawn(driver.run());
        handle
    }
}

enum SessionEnd {
    HandleDropped,
    Disconnected,
}

struct Driver {
    adapter: Adapter,
    device: DiscoveredDevice,
    channels: DriverChannels,
    last_control: Option<TrainerControl>,
    resistance_range: ResistanceRange,
}

impl Driver {
    async fn run(mut self) {
        loop {
            match self.session().await {
                Ok(SessionEnd::HandleDropped) => break,
                Ok(SessionEnd::Disconnected) => info!(device = %self.device.name, "disconnected"),
                Err(error) => warn!(device = %self.device.name, %error, "connection failed"),
            }
            if self
                .channels
                .events
                .send(DeviceEvent::Disconnected)
                .await
                .is_err()
            {
                break;
            }
            tokio::select! {
                () = sleep(RECONNECT_DELAY) => {}
                () = self.channels.events.closed() => break,
            }
        }
        if let Err(error) = self.device.peripheral.disconnect().await {
            debug!(device = %self.device.name, %error, "disconnect failed");
        }
    }

    /// One connection, from connecting until it drops or the handle goes away.
    async fn session(&mut self) -> Result<SessionEnd, DeviceError> {
        let peripheral = self.device.peripheral.clone();
        let mut central_events = self.adapter.events().await?;

        info!(device = %self.device.name, "connecting");
        peripheral.connect().await?;
        peripheral.discover_services().await?;
        let mut notifications = peripheral.notifications().await?;
        // A separate sender, so waiting for closure does not borrow `channels` during `next_control`.
        let events = self.channels.events.clone();

        let control_point = match self.device.kind {
            DeviceKind::Trainer => Some(self.set_up_trainer(&peripheral).await?),
            DeviceKind::HeartRateSensor => {
                let measurement = characteristic(&peripheral, heart_rate::MEASUREMENT)?;
                peripheral.subscribe(&measurement).await?;
                None
            }
        };
        info!(device = %self.device.name, "connected");
        if events.send(DeviceEvent::Connected).await.is_err() {
            return Ok(SessionEnd::HandleDropped);
        }

        loop {
            tokio::select! {
                notification = notifications.next() => {
                    let Some(notification) = notification else {
                        return Ok(SessionEnd::Disconnected);
                    };
                    if let Some(telemetry) = self.decode(&notification)
                        && events.send(DeviceEvent::Telemetry(telemetry)).await.is_err()
                    {
                        return Ok(SessionEnd::HandleDropped);
                    }
                }
                event = central_events.next() => match event {
                    Some(CentralEvent::DeviceDisconnected(id)) if id == peripheral.id() => {
                        return Ok(SessionEnd::Disconnected);
                    }
                    Some(_) => {}
                    None => return Ok(SessionEnd::Disconnected),
                },
                control = self.channels.next_control() => {
                    let Some(control) = control else {
                        return Ok(SessionEnd::HandleDropped);
                    };
                    self.last_control = Some(control);
                    if let Some(control_point) = &control_point {
                        let command = ftms::encode_control(&control, self.resistance_range);
                        peripheral.write(control_point, &command, WriteType::WithResponse).await?;
                    }
                }
                () = events.closed() => return Ok(SessionEnd::HandleDropped),
            }
        }
    }

    /// Subscribes to trainer data, takes control and restores the last control after a reconnect.
    async fn set_up_trainer(
        &mut self,
        peripheral: &Peripheral,
    ) -> Result<Characteristic, DeviceError> {
        let data = characteristic(peripheral, ftms::INDOOR_BIKE_DATA)?;
        let control_point = characteristic(peripheral, ftms::CONTROL_POINT)?;

        if let Ok(range) = characteristic(peripheral, ftms::SUPPORTED_RESISTANCE_LEVEL_RANGE) {
            match ftms::ResistanceRange::parse(&peripheral.read(&range).await?) {
                Ok(parsed) => self.resistance_range = parsed,
                Err(error) => warn!(device = %self.device.name, %error, "invalid resistance range"),
            }
        }
        debug!(device = %self.device.name, range = ?self.resistance_range, "resistance range");

        peripheral.subscribe(&data).await?;
        peripheral.subscribe(&control_point).await?;
        peripheral
            .write(
                &control_point,
                &ftms::request_control(),
                WriteType::WithResponse,
            )
            .await?;
        peripheral
            .write(
                &control_point,
                &ftms::start_or_resume(),
                WriteType::WithResponse,
            )
            .await?;
        if let Some(control) = &self.last_control {
            let command = ftms::encode_control(control, self.resistance_range);
            peripheral
                .write(&control_point, &command, WriteType::WithResponse)
                .await?;
        }
        Ok(control_point)
    }

    fn decode(&self, notification: &ValueNotification) -> Option<Telemetry> {
        let name = &self.device.name;
        let (uuid, value) = (notification.uuid, notification.value.as_slice());
        if uuid == uuid_from_u16(ftms::INDOOR_BIKE_DATA) {
            ftms::parse_indoor_bike_data(value)
                .inspect_err(|error| warn!(device = %name, %error, "invalid indoor bike data"))
                .ok()
        } else if uuid == uuid_from_u16(heart_rate::MEASUREMENT) {
            let heart_rate = heart_rate::parse_measurement(value)
                .inspect_err(|error| warn!(device = %name, %error, "invalid heart rate"))
                .ok()?;
            Some(Telemetry {
                heart_rate,
                ..Telemetry::default()
            })
        } else {
            if uuid == uuid_from_u16(ftms::CONTROL_POINT) {
                match ftms::parse_control_response(value) {
                    Some(r) if r.result == ControlResult::Success => {
                        debug!(device = %name, request = r.request, "control accepted");
                    }
                    Some(r) => {
                        warn!(device = %name, request = r.request, result = ?r.result, "control rejected");
                    }
                    None => {}
                }
            }
            None
        }
    }
}

fn characteristic(peripheral: &Peripheral, uuid: u16) -> Result<Characteristic, DeviceError> {
    let wanted = uuid_from_u16(uuid);
    peripheral
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == wanted)
        .ok_or(DeviceError::MissingCharacteristic(uuid))
}
