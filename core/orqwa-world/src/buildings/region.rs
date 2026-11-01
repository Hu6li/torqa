//! Regional architecture from bundled outlines, independent of climate and network access.

use std::sync::LazyLock;

/// Asian rides use the East Asian starter archetypes; other places keep their architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    Generic,
    Asia,
}

type Polygon = Vec<Vec<[f64; 2]>>;

struct BoundedPolygon {
    rings: Polygon,
    bounds: (f64, f64, f64, f64),
}

static ASIA: LazyLock<Vec<BoundedPolygon>> = LazyLock::new(|| {
    let polygons: Vec<Polygon> = serde_json::from_str(include_str!("../../data/asia.json"))
        .expect("bundled Asian outlines are valid JSON");
    polygons
        .into_iter()
        .filter_map(|rings| {
            let outer = rings.first()?;
            let bounds = outer.iter().fold(
                (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(min_lon, min_lat, max_lon, max_lat), [lon, lat]| {
                    (
                        min_lon.min(*lon),
                        min_lat.min(*lat),
                        max_lon.max(*lon),
                        max_lat.max(*lat),
                    )
                },
            );
            Some(BoundedPolygon { rings, bounds })
        })
        .collect()
});

impl Region {
    pub(crate) fn at((lat, lon): (f64, f64)) -> Self {
        // UN M49 Asia region polygons (142), with per-island bounds to keep lookups cheap.
        if ASIA.iter().any(|polygon| {
            let (min_lon, min_lat, max_lon, max_lat) = polygon.bounds;
            (min_lat..=max_lat).contains(&lat)
                && (min_lon..=max_lon).contains(&lon)
                && polygon
                    .rings
                    .first()
                    .is_some_and(|ring| contains(ring, (lat, lon)))
                && !polygon
                    .rings
                    .iter()
                    .skip(1)
                    .any(|ring| contains(ring, (lat, lon)))
        }) {
            Self::Asia
        } else {
            Self::Generic
        }
    }
}

fn contains(ring: &[[f64; 2]], (lat, lon): (f64, f64)) -> bool {
    let mut inside = false;
    for pair in ring.windows(2) {
        let ([ax, ay], [bx, by]) = (pair[0], pair[1]);
        if (ay > lat) != (by > lat) && lon < ax + (lat - ay) * (bx - ax) / (by - ay) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asian_architecture_follows_country_outlines_across_latitudes_and_longitudes() {
        // Japan, Korea, China, India, Thailand, Indonesia, Saudi Arabia and Asian Russia.
        for place in [
            (35.01, 135.77),
            (37.57, 126.98),
            (31.23, 121.47),
            (28.61, 77.21),
            (13.75, 100.50),
            (-6.21, 106.85),
            (24.71, 46.68),
            (61.52, 105.32),
        ] {
            assert_eq!(Region::at(place), Region::Asia, "{place:?}");
        }
        // Europe, Africa, the Americas, Australia and Switzerland remain generic.
        for place in [
            (48.86, 2.35),
            (-1.29, 36.82),
            (40.71, -74.01),
            (-33.87, 151.21),
            (46.95, 7.45),
        ] {
            assert_eq!(Region::at(place), Region::Generic, "{place:?}");
        }
    }
}
