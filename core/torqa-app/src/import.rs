//! Importing a route with everything needed to ride it.

use std::path::Path;

use torqa_domain::files::UsedFiles;
use torqa_osm::{MapData, Osm};
use torqa_routes::{ElevationModel, LocalProjection, Route};
use torqa_terrain::{Terrain, TileSource};
use torqa_world::course_map::{self, CourseMap, Frame};
use tracing::warn;

/// A step of preparing a course, for progress display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStage {
    /// Reading the GPX file.
    Route,
    /// Downloading (or reading cached) map data.
    Map,
    /// Correcting elevations with the terrain model.
    Elevation,
    /// Downloading (or reading cached) map data for the course's picture.
    CourseMap,
    /// Building the 3D world.
    World,
}

impl LoadStage {
    /// What the step does, for display.
    #[must_use]
    pub fn label(self) -> &'static str {
        // i18n-begin: translated by the front end.
        match self {
            Self::Route => "Reading route",
            Self::Map => "Downloading map data",
            Self::Elevation => "Correcting elevations",
            Self::CourseMap => "Downloading the course map",
            Self::World => "Building 3D world",
        }
        // i18n-end
    }

    /// What the step counts, for display.
    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::Route => "files",
            Self::Map | Self::CourseMap => "tiles",
            Self::Elevation => "points",
            Self::World => "chunks",
        }
    }
}

/// Receives (stage, done, total) while a course is being prepared.
pub type Progress<'a> = &'a mut (dyn FnMut(LoadStage, usize, usize) + Send);

/// A route ready to ride, with the map data around it.
pub struct Imported {
    /// The route.
    pub route: Route,
    /// OpenStreetMap features around the route (empty if unavailable).
    pub map: MapData,
    /// Name from the file, or the file name.
    pub name: String,
    /// The original GPX track, kept for saving the course.
    pub gpx: String,
    /// The map for the course's picture, when it is to get one ([`course_map`]).
    pub course_map: Option<Box<CourseMap>>,
}

/// Reads a GPX file and imports it with [`import_gpx`], named after the file if the track has
/// no name.
///
/// # Errors
/// A readable message if the file cannot be read or imported.
pub async fn import_route(
    path: &Path,
    cache_dir: &Path,
    offline: bool,
    used: &UsedFiles,
    progress: Progress<'_>,
) -> Result<Imported, String> {
    progress(LoadStage::Route, 0, 1);
    let xml = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let fallback = path
        .file_stem()
        .map_or_else(|| "Route".to_owned(), |s| s.to_string_lossy().into_owned());
    import_gpx(xml, &fallback, (cache_dir, offline), false, used, progress)
        .await
        .map_err(|e| format!("cannot import {}: {e}", path.display()))
}

/// Imports a GPX track: fetches map data along it (bridges and tunnels shape the elevation
/// profile) and corrects elevations with the terrain model. Downloads are cached under
/// `cache_dir` and every cached file used is recorded in `used`; `offline` uses the caches
/// only. Without a `video`, the route is ridden in 3D and loses its turns in place
/// ([`Route::without_turns_in_place`]); a video keeps the track it was recorded along.
///
/// # Errors
/// A readable message if the track cannot be imported.
pub async fn import_gpx(
    xml: String,
    fallback_name: &str,
    (cache_dir, offline): (&Path, bool),
    video: bool,
    used: &UsedFiles,
    progress: Progress<'_>,
) -> Result<Imported, String> {
    let track = torqa_routes::track_points(&xml).map_err(|e| e.to_string())?;
    progress(LoadStage::Route, 1, 1);

    let mut osm = Osm::new(cache_dir.join("osm")).recording(used.clone());
    if offline {
        osm = osm.offline();
    }
    let map = osm
        .around(&track, torqa_world::CORRIDOR, &mut |done, total| {
            progress(LoadStage::Map, done, total);
        })
        .await
        .unwrap_or_else(|error| {
            warn!(%error, "no map data; bridges and tunnels follow the terrain");
            MapData::default()
        });

    let mut terrain =
        Terrain::new(TileSource::defaults(), cache_dir.join("terrain")).recording(used.clone());
    if offline {
        terrain = terrain.offline();
    }
    let mut counting = Counting {
        inner: &mut terrain,
        done: 0,
        total: track.len(),
        progress: &mut *progress,
    };
    let mut route = Route::from_gpx_with(&xml, Some(&mut counting), &map)
        .await
        .map_err(|e| e.to_string())?;
    if !video {
        route = route.without_turns_in_place();
    }
    progress(LoadStage::Elevation, track.len(), track.len());
    let name = route
        .name()
        .map_or_else(|| fallback_name.to_owned(), ToOwned::to_owned);
    Ok(Imported {
        route,
        map,
        name,
        gpx: xml,
        course_map: None,
    })
}

/// The map of the land around the whole of `route` for the course's picture (#203), from map
/// tiles as detailed as the picture needs. Downloads are cached under `cache_dir` but not
/// recorded for the course file, which keeps the picture instead; `offline` uses the cache
/// only. `None` if no tile is available.
pub async fn course_map(
    route: &Route,
    cache_dir: &Path,
    offline: bool,
    progress: Progress<'_>,
) -> Option<Box<CourseMap>> {
    let projection = LocalProjection::for_route(route);
    let track: Vec<_> = route
        .points()
        .iter()
        .map(|p| projection.project(p.lat, p.lon))
        .collect();
    let frame = Frame::around(&track)?;
    let mut osm = Osm::new(cache_dir.join("osm"));
    if offline {
        osm = osm.offline();
    }
    let map = osm
        .area(
            frame.corners(&projection),
            frame.zoom(&projection),
            &mut |done, total| progress(LoadStage::CourseMap, done, total),
        )
        .await
        .inspect_err(|error| warn!(%error, "no map for the course's picture"))
        .ok()?;
    Some(Box::new(course_map::build(
        &map,
        &projection,
        &track,
        frame,
    )))
}

/// Reports elevation lookups as progress.
struct Counting<'a, M> {
    inner: &'a mut M,
    done: usize,
    total: usize,
    progress: Progress<'a>,
}

impl<M: ElevationModel + Send> ElevationModel for Counting<'_, M> {
    async fn elevation(&mut self, lat: f64, lon: f64) -> Result<f64, String> {
        let result = self.inner.elevation(lat, lon).await;
        self.done += 1;
        (self.progress)(LoadStage::Elevation, self.done.min(self.total), self.total);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_course_map_shows_the_land_around_the_whole_route() {
        // #203: a 15 km line from Meiringen to the east, its map from tiles of a zoom for the
        // whole of it, here the one cached around Meiringen.
        let cache = std::env::temp_dir().join(format!("torqa-course-map-{}", std::process::id()));
        let tiles = cache.join("osm/openfreemap-omt-v1/12/2141");
        std::fs::create_dir_all(&tiles).unwrap();
        std::fs::write(
            tiles.join("1445.pbf"),
            include_bytes!("../../torqa-osm/tests/data/12_2141_1445.pbf"),
        )
        .unwrap();
        let gpx = r#"<gpx><trk><trkseg>
            <trkpt lat="46.7277" lon="8.1872"><ele>595</ele></trkpt>
            <trkpt lat="46.7050" lon="8.2300"><ele>625</ele></trkpt>
            <trkpt lat="46.7000" lon="8.3800"><ele>700</ele></trkpt>
        </trkseg></trk></gpx>"#;
        let route = Route::from_gpx(gpx, None).await.unwrap();
        let mut stages = Vec::new();

        let map = course_map(&route, &cache, true, &mut |stage, _, total| {
            stages.push((stage, total));
        })
        .await
        .unwrap();

        // The neighbouring tiles are not cached: the map has what there is.
        assert!(
            stages
                .iter()
                .all(|&(stage, total)| stage == LoadStage::CourseMap && total > 1)
        );
        assert!(map.labels.iter().any(|l| l.name == "Meiringen"));
        let (west, east) = (map.frame.west, map.frame.east);
        let projection = LocalProjection::for_route(&route);
        let (end_east, _) = projection.project(46.70, 8.38);
        assert!(west < -500.0 && east > end_east + 500.0);
        std::fs::remove_dir_all(cache).unwrap();
    }
}
