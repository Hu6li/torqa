//! Minimal GPX 1.0/1.1 reader: track points (or route points) with optional elevation and time.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::RouteError;

/// A position as recorded in the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RawPoint {
    pub(crate) lat: f64,
    pub(crate) lon: f64,
    pub(crate) elevation: Option<f64>,
    /// Seconds since the Unix epoch, for recorded activities.
    pub(crate) time: Option<f64>,
}

/// The parsed contents of a GPX file.
#[derive(Debug, Default)]
pub(crate) struct Gpx {
    pub(crate) name: Option<String>,
    pub(crate) points: Vec<RawPoint>,
}

#[derive(Clone, Copy, PartialEq)]
enum Capture {
    None,
    Name,
    Elevation,
    Time,
}

/// Reads track points; falls back to route points for files from route planners that only
/// contain `<rte>`. Waypoints are ignored.
pub(crate) fn parse(xml: &str) -> Result<Gpx, RouteError> {
    let mut reader = Reader::from_str(xml);
    let mut gpx = Gpx::default();
    let mut route_points = Vec::new();

    let mut point: Option<(RawPoint, bool)> = None; // (point, is track point)
    let mut in_waypoint = false;
    let mut capture = Capture::None;
    let mut text = String::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|e| RouteError::InvalidGpx(e.to_string()))?;
        match event {
            Event::Start(e) => {
                match e.local_name().as_ref() {
                    "trkpt" | "rtept" => {
                        point = Some((read_point(&e)?, e.local_name().as_ref() == "trkpt"));
                    }
                    "wpt" => in_waypoint = true,
                    "ele" if point.is_some() => capture = Capture::Elevation,
                    "time" if point.is_some() => capture = Capture::Time,
                    "name" if point.is_none() && !in_waypoint && gpx.name.is_none() => {
                        capture = Capture::Name;
                    }
                    _ => {}
                }
                text.clear();
            }
            Event::Empty(e) => match e.local_name().as_ref() {
                "trkpt" => gpx.points.push(read_point(&e)?),
                "rtept" => route_points.push(read_point(&e)?),
                _ => {}
            },
            Event::Text(t) if capture != Capture::None => text.push_str(&t.xml10_content()),
            Event::CData(t) if capture != Capture::None => text.push_str(&t),
            Event::GeneralRef(r) if capture != Capture::None => {
                if let Some(c) = resolve_entity(&r) {
                    text.push(c);
                }
            }
            Event::End(e) => {
                match (e.local_name().as_ref(), capture) {
                    ("ele", Capture::Elevation) => {
                        if let Some((p, _)) = point.as_mut() {
                            p.elevation = text.trim().parse().ok();
                        }
                    }
                    ("time", Capture::Time) => {
                        if let Some((p, _)) = point.as_mut() {
                            p.time = parse_time(text.trim());
                        }
                    }
                    ("name", Capture::Name) => {
                        let name = text.trim();
                        if !name.is_empty() {
                            gpx.name = Some(name.to_owned());
                        }
                    }
                    ("trkpt" | "rtept", _) => {
                        if let Some((p, is_track)) = point.take() {
                            if is_track {
                                gpx.points.push(p);
                            } else {
                                route_points.push(p);
                            }
                        }
                    }
                    ("wpt", _) => in_waypoint = false,
                    _ => {}
                }
                capture = Capture::None;
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if gpx.points.is_empty() {
        gpx.points = route_points;
    }
    Ok(gpx)
}

fn read_point(e: &BytesStart<'_>) -> Result<RawPoint, RouteError> {
    let coordinate = |key: &str| -> Result<f64, RouteError> {
        let attribute = e
            .try_get_attribute(key)
            .map_err(|err| RouteError::InvalidGpx(err.to_string()))?
            .ok_or_else(|| RouteError::InvalidGpx(format!("point without {key}")))?;
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| RouteError::InvalidGpx(err.to_string()))?;
        value
            .trim()
            .parse()
            .map_err(|_| RouteError::InvalidGpx(format!("invalid {key} {value:?}")))
    };
    let (lat, lon) = (coordinate("lat")?, coordinate("lon")?);
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(RouteError::InvalidGpx(format!(
            "coordinate out of range: {lat}, {lon}"
        )));
    }
    Ok(RawPoint {
        lat,
        lon,
        elevation: None,
        time: None,
    })
}

/// Seconds since the Unix epoch from an ISO 8601 / XML Schema date-time as GPX uses it, e.g.
/// `2026-10-03T07:15:30Z`, `…30.250Z` or `…30+02:00`. `None` if malformed.
pub(crate) fn parse_time(text: &str) -> Option<f64> {
    let (date, rest) = text.split_once('T')?;
    let mut date_parts = date.splitn(3, '-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    // The zone starts at Z, + or the first - after the time.
    let zone_at = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(zone_at);
    let mut clock_parts = clock.splitn(3, ':');
    let hour: f64 = clock_parts.next()?.parse().ok()?;
    let minute: f64 = clock_parts.next()?.parse().ok()?;
    let second: f64 = clock_parts.next()?.parse().ok()?;
    let offset = match zone {
        "" | "Z" => 0.0,
        _ => {
            let sign = if zone.starts_with('-') { -1.0 } else { 1.0 };
            let (h, m) = zone[1..].split_once(':')?;
            sign * (h.parse::<f64>().ok()? * 3600.0 + m.parse::<f64>().ok()? * 60.0)
        }
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    #[allow(clippy::cast_precision_loss)] // day counts are far below 2^52
    let days = days as f64;
    Some(days * 86_400.0 + hour * 3600.0 + minute * 60.0 + second - offset)
}

fn resolve_entity(reference: &quick_xml::events::BytesRef<'_>) -> Option<char> {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        return Some(c);
    }
    match &**reference {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

#[cfg(test)]
mod time_tests {
    use super::parse_time;

    #[test]
    fn reads_gpx_timestamps() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_time("2000-03-01T00:00:00Z"), Some(951_868_800.0));
        assert_eq!(parse_time("2026-10-03T07:15:30Z"), Some(1_791_011_730.0));
        // The same moment in Central European Summer Time, with fractional seconds.
        assert_eq!(
            parse_time("2026-10-03T09:15:30.5+02:00"),
            Some(1_791_011_730.5)
        );
        assert_eq!(parse_time("2026-10-03T07:15:30"), Some(1_791_011_730.0));
        assert_eq!(parse_time("yesterday"), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_track_points_with_elevation_and_name() {
        let xml = r#"<?xml version="1.0"?>
            <gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1">
              <metadata><name>Gurten &amp; back</name></metadata>
              <wpt lat="1" lon="1"><name>Ignored waypoint</name></wpt>
              <trk><name>Track</name><trkseg>
                <trkpt lat="46.9" lon="7.4"><ele>540.5</ele><time>2026-01-01T00:00:00Z</time></trkpt>
                <trkpt lat="46.91" lon="7.41"/>
              </trkseg></trk>
            </gpx>"#;

        let gpx = parse(xml).unwrap();

        assert_eq!(gpx.name.as_deref(), Some("Gurten & back"));
        assert_eq!(
            gpx.points,
            [
                RawPoint {
                    lat: 46.9,
                    lon: 7.4,
                    elevation: Some(540.5),
                    time: Some(1_767_225_600.0),
                },
                RawPoint {
                    lat: 46.91,
                    lon: 7.41,
                    elevation: None,
                    time: None,
                },
            ]
        );
    }

    #[test]
    fn falls_back_to_route_points() {
        let xml = r#"<gpx><rte><name>Planned</name>
            <rtept lat="47.0" lon="8.0"><ele>400</ele></rtept>
            <rtept lat="47.1" lon="8.1"><ele>410</ele></rtept>
        </rte></gpx>"#;

        let gpx = parse(xml).unwrap();

        assert_eq!(gpx.name.as_deref(), Some("Planned"));
        assert_eq!(gpx.points.len(), 2);
        assert_eq!(gpx.points[1].elevation, Some(410.0));
    }

    #[test]
    fn rejects_invalid_coordinates() {
        assert!(
            parse(r#"<gpx><trk><trkseg><trkpt lat="95" lon="0"/></trkseg></trk></gpx>"#).is_err()
        );
        assert!(
            parse(r#"<gpx><trk><trkseg><trkpt lat="x" lon="0"/></trkseg></trk></gpx>"#).is_err()
        );
    }
}
