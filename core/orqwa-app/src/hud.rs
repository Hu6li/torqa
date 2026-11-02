//! The ride HUD's metrics (R23): what a rider can choose to show, and their live values.

use std::time::Duration;

use orqwa_domain::profile::Profile;
use orqwa_domain::recording::{RideSummary, Sample};
use orqwa_domain::units::{BeatsPerMinute, Watts};
use orqwa_routes::Route;
use orqwa_session::RideState;

/// How a metric's value is shown; front ends convert units and format by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    /// A plain number with a fixed unit.
    Number,
    /// Kilometres per hour (miles per hour in imperial).
    Speed,
    /// Kilometres (miles in imperial).
    Distance,
    /// Metres (feet in imperial).
    Elevation,
    /// Seconds, shown as a clock time.
    Duration,
    /// A gradient in percent, shown with its sign.
    Grade,
    /// A training zone number.
    Zone,
}

/// The training zones a metric's value is read against, for the bar beside it (#432).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zones {
    /// The rider's power zones, set on their FTP.
    Power,
    /// The rider's heart-rate zones, set on their maximum heart rate.
    HeartRate,
}

/// A metric the HUD can show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metric {
    /// Stable identifier, stored in the rider's layout.
    pub id: &'static str,
    /// Caption.
    pub caption: &'static str,
    /// Unit of [`MetricKind::Number`] values (others derive theirs from the kind).
    pub unit: &'static str,
    /// Decimals to show.
    pub decimals: u8,
    /// How to show it.
    pub kind: MetricKind,
    /// A typical value, in the units of [`values`], for previews of a layout.
    pub sample: f64,
    /// The zones its value falls in, and the metric in watts or beats per minute that places
    /// it there: itself, or the live figure it is worked out from (W/kg from the power).
    pub zones: Option<(Zones, &'static str)>,
}

const fn metric(
    id: &'static str,
    caption: &'static str,
    unit: &'static str,
    decimals: u8,
    kind: MetricKind,
    sample: f64,
) -> Metric {
    Metric {
        id,
        caption,
        unit,
        decimals,
        kind,
        sample,
        zones: None,
    }
}

impl Metric {
    /// The metric read against `zones`, placed there by the value of the metric `by`.
    const fn zoned(self, zones: Zones, by: &'static str) -> Self {
        Self {
            zones: Some((zones, by)),
            ..self
        }
    }
}

// i18n-begin: captions and units are translated by the front end.
/// Every metric, in the order offered to the rider.
pub const METRICS: &[Metric] = &[
    metric("power", "Power", "W", 0, MetricKind::Number, 245.0).zoned(Zones::Power, "power"),
    metric("power_3s", "Power 3 s", "W", 0, MetricKind::Number, 252.0)
        .zoned(Zones::Power, "power_3s"),
    metric("power_10s", "Power 10 s", "W", 0, MetricKind::Number, 238.0)
        .zoned(Zones::Power, "power_10s"),
    metric("avg_power", "Avg power", "W", 0, MetricKind::Number, 221.0)
        .zoned(Zones::Power, "avg_power"),
    metric(
        "normalized_power",
        "Normalized power",
        "W",
        0,
        MetricKind::Number,
        236.0,
    )
    .zoned(Zones::Power, "normalized_power"),
    metric("watts_per_kg", "W/kg", "W/kg", 1, MetricKind::Number, 3.3).zoned(Zones::Power, "power"),
    metric(
        "power_percent_ftp",
        "Power % FTP",
        "%",
        0,
        MetricKind::Number,
        86.0,
    )
    .zoned(Zones::Power, "power"),
    metric("power_zone", "Power zone", "", 0, MetricKind::Zone, 3.0).zoned(Zones::Power, "power"),
    metric(
        "heart_rate",
        "Heart rate",
        "bpm",
        0,
        MetricKind::Number,
        148.0,
    )
    .zoned(Zones::HeartRate, "heart_rate"),
    metric(
        "heart_rate_percent_max",
        "Heart rate % max",
        "%",
        0,
        MetricKind::Number,
        80.0,
    )
    .zoned(Zones::HeartRate, "heart_rate"),
    metric(
        "heart_rate_zone",
        "Heart-rate zone",
        "",
        0,
        MetricKind::Zone,
        3.0,
    )
    .zoned(Zones::HeartRate, "heart_rate"),
    metric("cadence", "Cadence", "rpm", 0, MetricKind::Number, 88.0),
    metric("gear", "Gear", "", 0, MetricKind::Number, 12.0),
    metric("speed", "Speed", "", 1, MetricKind::Speed, 31.4),
    metric("avg_speed", "Avg speed", "", 1, MetricKind::Speed, 29.8),
    metric("distance", "Distance", "", 2, MetricKind::Distance, 12.4),
    metric("remaining", "To go", "", 2, MetricKind::Distance, 7.6),
    metric("elapsed", "Time", "", 0, MetricKind::Duration, 1543.0),
    metric(
        "time_remaining",
        "Time to go",
        "",
        0,
        MetricKind::Duration,
        2_710.0,
    ),
    metric(
        "elevation",
        "Elevation",
        "",
        0,
        MetricKind::Elevation,
        612.0,
    ),
    metric(
        "elevation_gain",
        "Climbed",
        "",
        0,
        MetricKind::Elevation,
        284.0,
    ),
    metric(
        "ascent_remaining",
        "Ascent to go",
        "",
        0,
        MetricKind::Elevation,
        356.0,
    ),
    metric("grade", "Grade", "", 1, MetricKind::Grade, 4.2),
    metric(
        "upcoming_grade",
        "Next 500 m",
        "",
        1,
        MetricKind::Grade,
        6.1,
    ),
    metric("intensity", "Intensity", "", 2, MetricKind::Number, 0.86),
    metric("training_stress", "TSS", "", 0, MetricKind::Number, 38.0),
    metric("work", "Work", "kJ", 0, MetricKind::Number, 412.0),
];

// i18n-end

/// The layout before a rider customises it: the first metric is shown large.
pub const DEFAULT_LAYOUT: &[&str] = &[
    "power",
    "speed",
    "grade",
    "heart_rate",
    "cadence",
    "distance",
    "elapsed",
];

/// Most metrics a layout may hold; more would cover the road.
pub const MAX_METRICS: usize = 13;

/// Distance over which the upcoming gradient is averaged.
const LOOK_AHEAD_M: f64 = 500.0;

/// A layout cleaned up: known metrics only, each once, at most [`MAX_METRICS`]; the default
/// if nothing is left.
#[must_use]
pub fn sanitize(layout: &[String]) -> Vec<String> {
    let mut clean: Vec<String> = Vec::new();
    for id in layout {
        if METRICS.iter().any(|m| m.id == id) && !clean.contains(id) {
            clean.push(id.clone());
        }
    }
    clean.truncate(MAX_METRICS);
    if clean.is_empty() {
        DEFAULT_LAYOUT.iter().map(|&id| id.to_owned()).collect()
    } else {
        clean
    }
}

/// Live value of every metric, in metric display units (km/h, km, m, s); `None` where unknown,
/// e.g. heart rate without a strap, or the road without a route. `summary` summarises the
/// samples so far, `time_to_go` is the estimated time to the finish (R66).
#[must_use]
pub fn values(
    state: &RideState,
    samples: &[Sample],
    summary: &RideSummary,
    route: Option<&Route>,
    profile: &Profile,
    time_to_go: Option<Duration>,
) -> Vec<(&'static str, Option<f64>)> {
    let t = state.telemetry;
    let power = t.power;
    let upcoming = route.and_then(|route| {
        let ahead = (state.distance.0 + LOOK_AHEAD_M).min(route.length().0);
        (ahead > state.distance.0).then(|| {
            let here = route.position(state.distance).elevation.0;
            let there = route
                .position(orqwa_domain::units::Meters(ahead))
                .elevation
                .0;
            (there - here) / (ahead - state.distance.0) * 100.0
        })
    });
    METRICS
        .iter()
        .map(|metric| {
            let value = match metric.id {
                "power" => power.map(|p| p.0),
                "power_3s" => recent_power(samples, 3),
                "power_10s" => recent_power(samples, 10),
                "avg_power" => summary.avg_power.map(|p| p.0),
                "normalized_power" => summary.normalized_power.map(|p| p.0),
                "watts_per_kg" => power.map(|p| profile.watts_per_kg(p)),
                "power_percent_ftp" => power.and_then(|p| profile.percent_of_ftp(p)),
                "power_zone" => power.map(|p| f64::from(profile.power_zone(p))),
                "heart_rate" => t.heart_rate.map(|h| h.0),
                "heart_rate_percent_max" => t
                    .heart_rate
                    .and_then(|h| profile.percent_of_max_heart_rate(h)),
                "heart_rate_zone" => t.heart_rate.map(|h| f64::from(profile.heart_rate_zone(h))),
                "cadence" => t.cadence.map(|c| c.0),
                #[allow(clippy::cast_precision_loss)] // a gear number
                "gear" => state.gear.map(|g| g.number as f64),
                "speed" => Some(state.speed.as_kilometers_per_hour()),
                "avg_speed" => Some(summary.avg_speed.as_kilometers_per_hour()),
                "distance" => Some(state.distance.0 / 1000.0),
                "remaining" => state.remaining.map(|r| r.0 / 1000.0),
                "elapsed" => Some(state.elapsed.as_secs_f64()),
                "time_remaining" => time_to_go.map(|t| t.as_secs_f64()),
                "elevation" => state.position.map(|p| p.elevation.0),
                "elevation_gain" => route.map(|_| summary.elevation_gain.0),
                "ascent_remaining" => route.map(|r| r.ascent_ahead(state.distance).0),
                "grade" => state.position.map(|p| p.grade.0),
                "upcoming_grade" => upcoming,
                "intensity" => summary.intensity_factor,
                "training_stress" => summary.training_stress,
                "work" => summary.work.map(|w| w.0 / 1000.0),
                _ => None,
            };
            (metric.id, value)
        })
        .collect()
}

/// The zone of each metric's value (#432) among `values`, as [`values`] gives them or as
/// [`samples`] are: power figures in the rider's power zones by their own value — the 3 s
/// average by the 3 s average, the ride's average by the average — and W/kg, % of FTP and the
/// zone figure by the power they come from; heart-rate figures likewise. `None` for metrics
/// without zones, without a value, or for a rider without the FTP or maximum heart rate the
/// zones are set on.
#[must_use]
pub fn zones(
    values: &[(&'static str, Option<f64>)],
    profile: &Profile,
) -> Vec<(&'static str, Option<u8>)> {
    let value = |id: &str| values.iter().find(|(v, _)| *v == id).and_then(|(_, v)| *v);
    METRICS
        .iter()
        .map(|metric| {
            let zone = metric.zones.and_then(|(zones, by)| {
                let value = value(by)?;
                match zones {
                    Zones::Power => (profile.ftp.0 > 0.0).then(|| profile.power_zone(Watts(value))),
                    Zones::HeartRate => (profile.max_heart_rate.0 > 0.0)
                        .then(|| profile.heart_rate_zone(BeatsPerMinute(value))),
                }
            });
            (metric.id, zone)
        })
        .collect()
}

/// Every metric's typical value, for previews of a layout.
#[must_use]
pub fn samples() -> Vec<(&'static str, Option<f64>)> {
    METRICS.iter().map(|m| (m.id, Some(m.sample))).collect()
}

/// The zones of the [`samples`], for a rider the samples describe: the sample power is the
/// sample share of their FTP, the heart rate that of their maximum.
#[must_use]
pub fn sample_zones() -> Vec<(&'static str, Option<u8>)> {
    let sample = |id: &str| {
        METRICS
            .iter()
            .find(|m| m.id == id)
            .map_or(0.0, |m| m.sample)
    };
    let rider = Profile {
        ftp: Watts(sample("power") / sample("power_percent_ftp") * 100.0),
        max_heart_rate: BeatsPerMinute(
            sample("heart_rate") / sample("heart_rate_percent_max") * 100.0,
        ),
        ..Profile::default()
    };
    zones(&samples(), &rider)
}

/// Average power over the last `seconds` samples that have power.
fn recent_power(samples: &[Sample], seconds: usize) -> Option<f64> {
    let recent: Vec<f64> = samples
        .iter()
        .rev()
        .take(seconds)
        .filter_map(|s| s.power.map(|p| p.0))
        .collect();
    #[allow(clippy::cast_precision_loss)] // at most a few samples
    let count = recent.len() as f64;
    (!recent.is_empty()).then(|| recent.iter().sum::<f64>() / count)
}

#[cfg(test)]
mod tests {
    use orqwa_domain::telemetry::Telemetry;
    use orqwa_domain::units::{BeatsPerMinute, Meters, MetersPerSecond, Watts};

    use super::*;

    #[test]
    fn layouts_keep_known_metrics_once_in_order() {
        let layout: Vec<String> = ["speed", "bogus", "power_3s", "speed"]
            .iter()
            .map(|&s| s.to_owned())
            .collect();

        assert_eq!(sanitize(&layout), ["speed", "power_3s"]);
        assert_eq!(sanitize(&[]), DEFAULT_LAYOUT);
        let everything: Vec<String> = METRICS.iter().map(|m| m.id.to_owned()).collect();
        assert_eq!(sanitize(&everything).len(), MAX_METRICS);
    }

    #[test]
    fn every_metric_has_a_unique_id_and_the_default_layout_is_valid() {
        for (i, metric) in METRICS.iter().enumerate() {
            assert!(
                METRICS[..i].iter().all(|m| m.id != metric.id),
                "{}",
                metric.id
            );
        }
        let default: Vec<String> = DEFAULT_LAYOUT.iter().map(|&s| s.to_owned()).collect();
        assert_eq!(sanitize(&default), default);
    }

    #[test]
    fn percentages_relate_power_to_ftp_and_heart_rate_to_the_maximum() {
        let profile = Profile {
            ftp: Watts(250.0),
            max_heart_rate: BeatsPerMinute(190.0),
            ..Profile::default()
        };
        let mut state = RideState {
            elapsed: Duration::ZERO,
            distance: Meters(0.0),
            remaining: None,
            speed: MetersPerSecond(0.0),
            position: None,
            telemetry: Telemetry {
                power: Some(Watts(275.0)),
                heart_rate: Some(BeatsPerMinute(171.0)),
                ..Telemetry::default()
            },
            workout: None,
            gear: None,
        };
        let value = |state: &RideState, id: &str| {
            values(state, &[], &RideSummary::default(), None, &profile, None)
                .into_iter()
                .find(|&(metric, _)| metric == id)
                .and_then(|(_, value)| value)
        };

        assert!((value(&state, "power_percent_ftp").unwrap() - 110.0).abs() < 1e-9);
        assert!((value(&state, "heart_rate_percent_max").unwrap() - 90.0).abs() < 1e-9);
        // Without a strap or a power reading there is nothing to relate.
        state.telemetry = Telemetry::default();
        assert_eq!(value(&state, "power_percent_ftp"), None);
        assert_eq!(value(&state, "heart_rate_percent_max"), None);
    }

    fn power_samples(watts: &[f64]) -> Vec<Sample> {
        watts
            .iter()
            .enumerate()
            .map(|(i, &watts)| Sample {
                elapsed: Duration::from_secs(i as u64),
                location: None,
                distance: Meters(0.0),
                speed: MetersPerSecond(0.0),
                power: Some(Watts(watts)),
                cadence: None,
                heart_rate: None,
            })
            .collect()
    }

    fn riding(power: f64, heart_rate: f64) -> RideState {
        RideState {
            elapsed: Duration::ZERO,
            distance: Meters(0.0),
            remaining: None,
            speed: MetersPerSecond(0.0),
            position: None,
            telemetry: Telemetry {
                power: Some(Watts(power)),
                heart_rate: Some(BeatsPerMinute(heart_rate)),
                ..Telemetry::default()
            },
            workout: None,
            gear: None,
        }
    }

    fn zone_of(zones: &[(&str, Option<u8>)], id: &str) -> Option<u8> {
        zones
            .iter()
            .find(|&&(metric, _)| metric == id)
            .and_then(|&(_, zone)| zone)
    }

    #[test]
    fn each_figure_shows_the_zone_of_its_own_value() {
        let profile = Profile {
            ftp: Watts(250.0),
            max_heart_rate: BeatsPerMinute(190.0),
            ..Profile::default()
        };
        // A sprint just now (VO2max) after easy pedalling (recovery), averaging tempo.
        let state = riding(275.0, 171.0);
        let samples = power_samples(&[100.0, 100.0, 100.0]);
        let summary = RideSummary {
            avg_power: Some(Watts(200.0)),
            ..RideSummary::default()
        };
        let values = values(&state, &samples, &summary, None, &profile, None);
        let zones = zones(&values, &profile);

        for id in ["power", "watts_per_kg", "power_percent_ftp", "power_zone"] {
            assert_eq!(zone_of(&zones, id), Some(5), "{id} by the live power");
        }
        assert_eq!(
            zone_of(&zones, "power_3s"),
            Some(1),
            "the 3 s average by itself"
        );
        assert_eq!(
            zone_of(&zones, "avg_power"),
            Some(3),
            "the ride's average by itself"
        );
        // 90 % of the maximum is the top of zone 4, bounds being inclusive.
        for id in ["heart_rate", "heart_rate_percent_max", "heart_rate_zone"] {
            assert_eq!(zone_of(&zones, id), Some(4), "{id} by the heart rate");
        }
        assert_eq!(
            zone_of(&zones, "normalized_power"),
            None,
            "no value yet, no zone"
        );
        assert_eq!(zone_of(&zones, "speed"), None, "speed has no zones");
    }

    #[test]
    fn no_zones_without_the_figure_they_are_set_on() {
        let profile = Profile {
            ftp: Watts(0.0),
            max_heart_rate: BeatsPerMinute(0.0),
            ..Profile::default()
        };
        let state = riding(275.0, 171.0);
        let values = values(&state, &[], &RideSummary::default(), None, &profile, None);
        let zones = zones(&values, &profile);

        assert!(zones.iter().all(|&(_, zone)| zone.is_none()), "{zones:?}");
    }

    #[test]
    fn zoned_metrics_are_placed_by_watts_or_beats_per_minute() {
        for metric in METRICS {
            let Some((zones, by)) = metric.zones else {
                continue;
            };
            let source = METRICS.iter().find(|m| m.id == by);
            let unit = match zones {
                Zones::Power => "W",
                Zones::HeartRate => "bpm",
            };
            assert_eq!(source.map(|m| m.unit), Some(unit), "{} by {by}", metric.id);
        }
    }

    #[test]
    fn the_preview_zones_agree_with_the_sample_zone_figures() {
        let zones = sample_zones();
        let sample = |id: &str| METRICS.iter().find(|m| m.id == id).map(|m| m.sample);

        for id in ["power_zone", "heart_rate_zone"] {
            assert_eq!(zone_of(&zones, id).map(f64::from), sample(id), "{id}");
        }
    }

    #[test]
    fn short_term_power_averages_the_latest_seconds() {
        let samples = power_samples(&[100.0, 100.0, 200.0, 300.0, 400.0]);

        assert_eq!(recent_power(&samples, 3), Some(300.0));
        assert_eq!(recent_power(&samples, 10), Some(220.0));
        assert_eq!(recent_power(&[], 3), None);
    }
}
