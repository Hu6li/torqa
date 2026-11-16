//! Admission and allocation checkpoints for Android's shared CPU/GPU memory (R255).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use orqwa_domain::units::Meters;
use orqwa_osm::{LatLon, MapData};
use orqwa_routes::{LocalProjection, Route};

use crate::App;
use crate::failure::{Failure, Kind};

const MB: u64 = 1_000_000;
const CELL: f64 = 2000.0;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
/// Room for a chunk's deserialization, Godot arrays and mesh upload, beyond the OS reserve.
pub(crate) const UPLOAD_HEADROOM: u64 = 128 * MB;

/// Android's current system memory, not its Java heap limit or the APK/course file size.
#[derive(Debug, Clone, Copy)]
pub struct MemoryInfo {
    /// Bytes currently available to the system, including reclaimable memory.
    pub available: u64,
    /// Total bytes accessible to Android's kernel.
    pub total: u64,
    /// Android's available-memory threshold for considering the system low on memory.
    pub threshold: u64,
    /// Android already considers the system low on memory.
    pub low: bool,
}

/// Reads current memory. Failure to read it refuses 3D preparation rather than bypassing it.
pub type FreeMemory = Arc<dyn Fn() -> Option<MemoryInfo> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct Guard {
    free: FreeMemory,
    // Generation workers and the frame thread share one sample instead of querying Java for
    // every sink in a batch. Admission always takes a fresh sample after stopped jobs end.
    sample: Arc<Mutex<Option<Sample>>>,
}

#[derive(Clone, Copy)]
struct Sample {
    at: Instant,
    info: Option<MemoryInfo>,
}

impl Guard {
    fn read(&self, fresh: bool) -> Option<MemoryInfo> {
        let mut sample = self.sample.lock().unwrap_or_else(PoisonError::into_inner);
        if !fresh
            && let Some(Sample { at, info }) = *sample
            && at.elapsed() < SAMPLE_INTERVAL
        {
            return info;
        }
        let info = (self.free)();
        *sample = Some(Sample {
            at: Instant::now(),
            info,
        });
        info
    }

    pub(crate) fn check(&self, additional: u64, fresh: bool) -> Result<(), Failure> {
        let info = self.read(fresh).filter(|info| {
            info.total > 0 && info.available <= info.total && info.threshold < info.total
        });
        let Some(info) = info else {
            return Err(Failure::new(
                Kind::ReadMemory,
                "Android's current available memory could not be read",
            ));
        };
        // availMem includes reclaimable memory that Android itself needs. Keep at least
        // 512 MB, a tenth of RAM, and 256 MB above Android's own low-memory threshold.
        let reserve = (512 * MB)
            .max(info.total / 10)
            .max(info.threshold.saturating_add(256 * MB));
        let needed = additional.saturating_add(reserve);
        if fresh {
            tracing::info!(
                additional,
                reserve,
                needed,
                available = info.available,
                total = info.total,
                threshold = info.threshold,
                low = info.low,
                "checking Android 3D memory headroom"
            );
        }
        if info.low {
            return Err(Failure::new(
                Kind::MemoryPressure,
                format!(
                    "Android lowMemory is true, available {}, threshold {}",
                    info.available, info.threshold
                ),
            )
            .arg(format!("{} MB", info.available / MB)));
        }
        if info.available >= needed {
            return Ok(());
        }
        Err(Failure::new(
            Kind::WorldMemory,
            format!(
                "world needs {additional} more bytes, Android reserve {reserve}, available {}, total {}, threshold {}, low {}",
                info.available, info.total, info.threshold, info.low
            ),
        )
        .arg(format!("{} MB", needed.div_ceil(MB)))
        .arg(format!("{} MB", info.available / MB)))
    }
}

/// The next synchronous batch can use all cores even while a ride is paused. This is also
/// allowed for in the admission estimate, alongside the preparation and rendered scene.
pub(crate) fn batch_headroom() -> u64 {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get) as u64 * 64 * MB
}

pub(crate) struct Estimate {
    pub(crate) build: u64,
    pub(crate) ride: u64,
}

/// Conservative planning coefficients, grounded in the measured Wada preparation/batch and
/// Low scene (docs/benchmarks/world-memory.md). They are not an allocation limit: checkpoints
/// still check live memory. Count resampled street length and footprint complexity, not the
/// compressed file or a universal course-length limit.
#[allow(clippy::cast_possible_truncation)] // the finite coarse-cell reach is at most 8
pub(crate) fn estimate(route: &Route, map: &MapData, radius: Meters) -> Estimate {
    let projection = LocalProjection::for_route(route);
    let mut cells = HashMap::new();
    let route_bytes = bytes(route.length().0, 1024) + route.points().len() as u64 * 256;
    let mut preparation = 192 * MB + route_bytes;
    for road in &map.roads {
        preparation += line(&road.line, &projection, &mut cells);
    }
    for rail in &map.railways {
        preparation += line(&rail.line, &projection, &mut cells);
    }
    for building in &map.buildings {
        let points = building.outline.len() as u64;
        preparation += points * 512;
        if let Some(&(lat, lon)) = building.outline.first() {
            charge(&mut cells, projection.project(lat, lon), points * 1024);
        }
    }
    for area in &map.areas {
        for ring in area.outer.iter().chain(&area.inner) {
            preparation += ring.len() as u64 * 128;
            if let Some(&(lat, lon)) = ring.first() {
                charge(
                    &mut cells,
                    projection.project(lat, lon),
                    ring.len() as u64 * 256,
                );
            }
        }
    }
    // Include the ridden road even where the map has no streets.
    for pair in route.points().windows(2) {
        let a = projection.project(pair[0].lat, pair[0].lon);
        let b = projection.project(pair[1].lat, pair[1].lon);
        charge_segment(&mut cells, a, b, 1024);
    }
    // Bound the busiest surroundings anywhere on the course, including the eviction margin,
    // rather than assuming the start is its densest part. Coarse squares overestimate a disk.
    let reach = ((radius.0 + 720.0) / CELL).ceil();
    let scene = if !reach.is_finite() || reach > 8.0 {
        cells.values().sum()
    } else {
        busiest(&cells, reach.max(0.0) as i64 + 1)
    };
    // Terrain, vegetation, render targets and LOD work grow with the view's area even in an
    // empty map. 3,925 m is the measured Low upload radius, including its movement margin.
    let area = if radius.0.is_finite() {
        (radius.0 / 3925.0).max(1.0).powi(2)
    } else {
        1.0
    };
    let ride = bytes(area, 384 * MB) + route_bytes + scene;
    Estimate {
        build: preparation + batch_headroom() + ride,
        ride,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // size estimates saturate on conversion
fn bytes(metres: f64, per_metre: u64) -> u64 {
    (metres.max(0.0) * per_metre as f64).ceil() as u64
}

fn line(
    points: &[LatLon],
    projection: &LocalProjection,
    cells: &mut HashMap<(i64, i64), u64>,
) -> u64 {
    let mut preparation = points.len() as u64 * 128;
    for pair in points.windows(2) {
        let a = projection.project(pair[0].0, pair[0].1);
        let b = projection.project(pair[1].0, pair[1].1);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        preparation += bytes(length, 256);
        charge_segment(cells, a, b, 512);
    }
    preparation
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn charge_segment(
    cells: &mut HashMap<(i64, i64), u64>,
    a: (f64, f64),
    b: (f64, f64),
    per_metre: u64,
) {
    let length = (b.0 - a.0).hypot(b.1 - a.1);
    let parts = (length / CELL).ceil().max(1.0) as u64;
    for part in 0..parts {
        let t = (part as f64 + 0.5) / parts as f64;
        charge(
            cells,
            (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)),
            bytes(length, per_metre).div_ceil(parts),
        );
    }
}

#[allow(clippy::cast_possible_truncation)] // projected cell coordinates
fn charge(cells: &mut HashMap<(i64, i64), u64>, at: (f64, f64), bytes: u64) {
    *cells
        .entry(((at.0 / CELL).floor() as i64, (at.1 / CELL).floor() as i64))
        .or_default() += bytes;
}

fn busiest(cells: &HashMap<(i64, i64), u64>, reach: i64) -> u64 {
    cells
        .keys()
        .map(|&(x, y)| {
            (x - reach..=x + reach)
                .flat_map(|a| (y - reach..=y + reach).map(move |b| (a, b)))
                .filter_map(|cell| cells.get(&cell))
                .sum()
        })
        .max()
        .unwrap_or(0)
}

impl App {
    /// Enables Android's live-memory admission and allocation checks. Other platforms do not
    /// install this callback. An unavailable reading refuses 3D rather than failing open.
    pub fn check_free_memory(&mut self, free: FreeMemory) {
        self.memory = Some(Guard {
            free,
            sample: Arc::default(),
        });
    }

    /// Checks headroom before scene uploads, including after generation has finished. A failed
    /// check stops generation, pauses only the local 3D ride and preserves its recording.
    pub fn check_world_memory(&mut self) -> bool {
        if self.world_failed() {
            return false;
        }
        let result = self
            .memory
            .as_ref()
            .map_or(Ok(()), |guard| guard.check(UPLOAD_HEADROOM, false));
        if let Err(failure) = result {
            self.stop_background();
            if let Some(pace) = self.pace.take() {
                pace.stop();
            }
            if let Some(job) = self.loading.take() {
                let slot = self.builds.building.take();
                self.stop_build(job, slot);
            }
            self.world_build_failed();
            let _ = self.jobs_tx.send(crate::JobResult::Failed(failure));
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(info: Option<MemoryInfo>) -> Guard {
        Guard {
            free: Arc::new(move || info),
            sample: Arc::default(),
        }
    }

    fn phone(available: u64) -> MemoryInfo {
        MemoryInfo {
            available,
            total: 8_000 * MB,
            threshold: 200 * MB,
            low: false,
        }
    }

    #[test]
    fn admission_preserves_system_headroom_instead_of_spending_all_available_ram() {
        let free = guard(Some(phone(1_000 * MB)));
        assert!(free.check(200 * MB, true).is_ok());
        let error = free.check(201 * MB, true).unwrap_err();
        assert_eq!(error.kind, Kind::WorldMemory);
        assert_eq!(error.args, ["1001 MB", "1000 MB"]);

        let free = guard(Some(MemoryInfo {
            threshold: 900 * MB,
            ..phone(1_200 * MB)
        }));
        assert!(
            free.check(100 * MB, true).is_err(),
            "reserve exceeds Android's threshold"
        );
        let free = guard(Some(MemoryInfo {
            low: true,
            ..phone(7_000 * MB)
        }));
        assert!(
            free.check(0, true).is_err(),
            "Android's low-memory flag is authoritative"
        );
    }

    #[test]
    fn unknown_or_invalid_memory_refuses_preparation() {
        for info in [
            None,
            Some(MemoryInfo {
                total: 0,
                ..phone(0)
            }),
            Some(phone(9_000 * MB)),
        ] {
            assert_eq!(
                guard(info).check(0, true).unwrap_err().kind,
                Kind::ReadMemory
            );
        }
    }

    #[tokio::test]
    async fn geometry_complexity_and_viewing_distance_change_the_estimate() {
        let route = Route::from_gpx(
            r#"<gpx><trk><trkseg><trkpt lat="46" lon="7"><ele>500</ele></trkpt><trkpt lat="46.001" lon="7"><ele>500</ele></trkpt></trkseg></trk></gpx>"#,
            None,
        ).await.unwrap();
        let empty = estimate(&route, &MapData::default(), Meters(1000.0));
        let mut map = MapData::default();
        for i in 0..30 {
            map.buildings.push(orqwa_osm::Building {
                id: i64::from(i),
                outline: vec![(46.0 + f64::from(i) / 100.0, 7.0); 200],
                height: None,
                levels: None,
                color: None,
            });
        }
        let near = estimate(&route, &map, Meters(1000.0));
        let far = estimate(&route, &map, Meters(6000.0));
        assert!(near.build > empty.build, "same route, more complex map");
        assert!(
            far.ride > near.ride,
            "more geometry resident with a larger view"
        );
        assert!(
            far.build > far.ride,
            "generation overlaps the rendered scene"
        );
    }
}
