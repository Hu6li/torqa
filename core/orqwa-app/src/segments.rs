//! The rider's own segments of a course (R302): stretches such as a sprint or a favourite climb
//! that they mark on the course or bring as a GPX track. Like a climb, a segment is timed on
//! each ride that covers all of it, and its best time is the one to beat.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use orqwa_domain::units::Meters;
use orqwa_routes::Route;
use orqwa_session::analysis::{effort, time_at};
use orqwa_storage::rides::{RideRecord, SegmentTime};
use orqwa_storage::segments::{self, Placement, Segment, SegmentKey};

use crate::failure::{Failure, Kind};
use crate::moments::Moment;
use crate::{ActiveRide, App, AppError};

/// How far ahead of a segment the ride screen starts showing it, so the rider can get ready.
const HERALD: Meters = Meters(500.0);
/// How long before a segment's start, at the rider's speed, the ride tells them it is coming
/// (R303): time to shift and wind up whatever the speed, where a fixed distance would come a
/// minute early on a climb and late on a fast flat road…
const CALL_AHEAD: Duration = Duration::from_secs(15);
/// …but no later than this far before it, for a rider going slowly or standing still.
const LAST_CALL: Meters = Meters(50.0);

/// One of the rider's segments with their best time on it before this ride.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentBest {
    /// The segment.
    pub segment: Segment,
    /// The rider's fastest time on it, if they have ridden it.
    pub best: Option<Duration>,
    /// Its ends were set or checked on the course as it is prepared now. One that is not, as
    /// when a newer ORQWA prepares the course differently, may lie elsewhere along it: it is
    /// not timed until the rider checks its ends.
    pub confirmed: bool,
}

/// The segment the rider is on or coming up to.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentProgress {
    /// The segment.
    pub segment: Segment,
    /// How far its start still is; `None` once the rider is on it.
    pub ahead: Option<Meters>,
    /// Distance ridden on it so far.
    pub ridden: Meters,
    /// Time on it so far.
    pub elapsed: Duration,
    /// The rider's best time on it before this ride.
    pub best: Option<Duration>,
}

/// Where the segment ends lie in a ride's samples, which count from where the ride entered.
fn in_ride(segment: &Segment, entry: Meters) -> (Meters, Meters) {
    (
        Meters(segment.start_m - entry.0),
        Meters(segment.end_m - entry.0),
    )
}

fn probe(segment: &Segment) -> SegmentTime {
    SegmentTime {
        id: segment.id.clone(),
        name: segment.name.clone(),
        revision: segment.revision.clone(),
        start: Meters(segment.start_m),
        end: Meters(segment.end_m),
        elapsed: Duration::ZERO,
        avg_power: None,
    }
}

/// The fastest time on each stretch among `records`, in one pass: by its key, each of the
/// keys a segment of `definitions` lay on sharing the best of them all. Simulated and assisted
/// rides keep their times but set no records.
pub(crate) fn best_by_key<'a>(
    records: impl IntoIterator<Item = &'a RideRecord>,
    definitions: &[Segment],
) -> HashMap<SegmentKey, Duration> {
    let mut best: HashMap<SegmentKey, Duration> = HashMap::new();
    for time in records
        .into_iter()
        .filter(|r| !r.assisted && !r.simulated)
        .flat_map(|r| r.segments.iter())
    {
        best.entry(time.key())
            .and_modify(|b| *b = (*b).min(time.elapsed))
            .or_insert(time.elapsed);
    }
    for segment in definitions {
        let keys: Vec<SegmentKey> = segment.keys().collect();
        if let Some(fastest) = keys.iter().filter_map(|k| best.get(k)).min().copied() {
            for key in keys {
                best.insert(key, fastest);
            }
        }
    }
    best
}

/// Each of `segments` with the rider's best time on it, from [`best_by_key`]; confirmed if its
/// ends were set on the route's `revision` as prepared now.
pub(crate) fn bests(
    segments: Vec<Segment>,
    revision: &str,
    best: &HashMap<SegmentKey, Duration>,
) -> Vec<SegmentBest> {
    segments
        .into_iter()
        .map(|segment| SegmentBest {
            best: best.get(&segment.key()).copied(),
            confirmed: segment.revision == revision,
            segment,
        })
        .collect()
}

/// For each segment of a ride entering at `entry`, whether it is done before the ride starts:
/// one that starts behind the rider is not timed.
pub(crate) fn passed(records: &[SegmentBest], entry: Meters) -> Vec<bool> {
    records
        .iter()
        .map(|s| s.segment.start_m < entry.0)
        .collect()
}

/// The segments the rider has completed since the last look, as moments; `real` false (a
/// simulated or assisted ride) sets no records. Their times are kept for the saved ride as they
/// come: timed again from the samples at the end, a segment jumped into would be bridged.
pub(crate) fn timed(active: &mut ActiveRide, real: bool) -> Vec<Moment> {
    let entry = active.ride.entry_distance();
    let since_jump = active
        .ride
        .samples()
        .get(active.segments_from..)
        .unwrap_or_default();
    let mut moments = Vec::new();
    for (record, done) in active
        .records
        .segments
        .iter()
        .zip(&mut active.segments_done)
    {
        if *done {
            continue;
        }
        let (start, end) = in_ride(&record.segment, entry);
        let Some(effort) = effort(since_jump, start, end) else {
            continue;
        };
        *done = true;
        active.segment_times.push(SegmentTime {
            elapsed: effort.elapsed,
            avg_power: effort.avg_power,
            ..probe(&record.segment)
        });
        let name = record.segment.name.clone();
        let elapsed = effort.elapsed;
        moments.push(match record.best.filter(|best| real && elapsed < *best) {
            Some(best) => Moment::SegmentRecord {
                name,
                elapsed,
                gain: best.saturating_sub(elapsed),
            },
            None => Moment::SegmentDone { name, elapsed },
        });
    }
    moments
}

/// How far before a segment's start the rider is told of it, at `speed` metres a second with
/// the ride sped up `time_scale` times: [`CALL_AHEAD`] of the rider's own time, between
/// [`LAST_CALL`] and [`HERALD`].
fn call_distance(speed: f64, time_scale: f64) -> Meters {
    Meters((speed * time_scale * CALL_AHEAD.as_secs_f64()).clamp(LAST_CALL.0, HERALD.0))
}

/// The segments the rider is about to start since the last look, as moments, each told once a
/// ride; none that will not be timed on it (passed, or jumped past).
pub(crate) fn heralded(active: &mut ActiveRide) -> Vec<Moment> {
    let state = active.ride.state();
    let (position, speed) = (state.distance.0, state.speed.0);
    let reach = call_distance(speed, active.time_scale);
    let mut moments = Vec::new();
    for ((record, done), told) in active
        .records
        .segments
        .iter()
        .zip(&active.segments_done)
        .zip(&mut active.segments_told)
    {
        let ahead = record.segment.start_m - position;
        if *done || *told || ahead <= 0.0 || ahead > reach.0 {
            continue;
        }
        *told = true;
        moments.push(Moment::SegmentAhead {
            name: record.segment.name.clone(),
            distance: Meters(ahead),
        });
    }
    moments
}

impl App {
    /// The active rider's segments of the route with the key `route`.
    pub(crate) fn stored_segments(&self, route: &str) -> Vec<Segment> {
        segments::load(&self.data_dir, &self.profile.id, route)
            .inspect_err(|error| tracing::warn!(%error, "cannot read the course's segments"))
            .unwrap_or_default()
    }

    /// The active rider's segments of `route` with their best times among `same_route`, the
    /// rides on it; also those not to be timed until the rider checks their ends.
    pub(crate) fn segment_bests(
        &self,
        route: &Route,
        same_route: &[&RideRecord],
    ) -> Vec<SegmentBest> {
        let stored = self.stored_segments(&route.key());
        let best = best_by_key(same_route.iter().copied(), &stored);
        bests(stored, &route.revision(), &best)
    }

    /// The active rider's segments of the loaded course, in the order they were added, with
    /// their best times; also those to check (see [`SegmentBest::confirmed`]).
    #[must_use]
    pub fn segments(&self) -> Vec<SegmentBest> {
        let Some(route) = self.route() else {
            return Vec::new();
        };
        let key = route.key();
        let history = self.history();
        let same_route: Vec<&RideRecord> = history
            .iter()
            .map(|entry| &entry.record)
            .filter(|record| record.route_key.as_deref() == Some(key.as_str()))
            .collect();
        self.segment_bests(route, &same_route)
    }

    fn write_segment(&self, route: &Route, segment: &Segment) -> Result<(), AppError> {
        segments::save(
            &self.data_dir,
            &self.profile.id,
            &route.key(),
            route.length().0,
            segment,
        )
        .map_err(|e| Failure::new(Kind::SaveSegment, e).into())
    }

    /// Adds a segment named `name` from `start` to `end` along the loaded course; returns its
    /// id.
    ///
    /// # Errors
    /// No loaded route, a segment without a name, too short or off the course, or a
    /// file-system failure.
    pub fn add_segment(&self, name: &str, start: Meters, end: Meters) -> Result<String, AppError> {
        let route = self.route().ok_or(AppError::NoRoute)?;
        let segment = Segment {
            id: segments::new_id(),
            name: name.trim().to_owned(),
            start_m: start.0,
            end_m: end.0,
            revision: route.revision(),
            earlier: Vec::new(),
            added_ms: segments::now_ms(),
        };
        self.write_segment(route, &segment)?;
        Ok(segment.id)
    }

    /// Renames or moves the segment `id` of the loaded course. Moved, it is a new stretch, and
    /// its times start again. On a course prepared differently since its ends were set, this
    /// is the rider checking them: it is timed again, and its times from before still count.
    ///
    /// # Errors
    /// As [`App::add_segment`], or no such segment.
    pub fn change_segment(
        &self,
        id: &str,
        name: &str,
        start: Meters,
        end: Meters,
    ) -> Result<(), AppError> {
        let route = self.route().ok_or(AppError::NoRoute)?;
        let mut segment = self
            .stored_segments(&route.key())
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Failure::new(Kind::SaveSegment, "no such segment"))?;
        let revision = route.revision();
        if segment.revision != revision {
            let before = Placement {
                revision: std::mem::replace(&mut segment.revision, revision),
                start_m: segment.start_m,
                end_m: segment.end_m,
            };
            if !segment.earlier.contains(&before) {
                segment.earlier.push(before);
            }
        } else if segment.key() != SegmentKey::new(&revision, start.0, end.0) {
            segment.earlier.clear();
        }
        name.trim().clone_into(&mut segment.name);
        segment.start_m = start.0;
        segment.end_m = end.0;
        self.write_segment(route, &segment)
    }

    /// Removes the segment `id` of the loaded course; the times ridden on it stay with the rides.
    ///
    /// # Errors
    /// No loaded route or a file-system failure.
    pub fn remove_segment(&self, id: &str) -> Result<(), AppError> {
        let route = self.route().ok_or(AppError::NoRoute)?;
        segments::remove(&self.data_dir, &self.profile.id, &route.key(), id)
            .map_err(|e| Failure::new(Kind::SaveSegment, e).into())
    }

    /// Where the track of the GPX file at `path` lies on the loaded course, with the file's name
    /// for it: a segment to add.
    ///
    /// # Errors
    /// No loaded route, an unreadable file, or a track that does not follow the course.
    pub fn segment_from_gpx(&self, path: &Path) -> Result<(String, Meters, Meters), AppError> {
        let route = self.route().ok_or(AppError::NoRoute)?;
        let file = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let xml = std::fs::read_to_string(path)
            .map_err(|e| Failure::new(Kind::ReadFile, e).arg(&file))?;
        let track = orqwa_routes::track_points(&xml)
            .map_err(|e| Failure::new(Kind::ReadFile, e).arg(&file))?;
        let (start, end) = orqwa_routes::segments::locate(route, &track).ok_or_else(|| {
            Failure::new(Kind::SegmentRoute, "the track leaves the course").arg(&file)
        })?;
        let name = orqwa_routes::route_name(&xml)
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .or_else(|| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let name: String = name.chars().take(segments::MAX_NAME_CHARS).collect();
        Ok((name, start, end))
    }

    /// The segment the rider is on, or else the next one within a few hundred metres; on
    /// overlapping segments, the one ending first.
    #[must_use]
    pub fn current_segment(&self) -> Option<SegmentProgress> {
        let active = self.ride.as_ref()?;
        let state = active.ride.state();
        let entry = active.ride.entry_distance();
        let open = active
            .records
            .segments
            .iter()
            .zip(&active.segments_done)
            .filter(|(_, done)| !**done)
            .map(|(s, _)| s);
        let (on, ahead): (Vec<&SegmentBest>, Vec<&SegmentBest>) =
            open.partition(|s| s.segment.start_m <= state.distance.0);
        if let Some(record) = on
            .into_iter()
            .filter(|s| state.distance.0 < s.segment.end_m)
            .min_by(|a, b| a.segment.end_m.total_cmp(&b.segment.end_m))
        {
            let since_jump = active
                .ride
                .samples()
                .get(active.segments_from..)
                .unwrap_or_default();
            let started =
                time_at(since_jump, in_ride(&record.segment, entry).0).unwrap_or(state.elapsed);
            return Some(SegmentProgress {
                segment: record.segment.clone(),
                ahead: None,
                ridden: Meters(state.distance.0 - record.segment.start_m),
                elapsed: state.elapsed.saturating_sub(started),
                best: record.best,
            });
        }
        let next = ahead
            .into_iter()
            .min_by(|a, b| a.segment.start_m.total_cmp(&b.segment.start_m))?;
        let to_go = next.segment.start_m - state.distance.0;
        (to_go <= HERALD.0).then(|| SegmentProgress {
            segment: next.segment.clone(),
            ahead: Some(Meters(to_go)),
            ridden: Meters(0.0),
            elapsed: Duration::ZERO,
            best: next.best,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::tests::{ride_to_the_finish, run_until, temp_dir, write_climb_route};
    use crate::{AppEvent, GhostChoice};

    fn loaded(name: &str) -> (std::path::PathBuf, App) {
        let dir = temp_dir(name);
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_route(write_climb_route(&dir), true);
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        (dir, app)
    }

    #[test]
    fn segments_are_marked_renamed_and_removed_on_the_loaded_course() {
        let (dir, app) = loaded("segments-edit");
        let sprint = app
            .add_segment(" Village sprint ", Meters(100.0), Meters(300.0))
            .unwrap();
        let climb = app
            .add_segment("The hill", Meters(300.0), Meters(900.0))
            .unwrap();
        assert_ne!(sprint, climb);
        let names: Vec<String> = app.segments().into_iter().map(|s| s.segment.name).collect();
        assert_eq!(names, ["Village sprint", "The hill"]);

        app.change_segment(&climb, "Hill", Meters(300.0), Meters(850.0))
            .unwrap();
        assert_eq!(app.segments()[1].segment.name, "Hill");
        assert_eq!(app.segments()[1].segment.end_m, 850.0);
        app.remove_segment(&sprint).unwrap();
        assert_eq!(app.segments().len(), 1);

        // Nothing unnamed, too short, backwards or beyond the course is kept.
        for (name, start, end) in [
            ("", 0.0, 200.0),
            ("Short", 100.0, 120.0),
            ("Backwards", 600.0, 400.0),
            ("Beyond", 900.0, 5000.0),
        ] {
            assert!(app.add_segment(name, Meters(start), Meters(end)).is_err());
        }
        assert_eq!(app.segments().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_gpx_track_along_the_course_becomes_a_segment_and_one_elsewhere_does_not() {
        let (dir, app) = loaded("segments-gpx");
        // Along the course from 400 m to 800 m (it runs due north from 46° N, 7° E).
        let gpx = |name: &str, lon: f64| {
            let path = dir.join(format!("{name}.gpx"));
            let mut file = std::fs::File::create(&path).unwrap();
            write!(file, "<gpx><trk><name>{name}</name><trkseg>").unwrap();
            for i in 40..=80 {
                let lat = 46.0 + f64::from(i) * 10.0 / 111_195.0;
                write!(file, r#"<trkpt lat="{lat}" lon="{lon}"/>"#).unwrap();
            }
            write!(file, "</trkseg></trk></gpx>").unwrap();
            path
        };

        let (name, start, end) = app.segment_from_gpx(&gpx("Steep bit", 7.0)).unwrap();
        assert_eq!(name, "Steep bit");
        assert!((start.0 - 400.0).abs() < 15.0, "{start:?}");
        assert!((end.0 - 800.0).abs() < 15.0, "{end:?}");
        assert!(matches!(
            app.segment_from_gpx(&gpx("Elsewhere", 7.01)),
            Err(AppError::Reported(Failure {
                kind: Kind::SegmentRoute,
                ..
            }))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn segments_are_timed_against_the_riders_best() {
        let (dir, mut app) = loaded("segments-ride");
        app.add_segment("Sprint", Meters(50.0), Meters(250.0))
            .unwrap();

        let first = ride_to_the_finish(&mut app, &GhostChoice::None);
        let time = first.iter().find_map(|e| match e {
            AppEvent::Moment(Moment::SegmentDone { name, elapsed }) if name == "Sprint" => {
                Some(*elapsed)
            }
            _ => None,
        });
        assert!(time.is_some_and(|t| t > Duration::ZERO), "{first:?}");
        assert_eq!(app.segments()[0].best, time);
        let history = app.history();
        assert_eq!(history[0].record.segments.len(), 1);
        assert_eq!(history[0].segment_records, [true]);
        // Beside the ride, in their own file: a rename leaves them be, a delete takes them.
        let fit = history[0].fit.clone();
        let times = orqwa_storage::rides::segments_path(&fit);
        assert!(times.exists(), "{}", times.display());
        app.rename_ride(&fit, "Lunch spin").unwrap();
        assert_eq!(app.history()[0].record.segments.len(), 1);
        assert_eq!(app.segments()[0].best, time);

        // Renamed, it keeps its times; moved, it is a new stretch without any.
        let id = app.segments()[0].segment.id.clone();
        app.change_segment(&id, "Town sprint", Meters(50.0), Meters(250.0))
            .unwrap();
        assert_eq!(app.segments()[0].best, time);
        app.change_segment(&id, "Town sprint", Meters(60.0), Meters(250.0))
            .unwrap();
        assert_eq!(app.segments()[0].best, None);
        app.delete_ride(&fit).unwrap();
        assert!(!times.exists(), "deleted with the ride");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_course_prepared_differently_asks_for_its_segments_ends_and_keeps_their_times() {
        use std::fmt::Write as _;

        let (dir, mut app) = loaded("segments-rebuilt");
        let id = app
            .add_segment("Sprint", Meters(50.0), Meters(250.0))
            .unwrap();
        ride_to_the_finish(&mut app, &GhostChoice::None);
        let first = app.segments()[0].best;
        assert!(first.is_some());
        let (key, revision) = {
            let route = app.route().unwrap();
            (route.key(), route.revision())
        };

        // The same track on other elevations: the same course, prepared differently.
        let mut xml = String::from("<gpx><trk><name>Hill</name><trkseg>");
        for i in 0..=120 {
            let lat = 46.0 + f64::from(i) * 10.0 / 111_195.0;
            let ele = 520.0 + f64::from((i - 30).clamp(0, 60)) * 0.6;
            write!(
                xml,
                r#"<trkpt lat="{lat}" lon="7"><ele>{ele}</ele></trkpt>"#
            )
            .unwrap();
        }
        xml.push_str("</trkseg></trk></gpx>");
        let path = dir.join("hill-again.gpx");
        std::fs::write(&path, xml).unwrap();
        drop(app);
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_route(path, true);
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        assert_eq!(app.route().unwrap().key(), key);
        assert_ne!(app.route().unwrap().revision(), revision);

        // Its ends may lie elsewhere now: listed to check, not timed, its time kept.
        let listed = &app.segments()[0];
        assert!(!listed.confirmed);
        assert_eq!(listed.best, first);
        let unchecked = ride_to_the_finish(&mut app, &GhostChoice::None);
        assert!(
            !unchecked
                .iter()
                .any(|e| matches!(e, AppEvent::Moment(Moment::SegmentDone { .. }))),
            "{unchecked:?}"
        );
        let history = app.history();
        assert_eq!(history[0].record.segments, []);
        assert_eq!(
            history[1].segment_records,
            [true],
            "the old time stays a record"
        );

        // Checked, it is timed again, and the time from before still counts.
        app.change_segment(&id, "Sprint", Meters(50.0), Meters(250.0))
            .unwrap();
        assert!(app.segments()[0].confirmed);
        assert_eq!(app.segments()[0].best, first);
        let checked = ride_to_the_finish(&mut app, &GhostChoice::None);
        assert!(
            checked
                .iter()
                .any(|e| matches!(e, AppEvent::Moment(Moment::SegmentDone { .. }))),
            "{checked:?}"
        );
        let both = app.segments()[0].best;
        let history = app.history();
        assert_eq!(
            both,
            history[0].record.segments[0]
                .elapsed
                .min(first.unwrap())
                .into()
        );

        // Moved on the course as it is, it is another stretch.
        app.change_segment(&id, "Sprint", Meters(60.0), Meters(250.0))
            .unwrap();
        assert_eq!(app.segments()[0].best, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_rider_hears_when_a_saved_ride_lost_its_segment_times() {
        use orqwa_devices::fake::FakeRider;
        use orqwa_domain::units::{Percent, Rpm, Watts};
        use orqwa_physics::DescentMode;

        let (dir, mut app) = loaded("segments-unsaved");
        app.add_segment("Sprint", Meters(50.0), Meters(250.0))
            .unwrap();
        app.connect_trainer(crate::TrainerChoice::Fake(FakeRider {
            power: Watts(400.0),
            cadence: Rpm(90.0),
            heart: None,
        }))
        .unwrap();
        app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
            .unwrap();
        run_until(&mut app, |e| matches!(e, AppEvent::Connected(_)));
        app.set_time_scale(20.0);
        run_until(&mut app, |e| {
            matches!(e, AppEvent::Moment(Moment::SegmentDone { .. }))
        });
        // Something in the way of the times file, where the ride will be saved.
        let start = app.ride.as_ref().and_then(|active| active.started).unwrap();
        let fit = app
            .rides_dir()
            .join(crate::paths::activity_file_name(start));
        std::fs::create_dir_all(orqwa_storage::rides::segments_path(&fit)).unwrap();

        let events = app.finish_ride();
        assert!(
            matches!(events.first(), Some(AppEvent::RideSaved(saved)) if *saved == fit),
            "{events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                AppEvent::Error(Failure {
                    kind: Kind::SaveSegmentTimes,
                    ..
                })
            )),
            "{events:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_segment_is_announced_the_same_time_ahead_on_the_flat_and_on_a_climb() {
        let kmh = |v: f64| v / 3.6;
        // 15 s ahead: 187.5 m at 45 km/h, 125 m at 30 km/h.
        assert!((call_distance(kmh(45.0), 1.0).0 - 187.5).abs() < 1e-9);
        assert!((call_distance(kmh(30.0), 1.0).0 - 125.0).abs() < 1e-9);
        // A slow climb or a standstill still gets told before the start…
        assert_eq!(call_distance(kmh(10.0), 1.0), LAST_CALL);
        assert_eq!(call_distance(0.0, 1.0), LAST_CALL);
        // …and no earlier than the panel shows it, even sped up.
        assert_eq!(call_distance(kmh(40.0), 20.0), HERALD);
        assert!((call_distance(kmh(30.0), 2.0).0 - 250.0).abs() < 1e-9);
    }

    #[test]
    fn a_ride_sped_up_twentyfold_still_shows_times_and_tells_its_segments() {
        use orqwa_devices::fake::FakeRider;
        use orqwa_domain::units::{Percent, Rpm, Watts};
        use orqwa_physics::DescentMode;

        let (dir, mut app) = loaded("segments-sped-up");
        app.add_segment("Sprint", Meters(400.0), Meters(700.0))
            .unwrap();
        app.connect_trainer(crate::TrainerChoice::Fake(FakeRider {
            power: Watts(400.0),
            cadence: Rpm(90.0),
            heart: None,
        }))
        .unwrap();
        app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
            .unwrap();
        let mut events = Vec::new();
        while !events.iter().any(|e| matches!(e, AppEvent::Connected(_))) {
            std::thread::sleep(Duration::from_millis(16));
            events.extend(app.update(Duration::from_millis(16)));
        }
        assert_eq!(app.set_time_scale(20.0), 20.0);
        // Frames as the app draws them; at 20× a frame covers several metres.
        let mut missing = Vec::new();
        for _ in 0..20_000 {
            events.extend(app.update(Duration::from_millis(16)));
            if events.iter().any(|e| matches!(e, AppEvent::RideFinished)) {
                break;
            }
            let at = app.ride_state().unwrap().distance.0;
            if (400.0..700.0).contains(&at) && app.current_segment().is_none() {
                missing.push(at);
            }
        }
        let told = |kind: fn(&Moment) -> bool| {
            events
                .iter()
                .filter(|e| matches!(e, AppEvent::Moment(m) if kind(m)))
                .count()
        };
        assert_eq!(
            missing,
            Vec::<f64>::new(),
            "the panel's segment while on it"
        );
        assert_eq!(told(|m| matches!(m, Moment::SegmentAhead { .. })), 1);
        assert_eq!(
            told(|m| matches!(m, Moment::SegmentDone { .. })),
            1,
            "{events:?}"
        );
        app.finish_ride();
        // Its summary still shows the time, as no record: the card may go with the finish.
        let history = app.history();
        assert_eq!(history[0].record.segments.len(), 1);
        assert_eq!(history[0].segment_records, [false]);
        assert_eq!(app.segments()[0].best, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_jump_is_not_counted_as_riding_a_segment() {
        use orqwa_devices::fake::FakeRider;
        use orqwa_domain::units::{Percent, Rpm, Watts};
        use orqwa_physics::DescentMode;

        let (dir, mut app) = loaded("segments-jump");
        app.add_segment("Jumped into", Meters(300.0), Meters(600.0))
            .unwrap();
        app.add_segment("Ridden again", Meters(700.0), Meters(1000.0))
            .unwrap();
        app.connect_trainer(crate::TrainerChoice::Fake(FakeRider {
            power: Watts(400.0),
            cadence: Rpm(90.0),
            heart: None,
        }))
        .unwrap();
        app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
            .unwrap();
        run_until(&mut app, |e| matches!(e, AppEvent::Connected(_)));
        app.set_time_scale(20.0);
        let mut events = Vec::new();
        let mut ride_to = |app: &mut App, distance: f64| {
            while app.ride_state().is_some_and(|s| s.distance.0 < distance) {
                std::thread::sleep(Duration::from_millis(16));
                events.extend(app.update(Duration::from_millis(16)));
            }
        };

        // Into the first from before it; out of the second, back to before its start.
        ride_to(&mut app, 100.0);
        assert!(app.jump_to(Meters(450.0)));
        ride_to(&mut app, 850.0);
        assert!(app.jump_to(Meters(650.0)));
        let back = app.ride_state().unwrap().elapsed;
        ride_to(&mut app, 1100.0);
        let since_back = app.ride_state().unwrap().elapsed.saturating_sub(back);
        app.finish_ride();

        let done: Vec<(&str, Duration)> = events
            .iter()
            .filter_map(|e| match e {
                AppEvent::Moment(Moment::SegmentDone { name, elapsed }) => {
                    Some((name.as_str(), *elapsed))
                }
                _ => None,
            })
            .collect();
        assert_eq!(done.len(), 1, "{done:?}");
        let (name, elapsed) = done[0];
        assert_eq!(name, "Ridden again");
        assert!(elapsed < since_back, "{elapsed:?} of {since_back:?}");
        let saved = &app.history()[0].record.segments;
        assert_eq!(saved.len(), 1, "{saved:?}");
        assert_eq!(saved[0].name, "Ridden again");
        assert_eq!(saved[0].elapsed, elapsed, "saved as told");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_rider_is_told_once_before_a_segment_starts() {
        let (dir, mut app) = loaded("segments-ahead");
        app.add_segment("Sprint", Meters(400.0), Meters(700.0))
            .unwrap();

        let events = ride_to_the_finish(&mut app, &GhostChoice::None);
        let told: Vec<(usize, Meters)> = events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                AppEvent::Moment(Moment::SegmentAhead { name, distance }) if name == "Sprint" => {
                    Some((i, *distance))
                }
                _ => None,
            })
            .collect();
        let done = events
            .iter()
            .position(|e| matches!(e, AppEvent::Moment(Moment::SegmentDone { .. })));
        assert_eq!(told.len(), 1, "{events:?}");
        let (at, distance) = told[0];
        assert!(done.is_some_and(|done| at < done), "told before it is done");
        assert!(
            distance.0 > 0.0 && distance.0 <= HERALD.0,
            "told ahead of its start: {distance:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
