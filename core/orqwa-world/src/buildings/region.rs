//! Regional architecture from bundled outlines, independent of climate and network access.

use std::sync::LazyLock;

/// The first regional set; other places keep their existing architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    Generic,
    Japan,
}

type Polygon = Vec<Vec<[f64; 2]>>;

static JAPAN: LazyLock<Vec<Polygon>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/japan.json"))
        .expect("bundled Japan outlines are valid JSON")
});

impl Region {
    pub(crate) fn at((lat, lon): (f64, f64)) -> Self {
        // Avoid walking island outlines for buildings far outside this set's region.
        if (17.0..=46.0).contains(&lat)
            && (122.0..=156.0).contains(&lon)
            && JAPAN.iter().any(|polygon| {
                polygon
                    .first()
                    .is_some_and(|ring| contains(ring, (lat, lon)))
                    && !polygon
                        .iter()
                        .skip(1)
                        .any(|ring| contains(ring, (lat, lon)))
            })
        {
            Self::Japan
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
    fn japanese_architecture_follows_islands_rather_than_a_latitude_band() {
        // Kyoto, Matsumoto, Sapporo, Shikoku, Kyushu, Okinawa, Ishigaki and Tsushima.
        for place in [
            (35.01, 135.77),
            (36.24, 137.97),
            (43.06, 141.35),
            (33.84, 132.77),
            (32.80, 130.71),
            (26.21, 127.68),
            (24.34, 124.16),
            (34.20, 129.29),
        ] {
            assert_eq!(Region::at(place), Region::Japan, "{place:?}");
        }
        // Nearby Korea, Taiwan, China and Russia, plus the existing Swiss fixtures.
        for place in [
            (35.18, 129.08),
            (37.57, 126.98),
            (25.03, 121.56),
            (31.23, 121.47),
            (43.12, 131.89),
            (46.95, 7.45),
        ] {
            assert_eq!(Region::at(place), Region::Generic, "{place:?}");
        }
    }
}
