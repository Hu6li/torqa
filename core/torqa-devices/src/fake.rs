//! A simulated trainer with a simulated rider, for development and tests without hardware (R10).

use std::time::Duration;

use tokio::time::{MissedTickBehavior, interval};
use torqa_domain::telemetry::{Telemetry, TrainerControl};
use torqa_domain::units::{BeatsPerMinute, Rpm, Watts};

use crate::handle::{DeviceEvent, DeviceHandle};

/// How the simulated rider pedals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FakeRider {
    /// Power the rider produces when the trainer does not dictate it.
    pub power: Watts,
    /// Constant cadence.
    pub cadence: Rpm,
    /// The rider's heart, whose rate the trainer reports; `None` reports no heart rate.
    pub heart: Option<FakeHeart>,
}

/// How a simulated rider's heart rate follows their power: it settles at a rate that rises
/// with power, lags behind changes of power, and creeps up the longer the ride goes on
/// (cardiac drift). Enough to try heart-rate workouts (R56) without a strap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FakeHeart {
    /// Heart rate at rest.
    pub resting: BeatsPerMinute,
    /// How much the settled heart rate rises per watt, in beats per minute.
    pub per_watt: f64,
    /// Time constant of the lag: after a change of power, the heart rate has made about
    /// two thirds of its change after this long.
    pub lag: Duration,
    /// How much the settled heart rate rises per hour of riding.
    pub drift_per_hour: BeatsPerMinute,
}

impl Default for FakeHeart {
    /// A rider with a 185 bpm maximum and a 200 W FTP, at about 90 % of the maximum there.
    fn default() -> Self {
        Self {
            resting: BeatsPerMinute(65.0),
            per_watt: 0.5,
            lag: Duration::from_secs(40),
            drift_per_hour: BeatsPerMinute(6.0),
        }
    }
}

/// A [`FakeHeart`] beating along a ride.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedHeart {
    heart: FakeHeart,
    rate: f64,
    ridden: Duration,
}

impl SimulatedHeart {
    /// A heart at rest.
    #[must_use]
    pub fn new(heart: FakeHeart) -> Self {
        Self {
            heart,
            rate: heart.resting.0,
            ridden: Duration::ZERO,
        }
    }

    /// Advances by `dt` at `power`; returns the heart rate in whole beats, as straps report it.
    pub fn step(&mut self, power: Watts, dt: Duration) -> BeatsPerMinute {
        self.ridden += dt;
        let heart = self.heart;
        let settled = heart.resting.0
            + heart.per_watt * power.0.max(0.0)
            + heart.drift_per_hour.0 * self.ridden.as_secs_f64() / 3600.0;
        // Exact for a first-order lag at constant power, whatever the step.
        let follow = 1.0 - (-dt.as_secs_f64() / heart.lag.as_secs_f64().max(1e-3)).exp();
        self.rate += (settled - self.rate) * follow;
        BeatsPerMinute(self.rate.round())
    }
}

/// Starts a fake trainer that reports telemetry every `period`.
///
/// Reported power follows the active control: ERG reports the target; resistance scales the
/// rider's power linearly from 50 % (at 0 % resistance) to 150 % (at 100 %); slope simulation
/// and no control report the rider's own power, because grade only changes speed, which is
/// left to the physics model. Speed is not reported. Heart rate follows the reported power
/// if the rider has a [`FakeHeart`].
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
            let mut heart = rider.heart.map(SimulatedHeart::new);
            let mut ticker = interval(period);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let power = power(rider, control.as_ref());
                        let telemetry = Telemetry {
                            power: Some(power),
                            cadence: Some(rider.cadence),
                            heart_rate: heart.as_mut().map(|h| h.step(power, period)),
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
        heart: None,
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
    async fn heart_rate_follows_the_power_with_a_lag() {
        let heart = FakeHeart::default();
        let rider = FakeRider {
            heart: Some(heart),
            ..RIDER
        };
        let mut handle = spawn(rider, Duration::from_millis(250));
        let settled = heart.resting.0 + heart.per_watt * 200.0;

        let first = next_telemetry(&mut handle).await.heart_rate.unwrap();
        let mut after_one_lag = first;
        for _ in 0..(heart.lag.as_millis() / 250) {
            after_one_lag = next_telemetry(&mut handle).await.heart_rate.unwrap();
        }
        let mut after_five_minutes = after_one_lag;
        for _ in 0..(300_000 / 250) {
            after_five_minutes = next_telemetry(&mut handle).await.heart_rate.unwrap();
        }

        assert!(
            first.0 < heart.resting.0 + 5.0,
            "starts near rest: {first:?}"
        );
        // About two thirds of the way after one time constant.
        let share = (after_one_lag.0 - heart.resting.0) / (settled - heart.resting.0);
        assert!((0.55..0.75).contains(&share), "{share}");
        assert!(
            (after_five_minutes.0 - settled).abs() < 3.0,
            "{after_five_minutes:?}"
        );
    }

    #[test]
    fn heart_rate_drifts_up_at_constant_power() {
        let heart = FakeHeart::default();
        let mut simulated = SimulatedHeart::new(heart);
        let mut after = |minutes: u64| {
            let mut rate = BeatsPerMinute(0.0);
            for _ in 0..minutes * 60 {
                rate = simulated.step(Watts(150.0), Duration::from_secs(1));
            }
            rate
        };

        let at_ten = after(10);
        let at_seventy = after(60);

        let drift = at_seventy.0 - at_ten.0;
        assert!((drift - heart.drift_per_hour.0).abs() < 1.5, "{drift} bpm");
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
