//! A simulated trainer with a simulated rider, for development and tests without hardware (R10).

use std::time::Duration;

use tokio::time::{MissedTickBehavior, interval};
use torqa_domain::telemetry::{Telemetry, TrainerControl};
use torqa_domain::units::{Rpm, Watts};

use crate::handle::{DeviceEvent, DeviceHandle};

/// How the simulated rider pedals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FakeRider {
    /// Power the rider produces when the trainer does not dictate it.
    pub power: Watts,
    /// Constant cadence.
    pub cadence: Rpm,
}

/// Starts a fake trainer that reports telemetry every `period`.
///
/// Reported power follows the active control: ERG reports the target; resistance scales the
/// rider's power linearly from 50 % (at 0 % resistance) to 150 % (at 100 %); slope simulation
/// and no control report the rider's own power, because grade only changes speed, which is
/// left to the physics model. Speed is not reported.
///
/// Must be called within a Tokio runtime.
#[must_use]
pub fn spawn(rider: FakeRider, period: Duration) -> DeviceHandle {
    DeviceHandle::spawn(
        "Fake trainer".to_owned(),
        true,
        move |mut channels| async move {
            if channels.events.send(DeviceEvent::Connected).await.is_err() {
                return;
            }
            let mut control = None;
            let mut ticker = interval(period);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let telemetry = Telemetry {
                            power: Some(power(rider, control.as_ref())),
                            cadence: Some(rider.cadence),
                            ..Telemetry::default()
                        };
                        if channels.events.send(DeviceEvent::Telemetry(telemetry)).await.is_err() {
                            return;
                        }
                    }
                    next = channels.next_control() => match next {
                        Some(c) => control = Some(c),
                        None => return,
                    },
                }
            }
        },
    )
}

fn power(rider: FakeRider, control: Option<&TrainerControl>) -> Watts {
    match control {
        Some(TrainerControl::TargetPower(target)) => *target,
        Some(TrainerControl::Resistance(share)) => {
            Watts(rider.power.0 * (0.5 + share.0.clamp(0.0, 100.0) / 100.0))
        }
        Some(TrainerControl::Simulation(_)) | None => rider.power,
    }
}

#[cfg(test)]
mod tests {
    use torqa_domain::telemetry::SimulationParameters;
    use torqa_domain::units::Percent;

    use super::*;

    const RIDER: FakeRider = FakeRider {
        power: Watts(200.0),
        cadence: Rpm(90.0),
    };

    async fn next_telemetry(handle: &mut DeviceHandle) -> Telemetry {
        loop {
            match handle.next_event().await {
                Some(DeviceEvent::Telemetry(t)) => return t,
                Some(_) => {}
                None => panic!("fake trainer stopped"),
            }
        }
    }

    /// Lets the driver process queued controls before the next sample is read.
    async fn settle(handle: &mut DeviceHandle) -> Telemetry {
        next_telemetry(handle).await;
        next_telemetry(handle).await
    }

    #[tokio::test(start_paused = true)]
    async fn reports_rider_power_and_cadence_without_control() {
        let mut handle = spawn(RIDER, Duration::from_millis(250));

        assert_eq!(handle.next_event().await, Some(DeviceEvent::Connected));
        let t = next_telemetry(&mut handle).await;
        assert_eq!(t.power, Some(Watts(200.0)));
        assert_eq!(t.cadence, Some(Rpm(90.0)));
    }

    #[tokio::test(start_paused = true)]
    async fn erg_reports_target_power() {
        let mut handle = spawn(RIDER, Duration::from_millis(250));

        handle
            .control(TrainerControl::TargetPower(Watts(275.0)))
            .await
            .unwrap();

        assert_eq!(settle(&mut handle).await.power, Some(Watts(275.0)));
    }

    #[tokio::test(start_paused = true)]
    async fn resistance_scales_rider_power() {
        let mut handle = spawn(RIDER, Duration::from_millis(250));

        handle
            .control(TrainerControl::Resistance(Percent(100.0)))
            .await
            .unwrap();

        assert_eq!(settle(&mut handle).await.power, Some(Watts(300.0)));
    }

    #[tokio::test(start_paused = true)]
    async fn simulation_keeps_rider_power() {
        let mut handle = spawn(RIDER, Duration::from_millis(250));

        handle
            .control(TrainerControl::TargetPower(Watts(275.0)))
            .await
            .unwrap();
        handle
            .control(TrainerControl::Simulation(SimulationParameters::default()))
            .await
            .unwrap();

        assert_eq!(settle(&mut handle).await.power, Some(Watts(200.0)));
    }

    #[tokio::test(start_paused = true)]
    async fn close_stops_the_driver() {
        let handle = spawn(RIDER, Duration::from_millis(250));
        let started = tokio::time::Instant::now();

        handle.close(Duration::from_secs(5)).await;

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "close timed out"
        );
    }
}
