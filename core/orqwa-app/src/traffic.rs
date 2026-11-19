//! Local traffic rules affect the avatar, never the recorded route or ride physics.

use orqwa_domain::units::Meters;
use orqwa_routes::{LocalProjection, Route};
use orqwa_world::RoadWidths;

/// ISO country/territory defaults. Steering-wheel position is deliberately irrelevant
/// (e.g. the US Virgin Islands keep left). Unknown places keep right.
/// Reference: <https://www.worldstandards.eu/cars/list-of-left-driving-countries/>.
pub(crate) fn keeps_left(code: Option<&str>) -> bool {
    matches!(
        code,
        Some(
            "AG" | "AI"
                | "AU"
                | "BB"
                | "BD"
                | "BM"
                | "BN"
                | "BS"
                | "BT"
                | "BW"
                | "CC"
                | "CK"
                | "CX"
                | "CY"
                | "DM"
                | "FJ"
                | "FK"
                | "GB"
                | "GD"
                | "GG"
                | "GS"
                | "GY"
                | "HK"
                | "ID"
                | "IE"
                | "IM"
                | "IN"
                | "JE"
                | "JM"
                | "JP"
                | "KE"
                | "KI"
                | "KN"
                | "KY"
                | "LC"
                | "LK"
                | "LS"
                | "MO"
                | "MS"
                | "MT"
                | "MU"
                | "MV"
                | "MW"
                | "MY"
                | "MZ"
                | "NA"
                | "NF"
                | "NP"
                | "NR"
                | "NU"
                | "NZ"
                | "PG"
                | "PK"
                | "PN"
                | "SB"
                | "SC"
                | "SG"
                | "SH"
                | "SR"
                | "SZ"
                | "TC"
                | "TH"
                | "TK"
                | "TL"
                | "TO"
                | "TT"
                | "TV"
                | "TZ"
                | "UG"
                | "VC"
                | "VG"
                | "VI"
                | "WS"
                | "ZA"
                | "ZM"
                | "ZW"
        )
    )
}

pub(crate) fn position(
    route: &Route,
    sides: &[Option<bool>],
    widths: &RoadWidths,
    distance: Meters,
    ghost: bool,
) -> (f64, f64) {
    let at = route.position(distance);
    let (east, north) = LocalProjection::for_route(route).project(at.lat, at.lon);
    let index = route
        .points()
        .partition_point(|p| p.distance.0 <= distance.0)
        .saturating_sub(1);
    let sign = if sides.get(index).copied().flatten().unwrap_or(false) {
        -1.0
    } else {
        1.0
    };
    // Halfway into the local half of the carriageway, scaling down with narrow paths, from
    // its middle, which lies off the route's line where buildings show the street does.
    // Ghosts stay nearer the centre on the same side, including in left-hand traffic.
    let offset =
        widths.at(east, north, at.elevation).0 * 0.25 * sign * if ghost { 0.35 } else { 1.0 };
    let (east, north) = widths.middle(east, north, at.elevation);
    (
        east + at.heading.cos() * offset,
        north - at.heading.sin() * offset,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_defaults_cover_both_sides_and_territory_exceptions() {
        for code in [
            "GB", "IE", "JP", "AU", "NZ", "IN", "ZA", "TH", "HK", "MO", "VI", "WS", "TL",
        ] {
            assert!(keeps_left(Some(code)), "{code}");
        }
        for code in [
            None,
            Some("CH"),
            Some("FR"),
            Some("US"),
            Some("CN"),
            Some("MM"),
            Some("SO"),
            Some("XX"),
        ] {
            assert!(!keeps_left(code), "{code:?}");
        }
    }

    #[tokio::test]
    async fn offsets_follow_travel_direction_on_both_sides_and_keep_ghosts_in_lane() {
        for (lat, lon) in [(0.002, 0.0), (0.0, 0.002), (-0.002, 0.0), (0.0, -0.002)] {
            let gpx = format!(
                r#"<gpx><trk><trkseg><trkpt lat="0" lon="0"><ele>400</ele></trkpt><trkpt lat="{lat}" lon="{lon}"><ele>400</ele></trkpt></trkseg></trk></gpx>"#
            );
            let route = Route::from_gpx(&gpx, None).await.unwrap();
            let widths = RoadWidths::default();
            let distance = Meters(100.0);
            let at = route.position(distance);
            let centre = LocalProjection::for_route(&route).project(at.lat, at.lon);
            for left in [false, true] {
                let sides = vec![Some(left); route.points().len()];
                let side = if left { -1.0 } else { 1.0 };
                for ghost in [false, true] {
                    let (east, north) = position(&route, &sides, &widths, distance, ghost);
                    let right = (east - centre.0) * at.heading.cos()
                        - (north - centre.1) * at.heading.sin();
                    assert!(right * side > 0.0 && right.abs() < 3.0);
                    assert!((right.abs() - if ghost { 0.525 } else { 1.5 }).abs() < 1e-6);
                }
            }
        }
    }
}
