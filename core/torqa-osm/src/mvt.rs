//! Decoding `OpenMapTiles` vector tiles into map features.
//! Schema: <https://openmaptiles.org/schema/>.

use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use geo_types::{Geometry, LineString, Polygon};
use mvt_reader::Reader;
use mvt_reader::feature::Value;

use crate::{
    Area, Building, LandCover, LatLon, MapData, OsmError, Road, RoadClass, StructureKind, Waterway,
    ZOOM,
};

/// Adds the features of tile (x, y) to `data`. Buildings crossing tile borders appear in each
/// tile; `buildings_seen` keeps the first copy so they are not drawn twice.
pub(crate) fn merge(
    (x, y): (u32, u32),
    bytes: Vec<u8>,
    data: &mut MapData,
    buildings_seen: &mut HashSet<u64>,
) -> Result<(), OsmError> {
    let invalid = |e: mvt_reader::error::ParserError| OsmError::Invalid(e.to_string());
    let reader = Reader::new(bytes).map_err(invalid)?;
    for layer in reader.get_layer_metadata().map_err(invalid)? {
        let projection = TileProjection {
            x: f64::from(x),
            y: f64::from(y),
            extent: f64::from(layer.extent),
        };
        for feature in reader
            .get_features_as::<f64>(layer.layer_index)
            .map_err(invalid)?
        {
            let tags = Tags(feature.properties.unwrap_or_default());
            let geometry = &feature.geometry;
            match layer.name.as_str() {
                "building" => {
                    if feature.id.is_none_or(|id| buildings_seen.insert(id)) {
                        add_building(data, feature.id, &tags, geometry, &projection);
                    }
                }
                "landcover" | "landuse" | "park" | "water" => {
                    add_area(data, &layer.name, &tags, geometry, &projection);
                }
                "waterway" => add_waterway(data, &tags, geometry, &projection),
                "transportation" => add_road(data, &tags, geometry, &projection),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Feature properties.
struct Tags(HashMap<String, Value>);

impl Tags {
    fn text(&self, key: &str) -> &str {
        match self.0.get(key) {
            Some(Value::String(s)) => s.as_str(),
            _ => "",
        }
    }

    fn number(&self, key: &str) -> Option<f64> {
        // Tag values are small (heights in metres); f64 holds them exactly.
        #[allow(clippy::cast_precision_loss)]
        match self.0.get(key) {
            Some(Value::Int(v) | Value::SInt(v)) => Some(*v as f64),
            Some(Value::UInt(v)) => Some(*v as f64),
            Some(Value::Float(v)) => Some(f64::from(*v)),
            Some(Value::Double(v)) => Some(*v),
            _ => None,
        }
    }
}

fn add_building(
    data: &mut MapData,
    id: Option<u64>,
    tags: &Tags,
    geometry: &Geometry<f64>,
    projection: &TileProjection,
) {
    // Ids only seed visual variation; any stable value will do.
    #[allow(clippy::cast_possible_wrap)]
    let id = id.map_or_else(
        || i64::try_from(data.buildings.len()).unwrap_or_default(),
        |id| (id & 0x7FFF_FFFF_FFFF_FFFF) as i64,
    );
    for polygon in polygons(geometry) {
        data.buildings.push(Building {
            id,
            outline: projection.ring(polygon.exterior()),
            height: tags.number("render_height").filter(|h| *h > 0.0),
            levels: None,
        });
    }
}

fn add_area(
    data: &mut MapData,
    layer: &str,
    tags: &Tags,
    geometry: &Geometry<f64>,
    projection: &TileProjection,
) {
    let Some(cover) = land_cover(layer, tags.text("class")) else {
        return;
    };
    for polygon in polygons(geometry) {
        data.areas.push(Area {
            cover,
            outer: vec![projection.ring(polygon.exterior())],
            inner: polygon
                .interiors()
                .iter()
                .map(|ring| projection.ring(ring))
                .collect(),
        });
    }
}

fn add_waterway(
    data: &mut MapData,
    tags: &Tags,
    geometry: &Geometry<f64>,
    projection: &TileProjection,
) {
    let width = match tags.text("class") {
        "river" => 12.0,
        "canal" => 8.0,
        _ => 3.0,
    };
    for line in lines(geometry) {
        data.waterways.push(Waterway {
            width,
            line: projection.line(line),
        });
    }
}

fn add_road(
    data: &mut MapData,
    tags: &Tags,
    geometry: &Geometry<f64>,
    projection: &TileProjection,
) {
    let class = match tags.text("class") {
        "motorway" | "trunk" | "primary" | "secondary" => Some(RoadClass::Major),
        "tertiary" | "minor" => Some(RoadClass::Street),
        "service" => Some(RoadClass::Service),
        "track" => Some(RoadClass::Track),
        // Cycleways among them: routes often follow them.
        "path" => Some(RoadClass::Path),
        _ => None,
    };
    let kind = match tags.text("brunnel") {
        "bridge" => Some(StructureKind::Bridge),
        "tunnel" => Some(StructureKind::Tunnel),
        _ => None,
    };
    for line in lines(geometry) {
        let line = projection.line(line);
        if line.len() < 2 {
            continue;
        }
        if let Some(class) = class {
            data.roads.push(Road {
                class,
                line,
                structure: kind,
            });
        }
    }
}

fn land_cover(layer: &str, class: &str) -> Option<LandCover> {
    match (layer, class) {
        ("landcover", "wood") => Some(LandCover::Forest),
        ("landcover", "grass") | ("park", _) => Some(LandCover::Meadow),
        ("landcover", "farmland") => Some(LandCover::Farmland),
        ("landcover", "rock" | "ice" | "sand") => Some(LandCover::Rock),
        ("landuse", "residential" | "commercial" | "industrial" | "retail" | "suburb") => {
            Some(LandCover::Residential)
        }
        ("water", _) => Some(LandCover::Water),
        _ => None,
    }
}

fn polygons(geometry: &Geometry<f64>) -> Vec<&Polygon<f64>> {
    match geometry {
        Geometry::Polygon(polygon) => vec![polygon],
        Geometry::MultiPolygon(multi) => multi.0.iter().collect(),
        _ => Vec::new(),
    }
}

fn lines(geometry: &Geometry<f64>) -> Vec<&LineString<f64>> {
    match geometry {
        Geometry::LineString(line) => vec![line],
        Geometry::MultiLineString(multi) => multi.0.iter().collect(),
        _ => Vec::new(),
    }
}

/// Converts tile pixel coordinates (y down) to latitude/longitude.
struct TileProjection {
    x: f64,
    y: f64,
    extent: f64,
}

impl TileProjection {
    fn point(&self, px: f64, py: f64) -> LatLon {
        let n = f64::from(1u32 << ZOOM);
        let lon = (self.x + px / self.extent) / n * 360.0 - 180.0;
        let mercator = PI * (1.0 - 2.0 * (self.y + py / self.extent) / n);
        (mercator.sinh().atan().to_degrees(), lon)
    }

    fn line(&self, line: &LineString<f64>) -> Vec<LatLon> {
        line.0.iter().map(|c| self.point(c.x, c.y)).collect()
    }

    /// A closed ring.
    fn ring(&self, ring: &LineString<f64>) -> Vec<LatLon> {
        let mut points = self.line(ring);
        if points.first() != points.last()
            && let Some(&first) = points.first()
        {
            points.push(first);
        }
        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tile() -> MapData {
        let mut data = MapData::default();
        merge(
            (8531, 5767),
            include_bytes!("../tests/data/14_8531_5767.pbf").to_vec(),
            &mut data,
            &mut HashSet::new(),
        )
        .unwrap();
        data
    }

    #[test]
    fn tile_coordinates_map_to_the_tile_area() {
        let projection = TileProjection {
            x: 8531.0,
            y: 5767.0,
            extent: 4096.0,
        };
        let (north, west) = projection.point(0.0, 0.0);
        let (south, east) = projection.point(4096.0, 4096.0);

        // Tile 14/8531/5767 spans about 46.92–46.94 °N, 7.44–7.47 °E.
        assert!(north > south && east > west);
        assert!((46.91..46.95).contains(&south) && (46.91..46.95).contains(&north));
        assert!((7.43..7.48).contains(&west) && (7.43..7.48).contains(&east));
    }

    #[test]
    fn decodes_buildings_with_heights_and_closed_outlines() {
        let data = test_tile();

        assert!(data.buildings.len() > 50, "{}", data.buildings.len());
        assert!(
            data.buildings
                .iter()
                .all(|b| b.outline.first() == b.outline.last())
        );
        assert!(data.buildings.iter().any(|b| b.height.is_some()));
    }

    #[test]
    fn decodes_land_cover_roads_and_water() {
        let data = test_tile();

        assert!(data.areas.iter().any(|a| a.cover == LandCover::Forest));
        assert!(data.areas.iter().any(|a| a.cover == LandCover::Residential));
        assert!(data.roads.iter().any(|r| !r.major()));
        assert!(
            !data.waterways.is_empty() || data.areas.iter().any(|a| a.cover == LandCover::Water)
        );
    }

    #[test]
    fn buildings_across_tiles_are_kept_once() {
        let mut data = MapData::default();
        let mut seen = HashSet::new();
        let bytes = include_bytes!("../tests/data/14_8531_5767.pbf").to_vec();
        merge((8531, 5767), bytes.clone(), &mut data, &mut seen).unwrap();
        let once = data.buildings.len();
        merge((8531, 5767), bytes, &mut data, &mut seen).unwrap();

        assert_eq!(data.buildings.len(), once);
    }
}
