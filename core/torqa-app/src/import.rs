//! Importing a route with everything needed to ride it.

use std::path::Path;

use torqa_osm::{MapData, Osm};
use torqa_routes::Route;
use torqa_terrain::{Terrain, TileSource};
use tracing::warn;

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
) -> Result<Imported, String> {
    let xml = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let invalid = |e: torqa_routes::RouteError| format!("cannot import {}: {e}", path.display());
    let track = torqa_routes::track_points(&xml).map_err(invalid)?;

    let mut osm = Osm::new(cache_dir.join("osm"));
    if offline {
        osm = osm.offline();
    }
    let map = osm
        .around(&track, torqa_world::CORRIDOR)
        .await
        .unwrap_or_else(|error| {
            warn!(%error, "no map data; bridges and tunnels follow the terrain");
            MapData::default()
        });

    let mut terrain = Terrain::new(TileSource::defaults(), cache_dir.join("terrain"));
    if offline {
        terrain = terrain.offline();
    }
    let route = Route::from_gpx_with(&xml, Some(&mut terrain), &map.structures)
        .await
        .map_err(invalid)?;
    let name = route.name().map_or_else(
        || {
            path.file_stem()
                .map_or_else(|| "Route".to_owned(), |s| s.to_string_lossy().into_owned())
        },
        ToOwned::to_owned,
    );
    Ok(Imported { route, map, name })
}
