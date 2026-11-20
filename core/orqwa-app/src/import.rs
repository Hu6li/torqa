//! Importing a route with everything needed to ride it.

use crate::failure::{Failure, Kind};
use std::path::Path;

use orqwa_domain::files::UsedFiles;
use orqwa_osm::{MapData, Osm};
use orqwa_routes::{ElevationModel, LocalProjection, Route};
use orqwa_storage::course::CourseCountry;
use orqwa_terrain::{Terrain, TileSource};
use orqwa_world::FlatMap;
use orqwa_world::course_map::{self, CourseMap, Frame};
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
    /// Finding the countries the route passes (#230).
    Countries,
    /// Reading the course's world kept from an earlier ride (R101).
    KeptWorld,
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
            Self::Countries => "Finding the countries",
            Self::KeptWorld => "Reading the saved world",
            Self::World => "Building 3D world",
        }
        // i18n-end
    }

    /// What the step counts, for display.
    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::Route | Self::KeptWorld => "files",
            Self::Map | Self::CourseMap => "tiles",
            Self::Countries => "steps",
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
    /// The map of the land around the whole route ([`course_map`]), for the course's picture
    /// and the minimap; `None` until it is made, or without map data.
    pub course_map: Option<Box<CourseMap>>,
    /// The detailed map of the route's corridor, for the minimap's close view (R70).
    pub minimap: FlatMap,
    /// The countries the route passes, when looked up ([`countries`]).
    pub countries: Option<Vec<CourseCountry>>,
    /// Whether traffic keeps left at each route point; `None` where its country is unknown.
    pub driving_left: Vec<Option<bool>>,
    /// The world file the route runs through, for a world's route (R266).
    pub authored: Option<std::sync::Arc<crate::authored::Authored>>,
}

/// Reads a GPX file and imports it with [`import_gpx`], named after the file if the track has
/// no name, with the map detail of the regions ready in the cache (ADR 0033).
///
/// # Errors
/// A readable message if the file cannot be read or imported.
pub async fn import_route(
    path: &Path,
    cache_dir: &Path,
    offline: bool,
    used: &UsedFiles,
    progress: Progress<'_>,
) -> Result<Imported, Failure> {
    progress(LoadStage::Route, 0, 1);
    let xml = tokio::fs::read(path)
        .await
        .map(text)
        .map_err(|e| Failure::new(Kind::ReadFile, e).arg(path.display()))?;
    let fallback = path
        .file_stem()
        .map_or_else(|| "Route".to_owned(), |s| s.to_string_lossy().into_owned());
    let map_detail: Vec<String> = orqwa_osm::detail::installed(&cache_dir.join("osm"))
        .into_iter()
        .map(|region| region.extract)
        .collect();
    import_gpx(
        xml,
        &fallback,
        (cache_dir, offline),
        (false, &map_detail),
        used,
        progress,
    )
    .await
    .map_err(|e| Failure::new(Kind::ImportFile, e).arg(path.display()))
}

/// Imports a GPX track: fetches map data along it (bridges and tunnels shape the elevation
/// profile) and corrects elevations with the terrain model. Downloads are cached under
/// `cache_dir` and every cached file used is recorded in `used`; `offline` uses the caches
/// only. Without a `video`, the route is ridden in 3D and loses its turns in place
/// ([`Route::without_turns_in_place`]); a video keeps the track it was recorded along. The
/// roads take the roundabouts of the `map_detail` extracts (ADR 0033) before the route is
/// matched to them, and their surfaces after; the cells read are recorded too.
///
/// # Errors
/// A readable message if the track cannot be imported.
pub async fn import_gpx(
    xml: String,
    fallback_name: &str,
    (cache_dir, offline): (&Path, bool),
    (video, map_detail): (bool, &[String]),
    used: &UsedFiles,
    progress: Progress<'_>,
) -> Result<Imported, Failure> {
    let track =
        orqwa_routes::track_points(&xml).map_err(|e| Failure::new(Kind::PrepareCourse, e))?;
    progress(LoadStage::Route, 1, 1);

    let mut osm = Osm::new(cache_dir.join("osm"))
        .recording(used.clone())
        .with_detail(map_detail.to_vec());
    if offline {
        osm = osm.offline();
    }
    let mut map = osm
        .around(&track, orqwa_world::CORRIDOR, &mut |done, total| {
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
    // Round a roundabout the detail maps the way traffic goes, wherever the tiles draw it.
    map.apply_roundabouts();
    let mut counting = Counting {
        inner: &mut terrain,
        done: 0,
        total: track.len(),
        progress: &mut *progress,
    };
    let mut route = Route::from_gpx_with(&xml, Some(&mut counting), &map)
        .await
        .map_err(|e| Failure::new(Kind::PrepareCourse, e))?;
    if !video {
        route = route.without_turns_in_place();
    }
    map.apply_surfaces();
    map.apply_building_kinds();
    progress(LoadStage::Elevation, track.len(), track.len());
    let name = route
        .name()
        .map_or_else(|| fallback_name.to_owned(), ToOwned::to_owned);
    let minimap = orqwa_world::minimap(&route, &map);
    // Record these border tiles with the course too, so lane placement works offline.
    let places: Vec<_> = route.points().iter().map(|p| (p.lat, p.lon)).collect();
    let codes = osm.countries_at(&places).await.unwrap_or_else(|error| {
        warn!(%error, "traffic side is unknown; keeping right");
        vec![None; places.len()]
    });
    let driving_left = codes
        .iter()
        .zip(&places)
        .map(|(code, &place)| {
            orqwa_osm::traffic_territory(place)
                .or(code.as_deref())
                .map(|code| crate::traffic::keeps_left(Some(code)))
        })
        .collect();
    Ok(Imported {
        route,
        map,
        name,
        gpx: xml,
        course_map: None,
        minimap,
        countries: None,
        driving_left,
        authored: None,
    })
}

/// The map of the land around the whole of `route` for the course's picture (#203) and the
/// minimap (R70), from map tiles as detailed as the picture needs. Downloads are cached under
/// `cache_dir` and every cached tile used is recorded in `used`, for the course file to keep;
/// `offline` uses the cache only. `None` if no tile is available.
pub async fn course_map(
    route: &Route,
    (cache_dir, offline): (&Path, bool),
    used: &UsedFiles,
    progress: Progress<'_>,
) -> Option<Box<CourseMap>> {
    let projection = LocalProjection::for_route(route);
    let track: Vec<_> = route
        .points()
        .iter()
        .map(|p| projection.project(p.lat, p.lon))
        .collect();
    let frame = Frame::around(&track)?;
    let mut osm = Osm::new(cache_dir.join("osm")).recording(used.clone());
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

/// The countries the route through `points` (latitude, longitude) passes (#230), from map
/// tiles cached under `cache_dir`, downloaded unless `offline`. `None` if the tiles are not to
/// be had, so that they are looked up again later.
pub async fn countries(
    points: &[(f64, f64)],
    cache_dir: &Path,
    offline: bool,
    progress: Progress<'_>,
) -> Option<Vec<CourseCountry>> {
    progress(LoadStage::Countries, 0, 1);
    let mut osm = Osm::new(cache_dir.join("osm"));
    if offline {
        osm = osm.offline();
    }
    let countries = osm
        .countries_along(points)
        .await
        .inspect_err(|error| warn!(%error, "the route's countries are not known yet"))
        .ok()?;
    progress(LoadStage::Countries, 1, 1);
    Some(
        countries
            .into_iter()
            .map(|country| CourseCountry {
                code: country.code,
                names: country.names,
            })
            .collect(),
    )
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

/// A route or control file's text. Route and video libraries also ship Windows-1252 files;
/// rather than refuse them, their odd characters are replaced, which garbles only accented
/// names.
pub(crate) fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_that_are_not_utf8_are_still_read() {
        assert_eq!(text("Col de Prémol".into()), "Col de Prémol");
        assert_eq!(text(b"Col de Pr\xe9mol".to_vec()), "Col de Pr\u{fffd}mol");
    }

    #[tokio::test]
    async fn traffic_side_comes_from_the_route_and_border_inputs_are_kept_offline() {
        let cache =
            std::env::temp_dir().join(format!("orqwa-traffic-import-{}", std::process::id()));
        let tile = cache.join("osm/openfreemap-omt-v1/6/33/22.pbf");
        std::fs::create_dir_all(tile.parent().unwrap()).unwrap();
        // A synthetic left-driving country at the Swiss fixture's location proves that the
        // map's country, rather than language or a geographic bounding box, sets the rule.
        let mut borders = include_bytes!("../../fixtures/countries/6/33/22.pbf").to_vec();
        for i in 0..borders.len().saturating_sub(2) {
            if &borders[i..i + 3] == b"CHE" {
                borders[i..i + 3].copy_from_slice(b"GBR");
            }
        }
        std::fs::write(&tile, borders).unwrap();
        let gpx = r#"<gpx><trk><trkseg>
            <trkpt lat="46.948" lon="7.447"><ele>550</ele></trkpt>
            <trkpt lat="46.949" lon="7.448"><ele>550</ele></trkpt>
        </trkseg></trk></gpx>"#;
        let used = UsedFiles::default();
        let imported = import_gpx(
            gpx.into(),
            "Traffic",
            (&cache, true),
            (false, &[]),
            &used,
            &mut |_, _, _| {},
        )
        .await
        .unwrap();
        assert_eq!(imported.driving_left.len(), imported.route.points().len());
        assert!(imported.driving_left.iter().all(|&left| left == Some(true)));
        assert!(
            used.paths().contains(&tile),
            "the course must bundle the border tile"
        );
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[tokio::test]
    async fn a_course_map_shows_the_land_around_the_whole_route() {
        // #203: a 15 km line from Meiringen to the east, its map from tiles of a zoom for the
        // whole of it, here the one cached around Meiringen.
        let cache = std::env::temp_dir().join(format!("orqwa-course-map-{}", std::process::id()));
        let tiles = cache.join("osm/openfreemap-omt-v1/12/2141");
        std::fs::create_dir_all(&tiles).unwrap();
        std::fs::write(
            tiles.join("1445.pbf"),
            include_bytes!("../../orqwa-osm/tests/data/12_2141_1445.pbf"),
        )
        .unwrap();
        let gpx = r#"<gpx><trk><trkseg>
            <trkpt lat="46.7277" lon="8.1872"><ele>595</ele></trkpt>
            <trkpt lat="46.7050" lon="8.2300"><ele>625</ele></trkpt>
            <trkpt lat="46.7000" lon="8.3800"><ele>700</ele></trkpt>
        </trkseg></trk></gpx>"#;
        let route = Route::from_gpx(gpx, None).await.unwrap();
        let mut stages = Vec::new();
        let used = UsedFiles::default();

        let map = course_map(&route, (&cache, true), &used, &mut |stage, _, total| {
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
        // R70: its tile goes into the course file, for the minimap to show the land offline.
        assert_eq!(used.paths(), [tiles.join("1445.pbf")]);
        std::fs::remove_dir_all(cache).unwrap();
    }
}
