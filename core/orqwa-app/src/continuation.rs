//! Solo continuation uses the same absolute-position/recorded-distance boundary as late entry.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use orqwa_domain::units::{Meters, Percent};
use orqwa_physics::DescentMode;
use orqwa_routes::Route;
use orqwa_storage::continuation::{self, Point};

use crate::failure::{Failure, Kind};
use crate::{ActiveRide, App, AppError, CourseEntry, GhostChoice};

/// The quick summary can still discard the activity that changed a continuation point.
pub(super) struct Undo {
    fit: PathBuf,
    rider: String,
    route: String,
    before: Option<Point>,
    after: Option<Point>,
}

fn revision(route: &Route) -> String {
    // Centimetre precision ignores platform rounding, while detecting prepared-route changes.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for point in route.points() {
        for (value, scale) in [
            (point.lat, 1e7),
            (point.lon, 1e7),
            (point.elevation.0, 100.0),
            (point.distance.0, 100.0),
        ] {
            for byte in (value * scale).round().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    format!("{hash:016x}")
}

impl App {
    fn read_continuation(&self, key: &str) -> Option<Point> {
        continuation::load(&self.data_dir, &self.profile.id, key)
            .inspect_err(|error| tracing::warn!(%error, "cannot read course continuation"))
            .ok()
            .flatten()
    }

    /// The active rider's point on a library course, without opening or building it.
    #[must_use]
    pub fn course_continuation(&self, course: &CourseEntry) -> Option<Point> {
        let point = self.read_continuation(course.manifest.route_key.as_deref()?)?;
        ((point.length_m - course.manifest.length_m).abs() < 0.01).then_some(point)
    }

    /// A remembered point, revalidated against the loaded prepared route.
    #[must_use]
    pub fn continuation_point(&self) -> Option<Point> {
        let route = self.route.as_ref()?;
        let point = self.read_continuation(&route.key())?;
        if point.revision != revision(route) || point.distance_m >= route.length().0 {
            if let Err(error) =
                continuation::save(&self.data_dir, &self.profile.id, &route.key(), None)
            {
                tracing::warn!(%error, "cannot clear an outdated continuation point");
            }
            return None;
        }
        Some(point)
    }

    /// Removes the active rider's point on the loaded course, leaving all activities intact.
    ///
    /// # Errors
    /// No loaded route, or a file-system failure.
    pub fn reset_continuation(&self) -> Result<(), AppError> {
        let route = self.route.as_ref().ok_or(AppError::NoRoute)?;
        continuation::save(&self.data_dir, &self.profile.id, &route.key(), None)
            .map_err(|e| Failure::new(Kind::SaveContinuation, e).into())
    }

    /// Accepts the quick summary's continuation change, ending its discard rollback window.
    pub fn accept_continuation(&self, fit: &Path) {
        let mut undo = self
            .continuation_undo
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if undo.as_ref().is_some_and(|change| change.fit == fit) {
            *undo = None;
        }
    }

    /// Whether Finish & save should offer to remember this unfinished solo course ride.
    #[must_use]
    pub fn can_save_continuation(&self) -> bool {
        self.ride.as_ref().is_some_and(|active| {
            active.solo_course
                && !active.simulated
                && !active.ride.is_finished()
                && active.started.is_some()
                && !active.ride.samples().is_empty()
                && active.ride.state().ridden.0 > 0.0
        })
    }

    /// Continues a solo 3D or video course as a fresh, stationary activity (R220).
    ///
    /// # Errors
    /// A missing/changed point, an active group or any normal ride-start error.
    pub fn continue_ride(
        &mut self,
        difficulty: Percent,
        descent: DescentMode,
        ghost: &GhostChoice,
    ) -> Result<(), AppError> {
        self.check_group_start()?;
        let point = self.continuation_point().ok_or_else(|| {
            AppError::Reported(Failure::new(
                Kind::NoContinuation,
                "point missing or route changed",
            ))
        })?;
        self.start_ride(difficulty, descent, ghost)?;
        let active = self.ride.as_mut().ok_or(AppError::NoRoute)?;
        let distance = Meters(point.distance_m);
        let entered = active.ride.enter_course(distance);
        debug_assert!(entered, "validated course point must be enterable");
        active.milestones.skip_to(distance);
        active.greeting = None;
        let climbs = active.ride.route().map_or(&[][..], Route::climbs);
        active.next_climb = climbs.partition_point(|climb| climb.start.0 < distance.0);
        if let Some(ghost) = &mut active.ghost
            && !ghost.enter_course(distance)
        {
            active.ghost = None;
        }
        if let Some(video) = &self.video {
            let time = video.time_at(distance);
            if let Some(player) = &active.player {
                player.show(time);
            }
            if let Some(sound) = &active.sound {
                sound.follow(time);
            }
        }
        Ok(())
    }

    pub(super) fn save_ride_continuation(
        &self,
        active: &ActiveRide,
        fit: &Path,
        remember: bool,
    ) -> Result<(), Failure> {
        if !active.solo_course || active.simulated {
            return Ok(());
        }
        let finished = active.ride.is_finished();
        if !finished && (!remember || active.ride.state().ridden.0 <= 0.0) {
            return Ok(());
        }
        let route = active.ride.route().expect("solo course");
        let key = route.key();
        let after = (!finished).then(|| Point {
            revision: revision(route),
            distance_m: active.ride.state().distance.0,
            length_m: route.length().0,
        });
        let before = self.read_continuation(&key);
        continuation::save(&self.data_dir, &self.profile.id, &key, after.as_ref())
            .map_err(|e| Failure::new(Kind::SaveContinuation, e))?;
        *self
            .continuation_undo
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Undo {
            fit: fit.to_owned(),
            rider: self.profile.id.clone(),
            route: key,
            before,
            after,
        });
        Ok(())
    }

    pub(super) fn restore_discarded_continuation(&self, fit: &Path) -> Result<(), AppError> {
        let mut undo = self
            .continuation_undo
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(change) = undo.as_ref().filter(|change| change.fit == fit) {
            // A deliberate reset or a change synced from another computer takes precedence.
            if continuation::load(&self.data_dir, &change.rider, &change.route)
                .ok()
                .flatten()
                == change.after
            {
                continuation::save(
                    &self.data_dir,
                    &change.rider,
                    &change.route,
                    change.before.as_ref(),
                )
                .map_err(|e| Failure::new(Kind::SaveContinuation, e))?;
            }
            *undo = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime};

    use orqwa_devices::fake::FakeRider;
    use orqwa_domain::telemetry::Telemetry;
    use orqwa_domain::units::{MetersPerSecond, Rpm, Watts};
    use orqwa_session::analysis;
    use orqwa_storage::{decode_fit, rides};

    use super::*;
    use crate::tests::{run_until, temp_dir, write_route};
    use crate::{AppEvent, TrainerChoice};

    static DAY: AtomicU64 = AtomicU64::new(1);

    fn setup(name: &str) -> App {
        let dir = temp_dir(name);
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_route(write_route(&dir), true);
        run_until(&mut app, |event| matches!(event, AppEvent::CourseAdded(_)));
        connect(&mut app);
        app
    }

    fn connect(app: &mut App) {
        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(250.0),
            cadence: Rpm(90.0),
            heart: None,
        }))
        .unwrap();
        run_until(app, |event| matches!(event, AppEvent::Connected(_)));
    }

    fn start(app: &mut App, continuing: bool) {
        if continuing {
            app.continue_ride(
                Percent(50.0),
                DescentMode::Coast,
                &GhostChoice::Power(Watts(250.0)),
            )
            .unwrap();
        } else {
            app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
                .unwrap();
        }
        // Tests advance logical time without waiting wall-clock days between activities.
        let active = app.ride.as_mut().unwrap();
        active.started = Some(
            SystemTime::UNIX_EPOCH
                + Duration::from_secs(1_800_000_000 + DAY.fetch_add(1, Ordering::Relaxed) * 86400),
        );
        active.ride.on_telemetry(&Telemetry {
            power: Some(Watts(250.0)),
            cadence: Some(Rpm(90.0)),
            ..Default::default()
        });
    }

    fn advance(app: &mut App, seconds: u32) {
        for _ in 0..seconds {
            app.update(Duration::from_secs(1));
        }
    }

    fn saved(events: &[AppEvent]) -> PathBuf {
        events
            .iter()
            .find_map(|event| match event {
                AppEvent::RideSaved(path) => Some(path.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected saved activity: {events:?}"))
    }

    #[test]
    fn continuation_skips_an_entered_climb_but_times_complete_later_climbs_and_power() {
        let mut app = setup("continuation-climbs");
        let xml = r#"<gpx><trk><trkseg><trkpt lat="46" lon="7"><ele>500</ele></trkpt><trkpt lat="46.01" lon="7"><ele>650</ele></trkpt><trkpt lat="46.02" lon="7"><ele>500</ele></trkpt><trkpt lat="46.03" lon="7"><ele>800</ele></trkpt></trkseg></trk></gpx>"#;
        let route = app.runtime.block_on(Route::from_gpx(xml, None)).unwrap();
        assert_eq!(route.climbs().len(), 2);
        let distance = route.climbs()[0].start.0.midpoint(route.climbs()[0].end.0);
        let point = Point {
            revision: revision(&route),
            distance_m: distance,
            length_m: route.length().0,
        };
        continuation::save(&app.data_dir, &app.profile.id, &route.key(), Some(&point)).unwrap();
        app.route = Some(route);
        start(&mut app, true);
        assert!(app.current_climb().is_none());
        let mut moments = Vec::new();
        for _ in 0..4000 {
            moments.extend(app.update(Duration::from_secs(1)));
            if app.ride.as_ref().unwrap().ride.is_finished() {
                break;
            }
        }
        assert!(app.ride.as_ref().unwrap().ride.is_finished());
        let topped: Vec<usize> = moments
            .iter()
            .filter_map(|event| match event {
                AppEvent::Moment(crate::moments::Moment::ClimbTopped { index, .. }) => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(topped, [1], "no retroactive skipped-climb moment");
        let fit = saved(&app.finish_ride());
        let record = rides::load(&fit).unwrap();
        assert!(record.route_time.is_none());
        assert_eq!(record.climbs.len(), 1);
        assert!(record.climbs[0].start.0 >= distance);
        assert!(!record.simulated && !record.assisted);
        assert_ne!(app.history()[0].power_records, [] as [Duration; 0]);
        app.shutdown();
    }

    #[test]
    fn failed_activity_saves_and_group_originated_rides_do_not_create_points() {
        let mut app = setup("continuation-save-failure");
        start(&mut app, false);
        advance(&mut app, 10);
        app.ride.as_mut().unwrap().solo_course = false;
        assert!(!app.can_save_continuation());
        saved(&app.finish_ride_remembering(true));
        assert!(app.continuation_point().is_none());
        start(&mut app, false);
        advance(&mut app, 10);
        let rides_dir = app.rides_dir();
        std::fs::remove_dir_all(&rides_dir).unwrap();
        std::fs::write(&rides_dir, b"blocked").unwrap();
        let events = app.finish_ride_remembering(true);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AppEvent::Error(_)))
        );
        assert!(app.continuation_point().is_none());
        app.shutdown();
    }

    #[test]
    fn continuation_is_opt_in_persistent_and_starts_a_fresh_activity() {
        let mut app = setup("continuation-fresh");
        start(&mut app, false);
        advance(&mut app, 15);
        assert!(app.can_save_continuation());
        app.set_paused(true);
        advance(&mut app, 5);
        assert!(
            app.continuation_point().is_none(),
            "pause never saves a point"
        );
        app.set_paused(false);
        let first = saved(&app.finish_ride_remembering(true));
        let point = app.continuation_point().unwrap();
        assert!(point.distance_m > 5.0 && point.distance_m < 400.0);
        let course = app.course.clone().unwrap();
        let data = app.data_dir.clone();
        let cache = app.cache_dir.clone();
        app.shutdown();
        drop(app);
        let mut app = App::new(data, cache).unwrap();
        app.open_course(course.clone());
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        connect(&mut app);
        assert_eq!(app.continuation_point(), Some(point.clone()));
        assert!(app.course_continuation(&app.courses()[0]).is_some());
        start(&mut app, true);
        let state = app.ride_state().unwrap();
        assert_eq!(state.distance, Meters(point.distance_m));
        assert_eq!(state.ridden, Meters(0.0));
        assert_eq!(state.elapsed, Duration::ZERO);
        assert_eq!(state.speed, MetersPerSecond(0.0));
        assert_eq!(app.ride.as_ref().unwrap().ride.samples(), []);
        assert!(!app.ride.as_ref().unwrap().simulated);
        let ghost = app.ghost_state().unwrap();
        assert_eq!(ghost.distance, state.distance);
        assert_eq!(ghost.gap, Some(0.0));
        advance(&mut app, 15);
        let second = saved(&app.finish_ride());
        assert_eq!(
            app.continuation_point(),
            Some(point.clone()),
            "Not now preserves the old point"
        );
        assert_ne!(first, second);
        let (_, samples) = decode_fit(&std::fs::read(&second).unwrap()).unwrap();
        assert_eq!(samples[0].distance, Meters(0.0));
        assert_eq!(samples[0].elapsed, Duration::ZERO);
        assert!(samples[0].location.unwrap().lat > app.route().unwrap().points()[0].lat);
        assert!(analysis::summarize(&samples, Watts(200.0)).distance.0 < point.distance_m);
        let record = rides::load(&second).unwrap();
        assert!(!record.simulated && record.route_time.is_none());
        assert_eq!(app.history().len(), 2);
        app.reset_continuation().unwrap();
        assert!(app.continuation_point().is_none());
        assert!(first.exists() && second.exists());
        app.shutdown();
    }

    #[test]
    fn decline_from_start_abort_simulation_and_discard_preserve_the_old_point() {
        let mut app = setup("continuation-choices");
        start(&mut app, false);
        advance(&mut app, 15);
        saved(&app.finish_ride_remembering(true));
        let old = app.continuation_point().unwrap();
        start(&mut app, false);
        assert_eq!(app.ride_state().unwrap().distance, Meters(0.0));
        advance(&mut app, 5);
        saved(&app.finish_ride());
        assert_eq!(app.continuation_point(), Some(old.clone()));
        start(&mut app, true);
        advance(&mut app, 5);
        app.abort_ride();
        assert_eq!(app.continuation_point(), Some(old.clone()));
        start(&mut app, true);
        advance(&mut app, 5);
        let discarded = saved(&app.finish_ride_remembering(true));
        assert!(app.continuation_point().unwrap().distance_m > old.distance_m);
        app.delete_ride(&discarded).unwrap();
        assert_eq!(app.continuation_point(), Some(old.clone()));
        start(&mut app, true);
        app.set_time_scale(2.0);
        advance(&mut app, 5);
        assert!(!app.can_save_continuation());
        saved(&app.finish_ride_remembering(true));
        assert_eq!(app.continuation_point(), Some(old.clone()));
        start(&mut app, true);
        advance(&mut app, 100);
        assert!(!app.can_save_continuation());
        let finished = saved(&app.finish_ride());
        assert!(
            app.continuation_point().is_none(),
            "saving the finish clears the point"
        );
        assert!(rides::load(&finished).unwrap().route_time.is_none());
        app.delete_ride(&finished).unwrap();
        assert_eq!(
            app.continuation_point(),
            Some(old),
            "discard restores the unfinished point"
        );
        app.shutdown();
    }

    #[test]
    fn points_follow_route_identity_but_not_changed_prepared_geometry_or_another_rider() {
        let mut app = setup("continuation-identity");
        start(&mut app, false);
        advance(&mut app, 10);
        saved(&app.finish_ride_remembering(true));
        let original = app.profile.id.clone();
        let query = crate::library::CourseQuery {
            in_progress: true,
            ..crate::library::CourseQuery::default()
        };
        assert_eq!(app.course_library(&query).groups[0].courses.len(), 1);
        let other = app
            .save_profile(
                None,
                orqwa_domain::profile::Profile {
                    name: "Other".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        app.select_profile(&other).unwrap();
        assert!(app.continuation_point().is_none());
        assert_eq!(app.course_library(&query).groups, []);
        app.select_profile(&original).unwrap();
        assert_eq!(app.course_library(&query).groups[0].courses.len(), 1);
        let path = app.course.clone().unwrap();
        app.rename_course(&path, "New course name").unwrap();
        assert!(app.continuation_point().is_some());
        let route_key = app.route().unwrap().key();
        let xml = std::fs::read_to_string(write_route(&app.cache_dir)).unwrap();
        let rounded = xml.replace("<ele>500</ele>", "<ele>500.001</ele>");
        app.route = Some(
            app.runtime
                .block_on(Route::from_gpx(&rounded, None))
                .unwrap(),
        );
        assert!(
            app.continuation_point().is_some(),
            "sub-centimetre rounding preserves the point"
        );
        let changed = app
            .runtime
            .block_on(Route::from_gpx(
                &xml.replace("<ele>500</ele>", "<ele>501</ele>"),
                None,
            ))
            .unwrap();
        assert_eq!(changed.key(), route_key);
        app.route = Some(changed);
        assert!(app.continuation_point().is_none());
        assert_eq!(app.course_library(&query).groups, []);
        assert!(
            app.continue_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
                .is_err()
        );
        app.shutdown();
    }

    #[test]
    fn continuing_a_video_seeks_its_frames_and_keeps_todays_distance_separate() {
        let dir = temp_dir("continuation-video");
        let video = orqwa_video::testing::gopro_video("continuation");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_video(video.clone(), true);
        run_until(&mut app, |e| matches!(e, AppEvent::CourseAdded(_)));
        connect(&mut app);
        start(&mut app, false);
        advance(&mut app, 15);
        saved(&app.finish_ride_remembering(true));
        let point = app.continuation_point().unwrap();
        start(&mut app, true);
        let wanted = app
            .video
            .as_ref()
            .unwrap()
            .time_at(Meters(point.distance_m));
        assert!(wanted > Duration::ZERO);
        assert_eq!(app.video_time(), Some(wanted));
        assert_eq!(app.ride_state().unwrap().ridden, Meters(0.0));
        let mut frame = None;
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(10));
            app.update(Duration::ZERO);
            frame = app.video_frame().or(frame);
            if frame.as_ref().is_some_and(|frame| frame.time >= wanted) {
                break;
            }
        }
        assert!(
            frame.is_some_and(|frame| frame.time >= wanted),
            "video must seek to continued position"
        );
        app.abort_ride();
        app.shutdown();
        let _ = std::fs::remove_file(video);
    }

    #[test]
    fn an_unlocated_tacx_video_can_continue_by_its_distance_mapping() {
        let dir = temp_dir("continuation-tacx");
        let video = orqwa_video::testing::test_video("continuation-tacx", 64, 48);
        std::fs::copy(&video, dir.join("ride.mp4")).unwrap();
        std::fs::write(
            dir.join("ride.rlv"),
            orqwa_video::testing::rlv_bytes("ride.mp4"),
        )
        .unwrap();
        std::fs::write(
            dir.join("ride.pgmf"),
            orqwa_video::testing::pgmf_bytes("Ride"),
        )
        .unwrap();
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_video(dir.join("ride.rlv"), true);
        run_until(&mut app, |e| matches!(e, AppEvent::CourseAdded(_)));
        connect(&mut app);
        start(&mut app, false);
        advance(&mut app, 3);
        assert!(app.can_save_continuation());
        saved(&app.finish_ride_remembering(true));
        let point = app.continuation_point().unwrap();
        start(&mut app, true);
        assert!(!app.video.as_ref().unwrap().located);
        assert_eq!(app.ride_state().unwrap().distance, Meters(point.distance_m));
        assert_eq!(
            app.video_time(),
            Some(
                app.video
                    .as_ref()
                    .unwrap()
                    .time_at(Meters(point.distance_m))
            )
        );
        assert!(app.video_time().unwrap() > Duration::ZERO);
        app.abort_ride();
        app.shutdown();
        let _ = std::fs::remove_file(video);
    }
}
