//! Where a stretch ridden elsewhere lies on a route (R302): a segment the rider brings as a GPX
//! track becomes the distances of its start and end along the course.

use orqwa_domain::units::Meters;

use crate::{LocalProjection, Route, SPACING};

/// How far a track point may lie from the route: GPS and map tracks differ by a road's width
/// and some drift.
const REACH_M: f64 = 30.0;
/// Track points closer together than this add nothing to following it.
const TRACK_STEP_M: f64 = 10.0;
/// How far along the route, beyond the track's own step, the next point is looked for.
const LOOK_AHEAD_M: f64 = 60.0;
/// A segment shorter than this is no stretch to time.
const MIN_LENGTH_M: f64 = 50.0;

/// The distances from the route start of the first and last point of `track` (latitude,
/// longitude), if the whole track follows the route forward; the earliest such stretch where
/// the route passes the start more than once. `None` if it leaves the route, runs against it
/// or is too short.
#[must_use]
pub fn locate(route: &Route, track: &[(f64, f64)]) -> Option<(Meters, Meters)> {
    let projection = LocalProjection::for_route(route);
    let road: Vec<(f64, f64)> = route
        .points()
        .iter()
        .map(|p| projection.project(p.lat, p.lon))
        .collect();
    let along: Vec<f64> = route.points().iter().map(|p| p.distance.0).collect();
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(track.len());
    for &(lat, lon) in track {
        let point = projection.project(lat, lon);
        if points
            .last()
            .is_none_or(|last| apart(*last, point) >= TRACK_STEP_M)
        {
            points.push(point);
        }
    }
    if let (Some(&(lat, lon)), Some(&last)) = (track.last(), points.last()) {
        let end = projection.project(lat, lon);
        if end != last {
            points.push(end);
        }
    }
    let (&first, rest) = points.split_first()?;
    if road.len() < 2 || rest.is_empty() {
        return None;
    }
    let near = |i: usize, p: (f64, f64)| apart(road[i], p);
    starts(&road, first).find_map(|start| {
        let mut at = start;
        let mut previous = first;
        for &point in rest {
            let reach = ((apart(previous, point) + LOOK_AHEAD_M) / SPACING).ceil();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few points
            let last = (at + reach as usize).min(road.len() - 1);
            let (best, distance) = (at..=last)
                .map(|i| (i, near(i, point)))
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            if distance > REACH_M {
                return None;
            }
            at = best;
            previous = point;
        }
        let from = on_road(&road, &along, start, first);
        let to = on_road(&road, &along, at, previous);
        (to - from >= MIN_LENGTH_M).then_some((Meters(from), Meters(to)))
    })
}

fn apart(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The route points nearest to `point` where the route passes it, in order along the route.
fn starts(road: &[(f64, f64)], point: (f64, f64)) -> impl Iterator<Item = usize> + '_ {
    let near = move |i: usize| apart(road[i], point);
    (0..road.len()).filter(move |&i| {
        near(i) <= REACH_M
            && (i == 0 || near(i) <= near(i - 1))
            && (i + 1 == road.len() || near(i) < near(i + 1))
    })
}

/// The distance along the road of `point`, projected onto the road beside its point `i`.
fn on_road(road: &[(f64, f64)], along: &[f64], i: usize, point: (f64, f64)) -> f64 {
    let mut best = (apart(road[i], point), along[i]);
    for j in [i.saturating_sub(1), i] {
        let Some(&b) = road.get(j + 1) else { continue };
        let a = road[j];
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy);
        if length <= f64::EPSILON {
            continue;
        }
        let t = (((point.0 - a.0) * dx + (point.1 - a.1) * dy) / (length * length)).clamp(0.0, 1.0);
        let foot = (a.0 + t * dx, a.1 + t * dy);
        let distance = apart(foot, point);
        if distance < best.0 {
            best = (distance, along[j] + t * (along[j + 1] - along[j]));
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Surface, WorldPoint};

    /// A route through these corners (metres east, north), its origin at 46.9° N 7.4° E.
    fn route(corners: &[(f64, f64)]) -> Route {
        let points: Vec<WorldPoint> = corners
            .iter()
            .map(|&(east, north)| WorldPoint {
                east,
                north,
                elevation: 500.0,
                surface: Surface::Ground,
            })
            .collect();
        Route::from_world(None, (46.9, 7.4), &points).unwrap()
    }

    /// The positions of a track along these corners every `step` metres, shifted `offset`
    /// metres north, as latitude and longitude.
    fn track(route: &Route, corners: &[(f64, f64)], step: f64, offset: f64) -> Vec<(f64, f64)> {
        let projection = LocalProjection::for_route(route);
        let mut track = Vec::new();
        for pair in corners.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = apart(a, b);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small
            let steps = (length / step).ceil() as usize;
            for k in 0..steps {
                #[allow(clippy::cast_precision_loss)] // small
                let t = k as f64 / steps as f64;
                let (east, north) = (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
                track.push(projection.unproject(east, north + offset));
            }
        }
        let end = corners[corners.len() - 1];
        track.push(projection.unproject(end.0, end.1 + offset));
        track
    }

    fn near(value: Meters, expected: f64) -> bool {
        (value.0 - expected).abs() < 2.0
    }

    #[test]
    fn a_track_along_the_road_lies_between_its_ends() {
        let road = route(&[(0.0, 0.0), (2000.0, 0.0)]);
        let sprint = track(&road, &[(500.0, 0.0), (800.0, 0.0)], 5.0, 8.0);

        let (start, end) = locate(&road, &sprint).unwrap();

        assert!(near(start, 500.0) && near(end, 800.0), "{start:?} {end:?}");
    }

    #[test]
    fn sparse_track_points_still_follow_the_road() {
        let road = route(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0)]);
        let climb = track(
            &road,
            &[(600.0, 0.0), (1000.0, 0.0), (1000.0, 700.0)],
            150.0,
            0.0,
        );

        let (start, end) = locate(&road, &climb).unwrap();

        assert!(near(start, 600.0) && near(end, 1700.0), "{start:?} {end:?}");
    }

    #[test]
    fn a_track_that_leaves_the_road_or_runs_against_it_is_not_on_the_course() {
        let road = route(&[(0.0, 0.0), (2000.0, 0.0)]);
        let elsewhere = track(
            &road,
            &[(500.0, 0.0), (800.0, 0.0), (800.0, 300.0)],
            10.0,
            0.0,
        );
        let backwards = track(&road, &[(800.0, 0.0), (500.0, 0.0)], 10.0, 0.0);
        let beside = track(&road, &[(500.0, 0.0), (800.0, 0.0)], 10.0, 80.0);
        let short = track(&road, &[(500.0, 0.0), (520.0, 0.0)], 5.0, 0.0);

        for stretch in [elsewhere, backwards, beside, short, Vec::new()] {
            assert_eq!(locate(&road, &stretch), None);
        }
    }

    #[test]
    fn on_a_lap_ridden_twice_the_segment_is_where_its_track_goes() {
        // Out east along y = 0, back west along y = 40: the second pass runs the other way.
        let road = route(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, 40.0), (0.0, 40.0)]);
        let home = track(&road, &[(700.0, 40.0), (300.0, 40.0)], 10.0, 0.0);

        let (start, end) = locate(&road, &home).unwrap();

        assert!(
            near(start, 1340.0) && near(end, 1740.0),
            "{start:?} {end:?}"
        );
    }

    #[test]
    fn on_two_laps_the_same_way_the_first_pass_counts() {
        let lap = [(0.0, 0.0), (500.0, 0.0), (500.0, 500.0), (0.0, 500.0)];
        let mut corners = lap.to_vec();
        corners.extend(lap);
        corners.push((0.0, 0.0));
        let road = route(&corners);
        let sprint = track(&road, &[(100.0, 0.0), (400.0, 0.0)], 10.0, 0.0);

        let (start, end) = locate(&road, &sprint).unwrap();

        assert!(near(start, 100.0) && near(end, 400.0), "{start:?} {end:?}");
    }
}
