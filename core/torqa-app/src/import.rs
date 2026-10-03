//! Importing a route with everything needed to ride it.

use std::path::Path;

use torqa_osm::{MapData, Osm};
use torqa_routes::{ElevationModel, Route};
use torqa_terrain::{Terrain, TileSource};
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
    /// Building the 3D world.
    World,
}

impl LoadStage {
    /// What the step does, for display.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Route => "Reading route",
            Self::Map => "Downloading map data",
            Self::Elevation => "Correcting elevations",
            Self::World => "Building 3D world",
        }
    }

    /// What the step counts, for display.
    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::Route => "files",
            Self::Map => "tiles",
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
}

/// Reads a GPX file, fetches map data along it (bridges and tunnels shape the elevation
/// profile) and corrects elevations with the terrain model. Downloads are cached under
/// `cache_dir`; `offline` uses the caches only.
///
/// # Errors
/// A readable message if the file cannot be read or imported.
pub async fn import_route(
    path: &Path,
    cache_dir: &Path,
    offline: bool,
    progress: Progress<'_>,
) -> Result<Imported, String> {
    progress(LoadStage::Route, 0, 1);
    let xml = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let invalid = |e: torqa_routes::RouteError| format!("cannot import {}: {e}", path.display());
    let track = torqa_routes::track_points(&xml).map_err(invalid)?;
    progress(LoadStage::Route, 1, 1);

    let mut osm = Osm::new(cache_dir.join("osm"));
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

    let mut terrain = Terrain::new(TileSource::defaults(), cache_dir.join("terrain"));
    if offline {
        terrain = terrain.offline();
    }
    let mut counting = Counting {
        inner: &mut terrain,
        done: 0,
        total: track.len(),
        progress: &mut *progress,
    };
    let route = Route::from_gpx_with(&xml, Some(&mut counting), &map.structures)
        .await
        .map_err(invalid)?;
    progress(LoadStage::Elevation, track.len(), track.len());
    let name = route.name().map_or_else(
        || {
            path.file_stem()
                .map_or_else(|| "Route".to_owned(), |s| s.to_string_lossy().into_owned())
        },
        ToOwned::to_owned,
    );
    Ok(Imported { route, map, name })
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
