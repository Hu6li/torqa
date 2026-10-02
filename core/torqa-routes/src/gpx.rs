//! Minimal GPX 1.0/1.1 reader: track points (or route points) with optional elevation.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::RouteError;

/// A position as recorded in the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RawPoint {
    pub(crate) lat: f64,
    pub(crate) lon: f64,
    pub(crate) elevation: Option<f64>,
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
    })
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
                    elevation: Some(540.5)
                },
                RawPoint {
                    lat: 46.91,
                    lon: 7.41,
                    elevation: None
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
