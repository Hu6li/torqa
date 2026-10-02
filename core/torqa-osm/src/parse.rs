//! Turning Overpass JSON into map features.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::{
    Area, Building, LandCover, LatLon, MapData, OsmError, Road, Structure, StructureKind, Waterway,
};

#[derive(Deserialize)]
struct Response {
    elements: Vec<Element>,
}

#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type")]
    kind: String,
    id: i64,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    geometry: Vec<Point>,
    #[serde(default)]
    members: Vec<Member>,
}

#[derive(Deserialize)]
struct Member {
    #[serde(default)]
    role: String,
    #[serde(default)]
    geometry: Vec<Point>,
}

#[derive(Deserialize, Clone, Copy)]
struct Point {
    lat: f64,
    lon: f64,
}

/// Adds the features of one tile to `data`, skipping elements already in `seen` (features
/// crossing tile borders appear in several tiles).
pub(crate) fn merge(
    json: &str,
    data: &mut MapData,
    seen: &mut HashSet<(String, i64)>,
) -> Result<(), OsmError> {
    let response: Response =
        serde_json::from_str(json).map_err(|e| OsmError::Invalid(e.to_string()))?;
    for element in response.elements {
        if !seen.insert((element.kind.clone(), element.id)) {
            continue;
        }
        add(&element, data);
    }
    Ok(())
}

fn add(element: &Element, data: &mut MapData) {
    let tags = &element.tags;
    let line: Vec<LatLon> = element.geometry.iter().map(|p| (p.lat, p.lon)).collect();

    if element.kind == "way" && tags.contains_key("highway") {
        let kind = if tags.get("bridge").is_some_and(|v| v != "no") {
            Some(StructureKind::Bridge)
        } else if tags.get("tunnel").is_some_and(|v| v != "no") {
            Some(StructureKind::Tunnel)
        } else {
            None
        };
        if line.len() < 2 {
            return;
        }
        let highway = tags.get("highway").map_or("", String::as_str);
        if matches!(
            highway,
            "motorway"
                | "trunk"
                | "primary"
                | "secondary"
                | "tertiary"
                | "unclassified"
                | "residential"
                | "living_street"
                | "track"
        ) {
            data.roads.push(Road {
                major: matches!(highway, "motorway" | "trunk" | "primary" | "secondary"),
                line: line.clone(),
            });
        }
        if let Some(kind) = kind {
            data.structures.push(Structure { kind, line });
        }
        return;
    }
    if element.kind == "way" && tags.contains_key("building") {
        if is_closed(&line) && line.len() >= 4 {
            data.buildings.push(Building {
                id: element.id,
                outline: line,
                height: tags.get("height").and_then(|v| number(v)),
                levels: tags.get("building:levels").and_then(|v| number(v)),
            });
        }
        return;
    }
    if element.kind == "way"
        && let Some(waterway) = tags.get("waterway")
    {
        let typical = match waterway.as_str() {
            "river" => 12.0,
            "canal" => 8.0,
            _ => 3.0,
        };
        let width = tags.get("width").and_then(|v| number(v)).unwrap_or(typical);
        if line.len() >= 2 {
            data.waterways.push(Waterway { width, line });
        }
        return;
    }
    let Some(cover) = land_cover(tags) else {
        return;
    };
    let area = match element.kind.as_str() {
        "way" if is_closed(&line) && line.len() >= 4 => Area {
            cover,
            outer: vec![line],
            inner: Vec::new(),
        },
        "relation" => {
            let rings = |role: &str| {
                let parts = element
                    .members
                    .iter()
                    .filter(|m| m.role == role)
                    .map(|m| m.geometry.iter().map(|p| (p.lat, p.lon)).collect())
                    .collect();
                assemble_rings(parts)
            };
            Area {
                cover,
                outer: rings("outer"),
                inner: rings("inner"),
            }
        }
        _ => return,
    };
    if !area.outer.is_empty() {
        data.areas.push(area);
    }
}

fn land_cover(tags: &HashMap<String, String>) -> Option<LandCover> {
    let value = |key: &str| tags.get(key).map(String::as_str);
    match (value("landuse"), value("natural")) {
        (Some("forest"), _) | (_, Some("wood" | "scrub")) => Some(LandCover::Forest),
        (Some("meadow" | "grass" | "village_green"), _) | (_, Some("grassland" | "heath")) => {
            Some(LandCover::Meadow)
        }
        (Some("farmland" | "farmyard" | "allotments"), _) => Some(LandCover::Farmland),
        (Some("vineyard" | "orchard"), _) => Some(LandCover::Orchard),
        (Some("residential" | "commercial" | "industrial" | "retail"), _) => {
            Some(LandCover::Residential)
        }
        (Some("reservoir"), _) | (_, Some("water" | "wetland")) => Some(LandCover::Water),
        (_, Some("bare_rock" | "scree" | "glacier")) => Some(LandCover::Rock),
        _ => None,
    }
}

/// Joins way pieces sharing end points into closed rings; unclosable pieces are dropped.
fn assemble_rings(mut parts: Vec<Vec<LatLon>>) -> Vec<Vec<LatLon>> {
    parts.retain(|p| p.len() >= 2);
    let mut rings = Vec::new();
    while let Some(mut ring) = parts.pop() {
        while !is_closed(&ring) {
            let end = ring[ring.len() - 1];
            let Some(index) = parts
                .iter()
                .position(|p| same(p[0], end) || same(p[p.len() - 1], end))
            else {
                break;
            };
            let mut next = parts.swap_remove(index);
            if !same(next[0], end) {
                next.reverse();
            }
            ring.extend(next.into_iter().skip(1));
        }
        if is_closed(&ring) && ring.len() >= 4 {
            rings.push(ring);
        }
    }
    rings
}

fn is_closed(line: &[LatLon]) -> bool {
    line.len() >= 2 && same(line[0], line[line.len() - 1])
}

fn same(a: LatLon, b: LatLon) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

/// Parses tag numbers such as `12`, `12.5` or `12 m`.
fn number(value: &str) -> Option<f64> {
    value
        .trim()
        .trim_end_matches('m')
        .trim()
        .replace(',', ".")
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite() && *v > 0.0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"{"elements": [
      {"type": "way", "id": 1, "tags": {"building": "house", "building:levels": "3"},
       "geometry": [{"lat": 46.93, "lon": 7.44}, {"lat": 46.93, "lon": 7.4401},
                    {"lat": 46.9301, "lon": 7.4401}, {"lat": 46.93, "lon": 7.44}]},
      {"type": "way", "id": 2, "tags": {"highway": "secondary", "bridge": "viaduct"},
       "geometry": [{"lat": 46.93, "lon": 7.44}, {"lat": 46.931, "lon": 7.44}]},
      {"type": "way", "id": 3, "tags": {"highway": "secondary", "tunnel": "yes"},
       "geometry": [{"lat": 46.93, "lon": 7.45}, {"lat": 46.931, "lon": 7.45}]},
      {"type": "way", "id": 4, "tags": {"highway": "path", "bridge": "no"},
       "geometry": [{"lat": 46.93, "lon": 7.46}, {"lat": 46.931, "lon": 7.46}]},
      {"type": "way", "id": 5, "tags": {"waterway": "river", "width": "20 m"},
       "geometry": [{"lat": 46.92, "lon": 7.44}, {"lat": 46.921, "lon": 7.44}]},
      {"type": "relation", "id": 6, "tags": {"type": "multipolygon", "natural": "wood"},
       "members": [
         {"type": "way", "role": "outer",
          "geometry": [{"lat": 46.0, "lon": 7.0}, {"lat": 46.0, "lon": 7.01}, {"lat": 46.01, "lon": 7.01}]},
         {"type": "way", "role": "outer",
          "geometry": [{"lat": 46.0, "lon": 7.0}, {"lat": 46.01, "lon": 7.0}, {"lat": 46.01, "lon": 7.01}]},
         {"type": "way", "role": "inner",
          "geometry": [{"lat": 46.004, "lon": 7.004}, {"lat": 46.004, "lon": 7.005},
                       {"lat": 46.005, "lon": 7.005}, {"lat": 46.004, "lon": 7.004}]}
       ]},
      {"type": "way", "id": 7, "tags": {"landuse": "farmland"},
       "geometry": [{"lat": 46.0, "lon": 7.0}, {"lat": 46.0, "lon": 7.01},
                    {"lat": 46.01, "lon": 7.01}, {"lat": 46.0, "lon": 7.0}]}
    ]}"#;

    fn parse(json: &str) -> MapData {
        let mut data = MapData::default();
        merge(json, &mut data, &mut HashSet::new()).unwrap();
        data
    }

    #[test]
    fn reads_buildings_with_levels() {
        let data = parse(SAMPLE);

        assert_eq!(data.buildings.len(), 1);
        assert_eq!(data.buildings[0].levels, Some(3.0));
        assert_eq!(data.buildings[0].height, None);
    }

    #[test]
    fn reads_bridges_and_tunnels_but_not_bridge_no() {
        let kinds: Vec<_> = parse(SAMPLE).structures.iter().map(|s| s.kind).collect();

        assert_eq!(kinds, [StructureKind::Bridge, StructureKind::Tunnel]);
    }

    #[test]
    fn reads_roads_for_the_minimap() {
        let roads = parse(SAMPLE).roads;

        // Both secondary roads (on a bridge and in a tunnel); the path is not drawn.
        assert_eq!(roads.len(), 2);
        assert!(roads.iter().all(|r| r.major));
    }

    #[test]
    fn reads_waterway_width_with_units() {
        assert_eq!(parse(SAMPLE).waterways[0].width, 20.0);
    }

    #[test]
    fn assembles_multipolygon_rings_from_pieces() {
        let data = parse(SAMPLE);
        let forest = data
            .areas
            .iter()
            .find(|a| a.cover == LandCover::Forest)
            .unwrap();

        assert_eq!(forest.outer.len(), 1, "two pieces form one ring");
        assert!(is_closed(&forest.outer[0]));
        assert_eq!(forest.outer[0].len(), 5);
        assert_eq!(forest.inner.len(), 1);
        assert!(data.areas.iter().any(|a| a.cover == LandCover::Farmland));
    }

    #[test]
    fn elements_in_several_tiles_are_kept_once() {
        let mut data = MapData::default();
        let mut seen = HashSet::new();
        merge(SAMPLE, &mut data, &mut seen).unwrap();
        merge(SAMPLE, &mut data, &mut seen).unwrap();

        assert_eq!(data.buildings.len(), 1);
    }
}
