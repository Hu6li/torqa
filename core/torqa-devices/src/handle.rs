//! The driver-independent interface to a running device.

use std::future::Future;
use std::time::Duration;

use crate::di2::ButtonPress;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use torqa_domain::telemetry::{Telemetry, TrainerControl};

/// Something a device reports while it is running.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceEvent {
    /// The device is connected and ready; also sent again after a reconnect.
    Connected,
    /// New measurements.
    Telemetry(Telemetry),
    /// Buttons were pressed on a shifter (Shimano Di2 D-Fly channels, R7).
    Buttons(Vec<ButtonPress>),
    /// The connection was lost; the driver keeps trying to reconnect.
    Disconnected,
}

/// Errors of device drivers.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// The device is a sensor and cannot be controlled.
    #[error("device cannot be controlled")]
    NotControllable,
    /// The driver task has stopped.
    #[error("device driver has stopped")]
    Stopped,
    /// No Bluetooth adapter is available.
    #[error("no Bluetooth adapter found")]
    NoAdapter,
    /// The device lacks a characteristic the driver needs.
    #[error("device lacks characteristic {0:#06x}")]
    MissingCharacteristic(u16),
    /// The device lacks a service the driver needs, by its full UUID.
    #[error("device lacks service {0}")]
    MissingService(&'static str),
    /// The Bluetooth stack reported an error.
    #[error("Bluetooth error: {0}")]
    Bluetooth(#[from] btleplug::Error),
}

/// A running device. Dropping the handle stops its driver and disconnects the device in the
/// background; use [`DeviceHandle::close`] to wait for the disconnect.
#[derive(Debug)]
pub struct DeviceHandle {
    name: String,
    events: mpsc::Receiver<DeviceEvent>,
    control: Option<mpsc::Sender<TrainerControl>>,
    task: JoinHandle<()>,
}

/// The driver side of a [`DeviceHandle`].
pub(crate) struct DriverChannels {
    pub(crate) events: mpsc::Sender<DeviceEvent>,
    pub(crate) control: Option<mpsc::Receiver<TrainerControl>>,
}

const CHANNEL_CAPACITY: usize = 64;

impl DeviceHandle {
    /// Spawns a driver task on the current Tokio runtime and returns its handle.
    pub(crate) fn spawn<F, Fut>(name: String, controllable: bool, driver: F) -> Self
    where
        F: FnOnce(DriverChannels) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (events_tx, events_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (control_tx, control_rx) = if controllable {
            let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let task = tokio::spawn(driver(DriverChannels {
            events: events_tx,
            control: control_rx,
        }));
        Self {
            name,
            events: events_rx,
            control: control_tx,
            task,
        }
    }

    /// Stops the driver and waits until the device is disconnected, at most `timeout`.
    pub async fn close(self, timeout: Duration) {
        let Self {
            events,
            control,
            task,
            ..
        } = self;
        // Closing both channels is what tells the driver to disconnect.
        drop(events);
        drop(control);
        // On timeout the driver keeps disconnecting in the background; nothing else to do.
        let _ = tokio::time::timeout(timeout, task).await;
    }

    /// Human-readable device name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the device accepts [`TrainerControl`]s.
    #[must_use]
    pub fn is_controllable(&self) -> bool {
        self.control.is_some()
    }

    /// Waits for the next event; `None` once the driver has stopped.
    pub async fn next_event(&mut self) -> Option<DeviceEvent> {
        self.events.recv().await
    }

    /// Returns the next event if one is waiting, without blocking (for frame loops).
    ///
    /// # Errors
    /// [`DeviceError::Stopped`] once the driver has stopped and all events were read.
    pub fn try_next_event(&mut self) -> Result<Option<DeviceEvent>, DeviceError> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(DeviceError::Stopped),
        }
    }

    /// Queues a resistance control without blocking (for frame loops); see [`Self::control`].
    ///
    /// # Errors
    /// [`DeviceError::NotControllable`] for sensors, [`DeviceError::Stopped`] if the driver
    /// ended. A full queue is not an error: the control is dropped, as a newer one follows.
    pub fn try_control(&self, control: TrainerControl) -> Result<(), DeviceError> {
        let sender = self.control.as_ref().ok_or(DeviceError::NotControllable)?;
        match sender.try_send(control) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(DeviceError::Stopped),
        }
    }

    /// Asks the trainer to apply a resistance control.
    ///
    /// Controls are applied in order; if several are queued, only the latest is sent. The last
    /// control is re-applied automatically after a reconnect.
    ///
    /// # Errors
    /// [`DeviceError::NotControllable`] for sensors, [`DeviceError::Stopped`] if the driver ended.
    pub async fn control(&self, control: TrainerControl) -> Result<(), DeviceError> {
        let sender = self.control.as_ref().ok_or(DeviceError::NotControllable)?;
        sender.send(control).await.map_err(|_| DeviceError::Stopped)
    }
}

impl DriverChannels {
    /// Waits for the next control, skipping to the newest if several are queued.
    ///
    /// Never completes for sensors, which have no control channel.
    pub(crate) async fn next_control(&mut self) -> Option<TrainerControl> {
        let Some(rx) = self.control.as_mut() else {
            return std::future::pending().await;
        };
        let mut latest = rx.recv().await?;
        while let Ok(newer) = rx.try_recv() {
            latest = newer;
        }
        Some(latest)
    }
}
