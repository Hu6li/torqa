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
const CELL: f64 = 480.0;
/// Current two-chunks-per-core batches added about 0.10 GB on eight cores in Wada's
/// measurements; 0.016 GB/core rounds that up (docs/benchmarks/world-memory.md).
const BATCH_PER_CORE: u64 = 16 * MB;
const LOW_UPLOAD_RADIUS: Meters = Meters(3925.0);
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
    /// Rendering extent is independent of whether generation waits for the whole world.
    radius: Option<Meters>,
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
    pub(crate) fn radius(&self, generation: Meters) -> Meters {
        self.radius.unwrap_or_else(|| {
            if generation.0.is_finite() {
                generation
            } else {
                LOW_UPLOAD_RADIUS
            }
        })
    }
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
            .arg(gigabytes(info.available, false)));
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
        .arg(gigabytes(additional, true))
        .arg(gigabytes(info.available.saturating_sub(reserve), false))
        .arg(gigabytes(reserve, true)))
    }
}

fn gigabytes(bytes: u64, round_up: bool) -> String {
    const HUNDREDTH: u64 = 10_000_000;
    let hundredths = if round_up {
        bytes.div_ceil(HUNDREDTH)
    } else {
        bytes / HUNDREDTH
    };
    format!("{}.{:02} GB", hundredths / 100, hundredths % 100)
}

/// The next synchronous batch can use all cores even while a ride is paused. This is also
/// allowed for in the admission estimate, alongside the preparation and rendered scene.
pub(crate) fn batch_headroom() -> u64 {
    batch_bytes(std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get))
}

fn batch_bytes(cores: usize) -> u64 {
    (cores.max(1) as u64 * BATCH_PER_CORE).max(UPLOAD_HEADROOM)
}

/// Provisional additional-memory planning values, in bytes, not measured process peaks.
#[derive(Debug)]
pub struct Estimate {
    /// Whole-route preparation, before concurrent chunks or rendering.
    pub preparation: u64,
    /// A concurrent chunk batch on the requested number of cores.
    pub batch: u64,
    /// Resident rendered surroundings, including whole-route geometry.
    pub ride: u64,
    /// Preparation, concurrent batch and rendered surroundings together.
    pub build: u64,
}

/// Conservative planning coefficients, grounded in the measured Wada preparation/batch and
/// Low scene (docs/benchmarks/world-memory.md). They are not an allocation limit: checkpoints
/// still check live memory. Count resampled street length and footprint complexity, not the
/// compressed file or a universal course-length limit.
pub fn estimate(route: &Route, map: &MapData, radius: Meters, cores: usize) -> Estimate {
    // Finishing the entire build before hand-over does not render the entire course.
    let radius = if radius.0.is_finite() {
        radius
    } else {
        LOW_UPLOAD_RADIUS
    };
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
    // A square window counts distant towns together even when they cannot be resident at
    // once. Match the circular upload/eviction area, allowing a cell's position error on
    // both sides. Chunk-sized cells keep this rounding to a small margin.
    let scene = busiest(&cells, radius);
    // Admission reads free RAM after Godot and its renderer already exist: don't charge the
    // engine's roughly 0.3 GB baseline again. Keep extra room for terrain/vegetation and LOD
    // uploads, growing with the view area. The ridden road's six vertices per two metres
    // and index buffers need about 144 bytes/m on the GPU; 256 leaves conversion margin.
    let area = (radius.0 / LOW_UPLOAD_RADIUS.0).max(1.0).powi(2);
    let ride = bytes(area, UPLOAD_HEADROOM) + bytes(route.length().0, 256) + scene;
    tracing::info!(
        preparation,
        batch = batch_bytes(cores),
        scene = ride,
        radius = radius.0,
        cores,
        "estimated additional 3D memory; Android reserve is separate"
    );
    Estimate {
        preparation,
        batch: batch_bytes(cores),
        build: preparation + batch_bytes(cores) + ride,
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

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // bounded spatial bins
fn busiest(cells: &HashMap<(i64, i64), u64>, radius: Meters) -> u64 {
    let metres = radius.0.max(0.0) + 720.0 + CELL * std::f64::consts::SQRT_2;
    if metres > 30_000.0 {
        return cells.values().sum();
    }
    let reach = (metres / CELL).ceil() as i64;
    cells
        .keys()
        .map(|&(x, y)| {
            (x - reach..=x + reach)
                .flat_map(|a| (y - reach..=y + reach).map(move |b| (a, b)))
                .filter(|&(a, b)| ((a - x) as f64 * CELL).hypot((b - y) as f64 * CELL) <= metres)
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
            radius: None,
            sample: Arc::default(),
        });
    }

    /// Supplies the actual graphics upload radius independently of generation's hand-over
    /// radius. Finishing a complete course before riding still renders only nearby chunks.
    pub fn set_world_upload_radius(&mut self, radius: Meters) {
        if radius.0.is_finite()
            && radius.0 >= 0.0
            && let Some(guard) = &mut self.memory
        {
            guard.radius = Some(radius);
        }
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
            radius: None,
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
        assert_eq!(error.args, ["0.21 GB", "0.20 GB", "0.80 GB"]);

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

    #[test]
    fn memory_messages_separate_additional_ram_and_androids_reserve_in_gb() {
        let free = guard(Some(MemoryInfo {
            total: 11_650 * MB,
            ..phone(3591 * MB)
        }));
        assert!(
            free.check(2402 * MB, true).is_ok(),
            "corrected Wada estimate fits this snapshot"
        );
        let error = free.check(3523 * MB, true).unwrap_err();
        assert_eq!(error.args, ["3.53 GB", "2.42 GB", "1.17 GB"]);
        assert_eq!(gigabytes(1_000_000_000, true), "1.00 GB");
        assert_eq!(gigabytes(1_000_000_001, true), "1.01 GB");
        assert_eq!(gigabytes(1_000_000_001, false), "1.00 GB");
    }

    #[test]
    fn diagonal_towns_outside_the_resident_circle_are_not_added_together() {
        let mut cells = HashMap::new();
        charge(&mut cells, (0.0, 0.0), 100);
        charge(&mut cells, (5000.0, 5000.0), 1000);
        charge(&mut cells, (-5000.0, -5000.0), 1000);
        assert_eq!(busiest(&cells, LOW_UPLOAD_RADIUS), 1000);
    }

    #[tokio::test]
    async fn geometry_complexity_and_viewing_distance_change_the_estimate() {
        let route = Route::from_gpx(
            r#"<gpx><trk><trkseg><trkpt lat="46" lon="7"><ele>500</ele></trkpt><trkpt lat="46.001" lon="7"><ele>500</ele></trkpt></trkseg></trk></gpx>"#,
            None,
        ).await.unwrap();
        let empty = estimate(&route, &MapData::default(), Meters(1000.0), 8);
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
        let near = estimate(&route, &map, Meters(1000.0), 8);
        let far = estimate(&route, &map, Meters(6000.0), 8);
        let whole = estimate(&route, &map, crate::WHOLE_WORLD, 8);
        let low = estimate(&route, &map, LOW_UPLOAD_RADIUS, 8);
        assert_eq!(
            whole.build, low.build,
            "whole generation is not whole-course rendering"
        );
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
