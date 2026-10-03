//! Routes for Torqa: GPX import (R11), elevation correction and smoothing (R12) and position,
//! elevation and gradient lookup along the route.

pub mod climbs;
mod gpx;
mod projection;
mod structures;

pub use climbs::{Climb, ClimbCategory};
pub use projection::LocalProjection;

use torqa_domain::units::{GradePercent, Meters};
use torqa_osm::Structure;
use torqa_terrain::Terrain;
use tracing::warn;

use gpx::RawPoint;

/// Distance between resampled route points.
const SPACING: f64 = 10.0;
/// Moving-average window for terrain-model elevations, which are already clean.
const TERRAIN_SMOOTHING: f64 = 40.0;
/// Moving-average window for recorded elevations, which are noisy (GPS, barometer drift).
const GPX_SMOOTHING: f64 = 100.0;
const EARTH_RADIUS: f64 = 6_371_000.0;

/// Errors while importing a route.
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    /// The file is not valid GPX.
    #[error("invalid GPX: {0}")]
    InvalidGpx(String),
    /// The file has fewer than two distinct points.
    #[error("route needs at least two distinct points")]
    TooShort,
    /// Neither the file nor the terrain model provided elevations.
    #[error("route has no elevation data and the terrain model is unavailable")]
    NoElevation,
}

/// The positions of a GPX file as (latitude, longitude), without building a route; for
/// fetching data along it before importing.
///
/// # Errors
/// [`RouteError::InvalidGpx`] if the file is not valid GPX.
pub fn track_points(xml: &str) -> Result<Vec<(f64, f64)>, RouteError> {
    Ok(gpx::parse(xml)?
        .points
        .iter()
        .map(|p| (p.lat, p.lon))
        .collect())
}

/// Where a route's elevations come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationSource {
    /// Terrain model (corrected, R12).
    Terrain,
    /// Elevations recorded in the file.
    File,
}

/// A point of the resampled route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutePoint {
    /// Latitude in degrees (WGS84).
    pub lat: f64,
    /// Longitude in degrees (WGS84).
    pub lon: f64,
    /// Smoothed elevation.
    pub elevation: Meters,
    /// Distance from the start along the route.
    pub distance: Meters,
    /// What carries the road here.
    pub surface: Surface,
}

/// What carries the road at a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Surface {
    /// The ground.
    #[default]
    Ground,
    /// A bridge: the road runs above the terrain.
    Bridge,
    /// A tunnel: the road runs below the terrain.
    Tunnel,
}

/// Where a rider is on the route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutePosition {
    /// Latitude in degrees (WGS84).
    pub lat: f64,
    /// Longitude in degrees (WGS84).
    pub lon: f64,
    /// Elevation.
    pub elevation: Meters,
    /// Gradient of the road at this point.
    pub grade: GradePercent,
    /// Direction of travel in radians, clockwise from north.
    pub heading: f64,
}

/// A route ready to ride: evenly resampled, with smoothed elevations.
#[derive(Debug, Clone)]
pub struct Route {
    name: Option<String>,
    points: Vec<RoutePoint>,
    elevation_source: ElevationSource,
    climbs: Vec<Climb>,
}

impl Route {
    /// Imports a GPX route.
    ///
    /// With a `terrain`, elevations come from the terrain model; if it cannot provide them (e.g.
    /// offline without cached tiles), the file's elevations are used instead.
    ///
    /// # Errors
    /// [`RouteError`] if the file is invalid, too short, or no elevations are available.
    pub async fn from_gpx(xml: &str, terrain: Option<&mut Terrain>) -> Result<Self, RouteError> {
        Self::from_gpx_with(xml, terrain, &[]).await
    }

    /// Like [`Route::from_gpx`], with any [`ElevationModel`] and the road `structures`
    /// (bridges, tunnels) along the route: there the elevation runs straight from one end to the
    /// other instead of following the ground (or water) below or the mountain above.
    ///
    /// # Errors
    /// [`RouteError`] if the file is invalid, too short, or no elevations are available.
    pub async fn from_gpx_with<M: ElevationModel>(
        xml: &str,
        model: Option<&mut M>,
        structures: &[Structure],
    ) -> Result<Self, RouteError> {
        let gpx = gpx::parse(xml)?;
        let mut track = dedup(gpx.points);

        // The model is sampled only at the file's points, which lie on the road. Between
        // sparse points a straight line can cut across a hillside, so elevations there are
        // interpolated rather than sampled.
        let mut source = None;
        if let Some(model) = model {
            match model_elevations(&track, model).await {
                Ok(elevations) => {
                    for (point, elevation) in track.iter_mut().zip(elevations) {
                        point.elevation = Some(elevation);
                    }
                    source = Some(ElevationSource::Terrain);
                }
                Err(error) => warn!(%error, "terrain model unavailable, using file elevations"),
            }
        }
        let source = match source {
            Some(source) => source,
            None if fill_gaps(&mut track) => ElevationSource::File,
            None if track.len() < 2 => return Err(RouteError::TooShort),
            None => return Err(RouteError::NoElevation),
        };
        let mut points = resample(&track)?;
        let surfaces = structures::surfaces(&points, structures);
        structures::bridge_elevations(&mut points, &surfaces);

        let window = match source {
            ElevationSource::Terrain => TERRAIN_SMOOTHING,
            ElevationSource::File => GPX_SMOOTHING,
        };
        let raw: Vec<f64> = points
            .iter()
            .map(|p| p.elevation.unwrap_or_default())
            .collect();
        let smoothed = smooth(&raw, window);

        let mut distance = 0.0;
        let points = points
            .iter()
            .zip(smoothed)
            .zip(surfaces)
            .enumerate()
            .map(|(i, ((p, elevation), surface))| {
                if i > 0 {
                    distance += haversine(&points[i - 1], p);
                }
                RoutePoint {
                    lat: p.lat,
                    lon: p.lon,
                    elevation: Meters(elevation),
                    distance: Meters(distance),
                    surface,
                }
            })
            .collect();

        let points: Vec<RoutePoint> = points;
        Ok(Self {
            name: gpx.name,
            climbs: climbs::detect(&points),
            points,
            elevation_source: source,
        })
    }

    /// Name from the file, if any.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Where the elevations come from.
    #[must_use]
    pub fn elevation_source(&self) -> ElevationSource {
        self.elevation_source
    }

    /// The resampled points.
    #[must_use]
    pub fn points(&self) -> &[RoutePoint] {
        &self.points
    }

    /// The climbs along the route, in order.
    #[must_use]
    pub fn climbs(&self) -> &[Climb] {
        &self.climbs
    }

    /// A fingerprint of the route's course: equal for the same track ridden again (e.g. the same
    /// GPX file or course), so rides on it can be compared. Ignores name and elevations.
    #[must_use]
    pub fn key(&self) -> String {
        // FNV-1a over the position every 100 m, rounded to about 10 m.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |value: i64| {
            for byte in value.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        };
        #[allow(clippy::cast_possible_truncation)] // degrees × 10⁴ fit easily
        for point in self.points.iter().step_by(10) {
            feed((point.lat * 1e4).round() as i64);
            feed((point.lon * 1e4).round() as i64);
        }
        feed(i64::try_from(self.points.len()).unwrap_or(i64::MAX));
        format!("{hash:016x}")
    }

    /// Total length.
    #[must_use]
    pub fn length(&self) -> Meters {
        self.points.last().map_or(Meters(0.0), |p| p.distance)
    }

    /// Sum of all climbs.
    #[must_use]
    pub fn elevation_gain(&self) -> Meters {
        Meters(
            self.points
                .windows(2)
                .map(|w| (w[1].elevation.0 - w[0].elevation.0).max(0.0))
                .sum(),
        )
    }

    /// The steepest climbing gradient anywhere on the route.
    #[must_use]
    pub fn max_grade(&self) -> GradePercent {
        let steepest = self
            .points
            .windows(2)
            .filter(|w| w[1].distance.0 > w[0].distance.0)
            .map(|w| (w[1].elevation.0 - w[0].elevation.0) / (w[1].distance.0 - w[0].distance.0))
            .fold(0.0, f64::max);
        GradePercent(steepest * 100.0)
    }

    /// Position, elevation and gradient at a distance from the start (clamped to the route).
    #[must_use]
    pub fn position(&self, distance: Meters) -> RoutePosition {
        let along = distance.0.clamp(0.0, self.length().0);
        // Index of the segment containing `along`; the route always has at least two points.
        let index = self
            .points
            .partition_point(|p| p.distance.0 <= along)
            .clamp(1, self.points.len() - 1)
            - 1;
        let (start, end) = (&self.points[index], &self.points[index + 1]);
        let span = end.distance.0 - start.distance.0;
        let fraction = if span > 0.0 {
            (along - start.distance.0) / span
        } else {
            0.0
        };
        let rise = end.elevation.0 - start.elevation.0;
        RoutePosition {
            lat: start.lat + (end.lat - start.lat) * fraction,
            lon: start.lon + (end.lon - start.lon) * fraction,
            elevation: Meters(start.elevation.0 + rise * fraction),
            grade: GradePercent(if span > 0.0 { rise / span * 100.0 } else { 0.0 }),
            heading: heading(start, end),
        }
    }
}

/// Direction from `a` to `b` in radians, clockwise from north (flat-earth approximation,
/// exact enough over a 10 m segment).
fn heading(a: &RoutePoint, b: &RoutePoint) -> f64 {
    let east = (b.lon - a.lon) * a.lat.to_radians().cos();
    let north = b.lat - a.lat;
    east.atan2(north)
}

/// Removes consecutive points closer than 10 cm, which would create zero-length segments.
fn dedup(points: Vec<RawPoint>) -> Vec<RawPoint> {
    let mut result: Vec<RawPoint> = Vec::with_capacity(points.len());
    for point in points {
        match result.last_mut() {
            Some(last) if haversine(last, &point) < 0.1 => {
                if last.elevation.is_none() {
                    last.elevation = point.elevation;
                }
            }
            _ => result.push(point),
        }
    }
    result
}

/// Resamples the polyline every [`SPACING`] metres, keeping the exact end point. Elevations are
/// interpolated between known values; unknown ones stay `None`.
fn resample(points: &[RawPoint]) -> Result<Vec<RawPoint>, RouteError> {
    if points.len() < 2 {
        return Err(RouteError::TooShort);
    }
    let mut result = vec![points[0]];
    let mut next = SPACING; // distance of the next sample from the start
    let mut travelled = 0.0;
    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let length = haversine(a, b);
        while next <= travelled + length {
            let t = (next - travelled) / length;
            result.push(RawPoint {
                lat: a.lat + (b.lat - a.lat) * t,
                lon: a.lon + (b.lon - a.lon) * t,
                elevation: a
                    .elevation
                    .zip(b.elevation)
                    .map(|(ea, eb)| ea + (eb - ea) * t),
            });
            next += SPACING;
        }
        travelled += length;
    }
    let end = points[points.len() - 1];
    if result
        .last()
        .is_some_and(|last| haversine(last, &end) >= 0.1)
    {
        result.push(end);
    }
    Ok(result)
}

/// A source of ground elevations, such as the terrain model.
pub trait ElevationModel {
    /// Ground elevation in metres at a WGS84 position.
    fn elevation(
        &mut self,
        lat: f64,
        lon: f64,
    ) -> impl std::future::Future<Output = Result<f64, String>> + Send;
}

impl ElevationModel for Terrain {
    async fn elevation(&mut self, lat: f64, lon: f64) -> Result<f64, String> {
        Terrain::elevation(self, lat, lon)
            .await
            .map_err(|e| e.to_string())
    }
}

async fn model_elevations<M: ElevationModel>(
    points: &[RawPoint],
    model: &mut M,
) -> Result<Vec<f64>, String> {
    let mut elevations = Vec::with_capacity(points.len());
    for point in points {
        elevations.push(model.elevation(point.lat, point.lon).await?);
    }
    Ok(elevations)
}

/// Fills missing elevations by linear interpolation (nearest value at the ends).
/// Returns `false` if there is no elevation at all.
fn fill_gaps(points: &mut [RawPoint]) -> bool {
    let known: Vec<usize> = (0..points.len())
        .filter(|&i| points[i].elevation.is_some())
        .collect();
    let (Some(&first), Some(&last)) = (known.first(), known.last()) else {
        return false;
    };
    let value = |points: &[RawPoint], i: usize| points[i].elevation.unwrap_or_default();
    let (start, end) = (value(points, first), value(points, last));
    for point in &mut points[..first] {
        point.elevation = Some(start);
    }
    for point in &mut points[last + 1..] {
        point.elevation = Some(end);
    }
    for pair in known.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (ea, eb) = (value(points, a), value(points, b));
        for (offset, point) in points[a + 1..b].iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)] // indices are far below 2^52
            let t = (offset + 1) as f64 / (b - a) as f64;
            point.elevation = Some(ea + (eb - ea) * t);
        }
    }
    true
}

/// Smooths evenly spaced samples with two passes of a centred moving average over `window`
/// metres, which approximates a Gaussian and leaves far less ripple than a single pass.
fn smooth(values: &[f64], window: f64) -> Vec<f64> {
    moving_average(&moving_average(values, window), window)
}

/// Centred moving average. Near the ends the window shrinks symmetrically, so a constant
/// gradient is preserved exactly and the end points keep their elevation.
fn moving_average(values: &[f64], window: f64) -> Vec<f64> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small positive count
    let half = ((window / SPACING / 2.0).round() as usize).max(1);
    let last = values.len().saturating_sub(1);
    (0..values.len())
        .map(|i| {
            let h = half.min(i).min(last - i);
            let range = &values[i - h..=i + h];
            #[allow(clippy::cast_precision_loss)]
            let count = range.len() as f64;
            range.iter().sum::<f64>() / count
        })
        .collect()
}

/// Great-circle distance in metres.
fn haversine(a: &RawPoint, b: &RawPoint) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// A straight track due north with the given elevations, `step` metres apart.
    fn gpx_north(elevations: &[Option<f64>], step: f64) -> String {
        let degrees_per_meter = 1.0 / (EARTH_RADIUS.to_radians());
        let mut points = String::new();
        for (i, elevation) in elevations.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let lat = 46.0 + i as f64 * step * degrees_per_meter;
            let ele = elevation
                .map(|e| format!("<ele>{e}</ele>"))
                .unwrap_or_default();
            let _ = write!(points, r#"<trkpt lat="{lat}" lon="7.0">{ele}</trkpt>"#);
        }
        format!("<gpx><trk><trkseg>{points}</trkseg></trk></gpx>")
    }

    async fn import(xml: &str) -> Route {
        Route::from_gpx(xml, None).await.unwrap()
    }

    #[tokio::test]
    async fn routes_know_their_climbs() {
        // 1 km flat, 2 km at 5 %, 1 km flat.
        let elevations: Vec<_> = (0..=40)
            .map(|i| Some(f64::from(i.clamp(10, 30) - 10) * 5.0))
            .collect();
        let route = import(&gpx_north(&elevations, 100.0)).await;

        let climbs = route.climbs();
        assert_eq!(climbs.len(), 1, "{climbs:?}");
        assert!((climbs[0].gain.0 - 100.0).abs() < 5.0, "{climbs:?}");
        // Smoothing the file's elevations rounds both ends of the climb by up to a window.
        assert!(
            (climbs[0].length().0 - 2000.0).abs() <= GPX_SMOOTHING * 2.0 + 1.0,
            "{climbs:?}"
        );
    }

    #[tokio::test]
    async fn the_same_track_has_the_same_key() {
        let track = gpx_north(&[Some(0.0); 11], 100.0);
        let renamed = track.replace("<trk>", "<trk><name>Other name</name>");
        let other = gpx_north(&[Some(0.0); 12], 100.0);

        let key = import(&track).await.key();

        assert_eq!(key, import(&renamed).await.key());
        assert_ne!(key, import(&other).await.key());
    }

    #[tokio::test]
    async fn measures_length_along_the_track() {
        let route = import(&gpx_north(&[Some(0.0); 11], 100.0)).await;

        assert!(
            (route.length().0 - 1000.0).abs() < 0.5,
            "{:?}",
            route.length()
        );
        assert_eq!(route.elevation_source(), ElevationSource::File);
    }

    #[tokio::test]
    async fn reports_grade_of_a_steady_climb() {
        // 5 % for 2 km: smoothing must not change a constant gradient.
        let elevations: Vec<_> = (0..=20).map(|i| Some(f64::from(i) * 5.0)).collect();
        let route = import(&gpx_north(&elevations, 100.0)).await;

        let middle = route.position(Meters(1000.0));
        assert!((middle.grade.0 - 5.0).abs() < 0.05, "{:?}", middle.grade);
        assert!(
            (middle.elevation.0 - 50.0).abs() < 0.5,
            "{:?}",
            middle.elevation
        );
        assert!((route.elevation_gain().0 - 100.0).abs() < 2.0);
    }

    #[tokio::test]
    async fn smoothing_removes_elevation_noise() {
        // Flat road with ±3 m GPS noise every 10 m would read as ±60 % spikes unsmoothed.
        let elevations: Vec<_> = (0..200)
            .map(|i| Some(if i % 2 == 0 { 503.0 } else { 497.0 }))
            .collect();
        let route = import(&gpx_north(&elevations, 10.0)).await;

        let max_grade = (0..19)
            .map(|k| {
                route
                    .position(Meters(100.0 + f64::from(k) * 90.0))
                    .grade
                    .0
                    .abs()
            })
            .fold(0.0, f64::max);
        assert!(max_grade < 1.0, "max grade {max_grade}");
    }

    #[tokio::test]
    async fn interpolates_missing_elevations() {
        let route = import(&gpx_north(&[Some(100.0), None, None, Some(130.0)], 100.0)).await;

        assert!((route.position(Meters(150.0)).elevation.0 - 115.0).abs() < 1.0);
    }

    #[tokio::test]
    async fn position_is_clamped_to_the_route() {
        let route = import(&gpx_north(&[Some(10.0), Some(20.0)], 100.0)).await;

        assert_eq!(route.position(Meters(-5.0)).lat, route.points()[0].lat);
        let end = route.position(Meters(1e6));
        assert!((end.lat - route.points().last().unwrap().lat).abs() < 1e-12);
    }

    #[tokio::test]
    async fn needs_elevation_and_two_points() {
        assert!(matches!(
            Route::from_gpx(&gpx_north(&[None, None], 100.0), None).await,
            Err(RouteError::NoElevation)
        ));
        assert!(matches!(
            Route::from_gpx(&gpx_north(&[Some(1.0)], 100.0), None).await,
            Err(RouteError::TooShort)
        ));
    }

    /// Terrain with a 200 m ridge between latitudes 46.000 and 46.009, everywhere else 500 m.
    struct Ridge;

    impl ElevationModel for Ridge {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let t = ((lat - 46.0) / 0.009).clamp(0.0, 1.0);
            std::future::ready(Ok(500.0 + 200.0 * (t * std::f64::consts::PI).sin()))
        }
    }

    #[tokio::test]
    async fn sparse_points_do_not_cut_across_the_terrain() {
        // Two points 1 km apart on a road that goes around the ridge, not over it.
        let xml = r#"<gpx><trk><trkseg>
            <trkpt lat="46.0" lon="7.0"/><trkpt lat="46.009" lon="7.0"/>
        </trkseg></trk></gpx>"#;

        let route = Route::from_gpx_with(xml, Some(&mut Ridge), &[])
            .await
            .unwrap();

        assert_eq!(route.elevation_source(), ElevationSource::Terrain);
        assert!(route.max_grade().0 < 0.5, "{:?}", route.max_grade());
        assert!(route.elevation_gain().0 < 1.0);
    }

    #[tokio::test]
    async fn reports_the_steepest_gradient() {
        let elevations: Vec<_> = [0.0, 0.0, 3.0, 13.0, 13.0, 13.0, 13.0]
            .into_iter()
            .map(Some)
            .collect();
        let route = import(&gpx_north(&elevations, 100.0)).await;

        // 10 m over 100 m, softened a little by smoothing.
        let max = route.max_grade().0;
        assert!((7.0..10.5).contains(&max), "{max}");
    }

    #[tokio::test]
    async fn heading_points_along_the_road() {
        let north = import(&gpx_north(&[Some(0.0), Some(0.0)], 100.0)).await;

        assert!(north.position(Meters(50.0)).heading.abs() < 1e-6);
    }

    /// A valley 60 m deep in the middle of a 1 km route due north at 500 m.
    struct Valley;

    impl ElevationModel for Valley {
        fn elevation(
            &mut self,
            lat: f64,
            _lon: f64,
        ) -> impl std::future::Future<Output = Result<f64, String>> + Send {
            let north = (lat - 46.0) * 111_195.0;
            let depth = if (300.0..700.0).contains(&north) {
                60.0
            } else {
                0.0
            };
            std::future::ready(Ok(500.0 - depth))
        }
    }

    /// A densely recorded track due north (a point every 10 m), as from a bike computer.
    fn dense_track_north() -> String {
        gpx_north(&[None; 101], 10.0)
    }

    fn line_north(from_m: f64, to_m: f64, lon: f64) -> Vec<(f64, f64)> {
        vec![
            (46.0 + from_m / 111_195.0, lon),
            (46.0 + to_m / 111_195.0, lon),
        ]
    }

    #[tokio::test]
    async fn bridges_carry_the_road_straight_across_valleys() {
        let bridge = Structure {
            kind: torqa_osm::StructureKind::Bridge,
            line: line_north(280.0, 720.0, 7.0),
        };

        let route = Route::from_gpx_with(&dense_track_north(), Some(&mut Valley), &[bridge])
            .await
            .unwrap();

        assert!(route.max_grade().0 < 1.0, "{:?}", route.max_grade());
        assert_eq!(route.position(Meters(500.0)).elevation, Meters(500.0));
        let middle = route
            .points()
            .iter()
            .find(|p| p.distance.0 >= 500.0)
            .unwrap();
        assert_eq!(middle.surface, Surface::Bridge);
        assert_eq!(route.points()[0].surface, Surface::Ground);
    }

    #[tokio::test]
    async fn without_the_bridge_the_route_dips_into_the_valley() {
        let route = Route::from_gpx_with(&dense_track_north(), Some(&mut Valley), &[])
            .await
            .unwrap();

        assert!(route.position(Meters(500.0)).elevation.0 < 450.0);
    }

    #[tokio::test]
    async fn roads_crossing_above_are_not_the_route() {
        // A bridge running east-west over the route.
        let crossing = Structure {
            kind: torqa_osm::StructureKind::Bridge,
            line: vec![
                (46.0 + 500.0 / 111_195.0, 6.999),
                (46.0 + 500.0 / 111_195.0, 7.001),
            ],
        };

        let route = Route::from_gpx_with(&dense_track_north(), Some(&mut Valley), &[crossing])
            .await
            .unwrap();

        assert!(route.points().iter().all(|p| p.surface == Surface::Ground));
    }
}
