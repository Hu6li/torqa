//! Map detail (ADR 0033): what the vector tiles leave out of OpenStreetMap — road surfaces such
//! as cobbles, and the way traffic goes round a roundabout — made from the download of a
//! region (`.osm.pbf`, from Geofabrik) a rider asked for, and kept in the map cache.

pub(crate) mod apply;
mod census;
mod extract;
mod geofabrik;
mod pbf;
mod regions;

pub use census::{BuildingCensus, SHAPE_TAGS, building_census};
pub use extract::{Extract, Stage, VERSION, extract};
pub use geofabrik::{Geofabrik, INDEX, Latest, Offer, cached_offers};
pub use regions::{
    DOWNLOADS, Kept, Region, download_path, download_source, for_course, install, installed, kept,
    remove,
};

use std::collections::BTreeSet;
use std::path::Path;

use orqwa_domain::files::UsedFiles;
use tracing::warn;

use extract::{CELL_ZOOM, Cell, Roundabout, SurfaceWay, cell_path};

/// The map detail around a route, from the extracts it was asked of (see
/// [`crate::Osm::with_detail`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detail {
    /// The extracts any of it came from.
    extracts: Vec<String>,
    /// Roads with a surface the tiles cannot tell, each once.
    surfaces: Vec<SurfaceWay>,
    /// Roads going round a roundabout, each once.
    roundabouts: Vec<Roundabout>,
}

impl Detail {
    /// The extracts any of it came from: those a course prepared with it takes along.
    #[must_use]
    pub fn extracts(&self) -> &[String] {
        &self.extracts
    }

    pub(crate) fn surfaces(&self) -> &[SurfaceWay] {
        &self.surfaces
    }

    pub(crate) fn roundabouts(&self) -> &[Roundabout] {
        &self.roundabouts
    }
}

/// The detail of `extracts` in the cells of the zoom-14 `tiles`, from the map cache `osm_dir`
/// only; every cell read is recorded in `used`. A way in several cells or extracts is taken
/// once, from the first extract that has it.
pub(crate) async fn load(
    osm_dir: &Path,
    extracts: &[String],
    tiles: &BTreeSet<(u32, u32)>,
    used: &UsedFiles,
) -> Detail {
    let shift = crate::ZOOM - CELL_ZOOM;
    let cells: BTreeSet<(u32, u32)> = tiles
        .iter()
        .map(|&(x, y)| (x >> shift, y >> shift))
        .collect();
    let dir = osm_dir.join(VERSION);
    let mut detail = Detail::default();
    for extract in extracts {
        let mut found = false;
        for &cell in &cells {
            let path = cell_path(&dir, extract, cell);
            let Ok(bytes) = tokio::fs::read(&path).await else {
                continue;
            };
            match serde_json::from_slice::<Cell>(&bytes) {
                Ok(content) => {
                    used.record(&path);
                    detail.surfaces.extend(content.surfaces);
                    detail.roundabouts.extend(content.roundabouts);
                    found = true;
                }
                Err(error) => warn!(path = %path.display(), %error, "map detail unreadable"),
            }
        }
        if found {
            detail.extracts.push(extract.clone());
        }
    }
    // Stable: of the same way in several extracts, the first given stays.
    detail.surfaces.sort_by_key(|way| way.id);
    detail.surfaces.dedup_by_key(|way| way.id);
    detail.roundabouts.sort_by_key(|way| way.id);
    detail.roundabouts.dedup_by_key(|way| way.id);
    detail
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RoadKind, RoadSurface, tile_of};

    fn sett(id: i64, lat_e7: i32) -> SurfaceWay {
        SurfaceWay {
            id,
            kind: RoadKind::Local,
            surface: RoadSurface::Cobbles,
            line: vec![[lat_e7, 74_470_000], [lat_e7, 74_480_000]],
        }
    }

    fn write(osm: &Path, extract: &str, cell: (u32, u32), surfaces: Vec<SurfaceWay>) {
        let path = cell_path(&osm.join(VERSION), extract, cell);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let cell = Cell {
            surfaces,
            roundabouts: Vec::new(),
        };
        std::fs::write(path, serde_json::to_vec(&cell).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn the_cells_of_a_routes_tiles_are_read_and_recorded_each_way_once() {
        let osm = std::env::temp_dir().join(format!("orqwa-detail-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&osm);
        let tile = tile_of(crate::ZOOM, 46.948, 7.447);
        let cell = (tile.0 >> 2, tile.1 >> 2);
        write(
            &osm,
            "bern-2026-10-24",
            cell,
            vec![sett(2, 469_481_000), sett(1, 469_480_000)],
        );
        // An overlapping region has way 1 too, and a cell the route does not reach.
        write(
            &osm,
            "berner-oberland-2026-10-20",
            cell,
            vec![sett(1, 469_480_100)],
        );
        write(
            &osm,
            "berner-oberland-2026-10-20",
            (cell.0 + 5, cell.1),
            vec![sett(3, 0)],
        );
        let used = UsedFiles::default();

        let extracts = [
            "bern-2026-10-24".to_owned(),
            "berner-oberland-2026-10-20".to_owned(),
            "zurich-2026-10-24".to_owned(),
        ];
        let detail = load(&osm, &extracts, &BTreeSet::from([tile]), &used).await;

        let ids: Vec<i64> = detail.surfaces().iter().map(|w| w.id).collect();
        assert_eq!(ids, [1, 2]);
        assert_eq!(
            detail.surfaces()[0].line[0][0],
            469_480_000,
            "the first extract's way"
        );
        // Zurich has no cell here: a course takes along only what it used.
        assert_eq!(detail.extracts(), &extracts[..2]);
        assert_eq!(
            used.paths(),
            [
                cell_path(&osm.join(VERSION), "bern-2026-10-24", cell),
                cell_path(&osm.join(VERSION), "berner-oberland-2026-10-20", cell),
            ]
        );
        std::fs::remove_dir_all(osm).unwrap();
    }

    #[tokio::test]
    async fn a_routes_streets_take_the_surfaces_of_its_detail_once_matched() {
        let cache = std::env::temp_dir().join(format!("orqwa-detail-osm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let tile = crate::tile_path(&cache, crate::ZOOM, (8531, 5767));
        std::fs::create_dir_all(tile.parent().unwrap()).unwrap();
        std::fs::write(&tile, include_bytes!("../../tests/data/14_8531_5767.pbf")).unwrap();
        // Wabern, near the Gurten.
        let route = [(46.9296, 7.4512)];
        let plain = crate::Osm::new(cache.clone())
            .offline()
            .around(&route, 50.0, &mut |_, _| {})
            .await
            .unwrap();
        let street = plain
            .roads
            .iter()
            .find(|r| {
                r.details.kind == RoadKind::Local && r.details.surface != RoadSurface::Unpaved
            })
            .unwrap()
            .clone();
        #[allow(clippy::cast_possible_truncation)] // positions on the map
        let line = street
            .line
            .iter()
            .map(|&(lat, lon)| [(lat * 1e7).round() as i32, (lon * 1e7).round() as i32])
            .collect();
        let cell = (8531 >> 2, 5767 >> 2);
        let cobbled = SurfaceWay {
            id: 1,
            kind: RoadKind::Local,
            surface: RoadSurface::Cobbles,
            line,
        };
        write(&cache, "bern-2026-10-24", cell, vec![cobbled]);
        let used = UsedFiles::default();

        let mut map = crate::Osm::new(cache.clone())
            .offline()
            .recording(used.clone())
            .with_detail(vec!["bern-2026-10-24".to_owned()])
            .around(&route, 50.0, &mut |_, _| {})
            .await
            .unwrap();

        assert_eq!(
            map.roads, plain.roads,
            "matching the route sees the tiles' roads"
        );
        assert_eq!(map.detail.extracts(), ["bern-2026-10-24"]);
        assert!(
            used.paths()
                .contains(&cell_path(&cache.join(VERSION), "bern-2026-10-24", cell))
        );
        map.apply_surfaces();
        let surface_of = |line: &[crate::LatLon]| {
            map.roads
                .iter()
                .find(|r| r.line == line)
                .map(|r| r.details.surface)
        };
        assert_eq!(surface_of(&street.line), Some(RoadSurface::Cobbles));
        assert!(
            map.roads
                .iter()
                .filter(|r| r.details.kind != RoadKind::Local)
                .all(|r| r.details.surface != RoadSurface::Cobbles)
        );
        std::fs::remove_dir_all(cache).unwrap();
    }
}
