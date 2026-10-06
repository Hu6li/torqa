//! Data prepared for drawing: route elevation profile, a flat map of the track and charts of
//! recorded rides.

use torqa_domain::recording::Sample;
use torqa_routes::{LocalProjection, Route};

/// A recorded value over the ride as `(elapsed seconds, value)`, averaged into at most
/// `max_points` equal time buckets so long rides stay cheap to draw. Buckets without values
/// (e.g. no heart-rate strap) are left out.
#[must_use]
pub fn ride_series(
    samples: &[Sample],
    max_points: usize,
    value: impl Fn(&Sample) -> Option<f64>,
) -> Vec<(f64, f64)> {
    let per_bucket = samples.len().div_ceil(max_points.max(1)).max(1);
    samples
        .chunks(per_bucket)
        .filter_map(|bucket| {
            let values: Vec<f64> = bucket.iter().filter_map(&value).collect();
            if values.is_empty() {
                return None;
            }
            #[allow(clippy::cast_precision_loss)] // bucket sizes are small
            let count = values.len() as f64;
            let middle = &bucket[bucket.len() / 2];
            Some((
                middle.elapsed.as_secs_f64(),
                values.iter().sum::<f64>() / count,
            ))
        })
        .collect()
}

/// `(distance, elevation)` in metres, at most `max_points` evenly picked points including the
/// finish.
#[must_use]
pub fn elevation_profile(route: &Route, max_points: usize) -> Vec<(f64, f64)> {
    pick(route, max_points, |p| (p.distance.0, p.elevation.0))
}

/// The track in metres east/north of the start, at most `max_points` points.
#[must_use]
pub fn track(route: &Route, max_points: usize) -> Vec<(f64, f64)> {
    let projection = LocalProjection::for_route(route);
    pick(route, max_points, |p| projection.project(p.lat, p.lon))
}

fn pick<T>(
    route: &Route,
    max_points: usize,
    map: impl Fn(&torqa_routes::RoutePoint) -> T,
) -> Vec<T> {
    let points = route.points();
    let step = points.len().div_ceil(max_points.max(2)).max(1);
    let mut picked: Vec<T> = points.iter().step_by(step).map(&map).collect();
    if !(points.len() - 1).is_multiple_of(step) {
        picked.push(map(&points[points.len() - 1]));
    }
    picked
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// 2 km due east with a steady climb from 500 m to 600 m.
    async fn route_east() -> Route {
        let mut xml = String::from("<gpx><trk><trkseg>");
        for i in 0..=20 {
            let lon = 7.0 + f64::from(i) * 100.0 / (111_195.0 * 46f64.to_radians().cos());
            let ele = 500.0 + f64::from(i) * 5.0;
            let _ = write!(
                xml,
                r#"<trkpt lat="46" lon="{lon}"><ele>{ele}</ele></trkpt>"#
            );
        }
        xml.push_str("</trkseg></trk></gpx>");
        Route::from_gpx(&xml, None).await.unwrap()
    }

    #[test]
    fn ride_series_averages_buckets_and_skips_gaps() {
        use std::time::Duration;
        use torqa_domain::units::{Meters, MetersPerSecond, Watts};

        let samples: Vec<Sample> = (0..10u32)
            .map(|i| Sample {
                elapsed: Duration::from_secs(u64::from(i)),
                location: None,
                distance: Meters(0.0),
                speed: MetersPerSecond(0.0),
                power: (i < 6).then(|| Watts(f64::from(i) * 10.0)),
                cadence: None,
                heart_rate: None,
            })
            .collect();

        let series = ride_series(&samples, 5, |s| s.power.map(|p| p.0));

        // Pairs of seconds: (0, 10) → 5 W, (20, 30) → 25 W, (40, 50) → 45 W; the rest has none.
        assert_eq!(series, [(1.0, 5.0), (3.0, 25.0), (5.0, 45.0)]);
        assert_eq!(
            ride_series(&samples, 5, |s| s.heart_rate.map(|h| h.0)).len(),
            0
        );
    }

    #[tokio::test]
    async fn profile_is_thinned_but_keeps_start_and_finish() {
        let route = route_east().await;

        let profile = elevation_profile(&route, 50);

        assert!(profile.len() <= 51, "{} points", profile.len());
        assert_eq!(profile[0], (0.0, route.points()[0].elevation.0));
        let finish = profile[profile.len() - 1];
        assert!((finish.0 - route.length().0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn track_is_in_metres_east_and_north_of_the_start() {
        let route = route_east().await;

        let track = track(&route, 1000);

        assert_eq!(track[0], (0.0, 0.0));
        let (east, north) = track[track.len() - 1];
        assert!((east - 2000.0).abs() < 10.0, "east {east}");
        assert!(north.abs() < 1.0, "north {north}");
    }
}
