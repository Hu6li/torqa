//! A course's map picture (#192, #203): the land around the whole route in black and greys,
//! after CARTO's Dark Matter, with its waters, roads and the names of its towns, framed for the
//! course's card. Drawn once when the course is prepared, from map tiles as detailed as the
//! frame needs, and kept in the course file as a picture.

use std::collections::HashSet;

pub use torqa_osm::PlaceKind;
use torqa_osm::{LandCover, LatLon, MapData, RoadClass};
use torqa_routes::LocalProjection;

use crate::minimap::{FlatMap, fill, simplify};

/// The picture's size in pixels: twice the course card's, so it stays sharp on the course
/// page and on high-density screens.
pub const PICTURE: [u32; 2] = [1072, 640];
/// Land, the picture's background.
pub const LAND: [f32; 4] = [0.055, 0.055, 0.055, 1.0];
const FOREST: [f32; 4] = [0.075, 0.075, 0.075, 1.0];
const BUILT: [f32; 4] = [0.115, 0.115, 0.115, 1.0];
const WATER: [f32; 4] = [0.17, 0.17, 0.17, 1.0];
const MINOR_ROAD: [f32; 4] = [0.15, 0.15, 0.15, 1.0];
const MAJOR_ROAD: [f32; 4] = [0.27, 0.27, 0.27, 1.0];
/// Room between the route and the picture's edges, as a share of its width and height.
const MARGIN: f64 = 0.08;
/// The narrowest frame: a short loop still shows the land around it.
const MIN_WIDTH_M: f64 = 3000.0;
/// `OpenMapTiles` simplifies each tile for drawing it this many pixels wide.
const TILE_PIXELS: f64 = 512.0;
/// The equator's length in Web Mercator, in metres.
const EQUATOR_M: f64 = 40_075_016.686;
/// Tiles of zoom 4 show whole countries; 14 is the most detailed there is.
const ZOOMS: std::ops::RangeInclusive<u8> = 4..=14;
/// Shapes are simplified to this share of a pixel; finer detail is invisible.
const DETAIL_PX: f64 = 0.5;
/// Names offered for the picture; it places the most important that fit.
const MAX_LABELS: usize = 40;
/// Places this close to the route lie along it: the route passes through them.
const ALONG_M: f64 = 1000.0;

/// The land a course's picture shows, in metres east/north of the route's start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Western edge.
    pub west: f64,
    /// Southern edge.
    pub south: f64,
    /// Eastern edge.
    pub east: f64,
    /// Northern edge.
    pub north: f64,
}

impl Frame {
    /// The frame for a route's `track` (metres east/north of its start): the whole route with
    /// room around it, in the picture's proportions. `None` without points.
    #[must_use]
    pub fn around(track: &[(f64, f64)]) -> Option<Self> {
        let &(e0, n0) = track.first()?;
        let (mut west, mut south, mut east, mut north) = (e0, n0, e0, n0);
        for &(e, n) in track {
            (west, south) = (west.min(e), south.min(n));
            (east, north) = (east.max(e), north.max(n));
        }
        let aspect = f64::from(PICTURE[0]) / f64::from(PICTURE[1]);
        let usable = 1.0 - 2.0 * MARGIN;
        let width = ((east - west) / usable)
            .max((north - south) / usable * aspect)
            .max(MIN_WIDTH_M);
        let height = width / aspect;
        let (centre_e, centre_n) = (f64::midpoint(west, east), f64::midpoint(south, north));
        Some(Self {
            west: centre_e - width / 2.0,
            south: centre_n - height / 2.0,
            east: centre_e + width / 2.0,
            north: centre_n + height / 2.0,
        })
    }

    /// Width in metres.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.east - self.west
    }

    /// Height in metres.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.north - self.south
    }

    /// Metres of land in a pixel of the picture.
    #[must_use]
    pub fn metres_per_pixel(&self) -> f64 {
        self.width() / f64::from(PICTURE[0])
    }

    /// The south-west and north-east corners as latitude/longitude.
    #[must_use]
    pub fn corners(&self, projection: &LocalProjection) -> (LatLon, LatLon) {
        (
            projection.unproject(self.west, self.south),
            projection.unproject(self.east, self.north),
        )
    }

    /// The zoom of the map tiles detailed enough for the picture and no more: each tile drawn
    /// about as large as its data is made for. Larger frames take coarser tiles, so a course
    /// of any length needs only a few.
    #[must_use]
    pub fn zoom(&self, projection: &LocalProjection) -> u8 {
        let (latitude, _) = projection.unproject(
            f64::midpoint(self.west, self.east),
            f64::midpoint(self.south, self.north),
        );
        let tiles =
            EQUATOR_M * latitude.to_radians().cos() / (TILE_PIXELS * self.metres_per_pixel());
        // Clamped into the zooms there are.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let zoom = tiles
            .log2()
            .round()
            .clamp(f64::from(*ZOOMS.start()), f64::from(*ZOOMS.end())) as u8;
        zoom
    }

    fn contains(&self, (e, n): (f64, f64)) -> bool {
        (self.west..=self.east).contains(&e) && (self.south..=self.north).contains(&n)
    }

    /// Whether any part of the box around `points` lies in the frame.
    fn touches(&self, points: &[(f64, f64)]) -> bool {
        let (mut west, mut south) = (f64::INFINITY, f64::INFINITY);
        let (mut east, mut north) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for &(e, n) in points {
            (west, south) = (west.min(e), south.min(n));
            (east, north) = (east.max(e), north.max(n));
        }
        west <= self.east && east >= self.west && south <= self.north && north >= self.south
    }
}

/// A river or road on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Points in metres east/north of the route's start.
    pub points: Vec<[f32; 2]>,
    /// Width in pixels of the picture.
    pub width: f32,
    /// Colour.
    pub color: [f32; 4],
}

/// The name of a city, town or village on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// The name, in the local language.
    pub name: String,
    /// The name in Latin letters, for where the local one cannot be shown; empty if unknown.
    pub latin: String,
    /// City, town or village: how prominent its name is.
    pub kind: PlaceKind,
    /// Where the name belongs, in metres east/north of the route's start.
    pub position: [f32; 2],
}

/// A course's map, ready to draw under its route.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseMap {
    /// The land shown.
    pub frame: Frame,
    /// Woods, towns and lakes as coloured triangles, in metres east/north of the route's start.
    pub areas: FlatMap,
    /// Rivers, then roads, minor before major: in drawing order.
    pub lines: Vec<Line>,
    /// Names in the frame, most important first.
    pub labels: Vec<Label>,
}

/// Draws `map` within `frame`, all of it rather than the route's corridor (#203), for the route
/// along `track` (metres east/north of its start).
#[must_use]
pub fn build(
    map: &MapData,
    projection: &LocalProjection,
    track: &[(f64, f64)],
    frame: Frame,
) -> CourseMap {
    let pixel = frame.metres_per_pixel();
    let project = |ring: &[LatLon]| -> Vec<(f64, f64)> {
        ring.iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect()
    };

    let mut areas = FlatMap::default();
    // Woods first: towns lie in their clearings, lakes in the towns.
    for (covers, color) in [
        (&[LandCover::Forest][..], FOREST),
        (
            &[
                LandCover::Residential,
                LandCover::Industrial,
                LandCover::Commercial,
            ][..],
            BUILT,
        ),
        (&[LandCover::Water][..], WATER),
    ] {
        for area in map.areas.iter().filter(|a| covers.contains(&a.cover)) {
            for ring in &area.outer {
                let ring = project(ring);
                if frame.touches(&ring) {
                    fill(&mut areas, &ring, color, pixel * DETAIL_PX);
                }
            }
            // Holes show the land, approximating the cut-out.
            for ring in &area.inner {
                let ring = project(ring);
                if frame.touches(&ring) {
                    fill(&mut areas, &ring, LAND, pixel * DETAIL_PX);
                }
            }
        }
    }

    let mut lines = Vec::new();
    let mut line = |points: &[(f64, f64)], width: f32, color: [f32; 4]| {
        if !frame.touches(points) {
            return;
        }
        let points = simplify(points, pixel * DETAIL_PX);
        if points.len() >= 2 {
            lines.push(Line {
                // Drawing precision.
                #[allow(clippy::cast_possible_truncation)]
                points: points.iter().map(|&(e, n)| [e as f32, n as f32]).collect(),
                width,
                color,
            });
        }
    };
    // Rivers and canals; streams would clutter the map.
    for waterway in map.waterways.iter().filter(|w| w.width >= 8.0) {
        line(&project(&waterway.line), 2.0, WATER);
    }
    for (class, width, color) in [
        (RoadClass::Street, 1.2, MINOR_ROAD),
        (RoadClass::Major, 2.4, MAJOR_ROAD),
    ] {
        for road in map.roads.iter().filter(|r| r.class == class) {
            line(&project(&road.line), width, color);
        }
    }

    // Cities first, then towns, then villages; of each, those the route passes through first.
    let mut places: Vec<_> = map
        .places
        .iter()
        .map(|p| (p, projection.project(p.position.0, p.position.1)))
        .filter(|&(_, at)| frame.contains(at))
        .map(|(p, at)| (p, at, distance(track, at) >= ALONG_M))
        .collect();
    places.sort_by(|(a, _, a_off), (b, _, b_off)| {
        (a.kind, a_off, a.rank, &a.name).cmp(&(b.kind, b_off, b.rank, &b.name))
    });
    // A place mapped twice (as a node and an area, say) is named once.
    let mut named = HashSet::new();
    places.retain(|(place, ..)| named.insert(&place.name));
    let labels = places
        .into_iter()
        .take(MAX_LABELS)
        .map(|(place, (e, n), _)| Label {
            name: place.name.clone(),
            latin: place.latin.clone(),
            kind: place.kind,
            // Drawing precision.
            #[allow(clippy::cast_possible_truncation)]
            position: [e as f32, n as f32],
        })
        .collect();

    CourseMap {
        frame,
        areas,
        lines,
        labels,
    }
}

/// Distance from `point` to the polyline `line`.
fn distance(line: &[(f64, f64)], (e, n): (f64, f64)) -> f64 {
    let to_segment = |(ae, an): (f64, f64), (be, bn): (f64, f64)| {
        let (de, dn) = (be - ae, bn - an);
        let length2 = de * de + dn * dn;
        let t = if length2 > 0.0 {
            (((e - ae) * de + (n - an) * dn) / length2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (ae + t * de - e).hypot(an + t * dn - n)
    };
    match line {
        [] => f64::INFINITY,
        [only] => to_segment(*only, *only),
        _ => line
            .windows(2)
            .map(|pair| to_segment(pair[0], pair[1]))
            .fold(f64::INFINITY, f64::min),
    }
}

#[cfg(test)]
mod tests {
    use torqa_osm::{Area, Place, Road};

    use super::*;

    /// A projection centred at Meiringen, 46.7° N.
    fn meiringen() -> LocalProjection {
        let gpx = r#"<gpx><trk><trkseg>
            <trkpt lat="46.7277" lon="8.1872"><ele>600</ele></trkpt>
            <trkpt lat="46.7300" lon="8.1900"><ele>600</ele></trkpt>
        </trkseg></trk></gpx>"#;
        let route = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(torqa_routes::Route::from_gpx(gpx, None))
            .unwrap();
        LocalProjection::for_route(&route)
    }

    fn square(projection: &LocalProjection, (e, n): (f64, f64), side: f64) -> Vec<LatLon> {
        [
            (0.0, 0.0),
            (side, 0.0),
            (side, side),
            (0.0, side),
            (0.0, 0.0),
        ]
        .iter()
        .map(|&(de, dn)| projection.unproject(e + de, n + dn))
        .collect()
    }

    #[test]
    fn the_frame_holds_the_whole_route_with_room_in_the_pictures_proportions() {
        // A route 40 km east and 10 km north: wide, as the picture is.
        let track = [(0.0, 0.0), (20_000.0, 10_000.0), (40_000.0, 2_000.0)];

        let frame = Frame::around(&track).unwrap();

        for &point in &track {
            assert!(frame.contains(point));
        }
        assert!(frame.west < -1000.0 && frame.east > 41_000.0);
        let aspect = f64::from(PICTURE[0]) / f64::from(PICTURE[1]);
        assert!((frame.width() / frame.height() - aspect).abs() < 1e-9);
        // A tall route fills the picture's height instead.
        let tall = Frame::around(&[(0.0, 0.0), (0.0, 40_000.0)]).unwrap();
        assert!(tall.south < -1000.0 && tall.north > 41_000.0);
        assert!((tall.width() / tall.height() - aspect).abs() < 1e-9);
    }

    #[test]
    fn a_short_route_still_shows_the_land_around_it() {
        let frame = Frame::around(&[(0.0, 0.0), (300.0, 100.0)]).unwrap();

        assert!(frame.width() >= MIN_WIDTH_M);
        assert!(frame.contains((150.0, 50.0)));
    }

    #[test]
    fn larger_frames_take_coarser_tiles() {
        let projection = meiringen();
        let zoom = |width: f64| {
            Frame::around(&[(0.0, 0.0), (width, 0.0)])
                .unwrap()
                .zoom(&projection)
        };

        // A 50 km pass loop, a 10 km climb, a lap of a park.
        let (long, middle, short) = (zoom(50_000.0), zoom(10_000.0), zoom(1_000.0));
        assert!(long < middle && middle < short, "{long} {middle} {short}");
        assert!((9..=11).contains(&long), "{long}");
        // A tile at the frame's zoom is drawn about as wide as its data is made for.
        let frame = Frame::around(&[(0.0, 0.0), (50_000.0, 0.0)]).unwrap();
        let tile_m = EQUATOR_M * 46.7_f64.to_radians().cos() / f64::from(1u32 << long);
        let tile_px = tile_m / frame.metres_per_pixel();
        assert!(
            (TILE_PIXELS / 1.5..TILE_PIXELS * 1.5).contains(&tile_px),
            "{tile_px}"
        );
        // Across a continent, still a few tiles; never finer than the finest there are.
        assert_eq!(zoom(3_000_000.0), 4);
        assert_eq!(zoom(10.0), 14);
    }

    #[test]
    fn the_map_shows_the_land_beyond_the_routes_corridor() {
        // #203: a lake 8 km from a 20 km route lies in its picture, though far outside the
        // corridor the world is built in.
        let projection = meiringen();
        let track = [(0.0, 0.0), (20_000.0, 0.0)];
        let frame = Frame::around(&track).unwrap();
        let lake = Area {
            cover: LandCover::Water,
            outer: vec![square(&projection, (10_000.0, 4_000.0), 1500.0)],
            inner: Vec::new(),
        };
        let far_lake = Area {
            outer: vec![square(&projection, (10_000.0, 40_000.0), 1500.0)],
            ..lake.clone()
        };
        let map = MapData {
            areas: vec![lake, far_lake],
            ..MapData::default()
        };

        let drawn = build(&map, &projection, &track, frame);

        assert_ne!(drawn.areas.vertices.len(), 0);
        // Only the lake in the frame.
        assert!(
            drawn
                .areas
                .vertices
                .iter()
                .all(|&[_, n]| f64::from(n) < frame.north)
        );
        assert!(drawn.areas.colors.iter().all(|&c| c == WATER));
    }

    #[test]
    fn major_roads_are_drawn_over_minor_ones_and_paths_are_left_out() {
        let projection = meiringen();
        let track = [(0.0, 0.0), (5_000.0, 0.0)];
        let frame = Frame::around(&track).unwrap();
        let road = |class| Road {
            class,
            line: vec![
                projection.unproject(0.0, 0.0),
                projection.unproject(4_000.0, 0.0),
            ],
            structure: None,
        };
        let map = MapData {
            roads: vec![
                road(RoadClass::Major),
                road(RoadClass::Path),
                road(RoadClass::Street),
            ],
            ..MapData::default()
        };

        let drawn = build(&map, &projection, &track, frame);

        let colors: Vec<_> = drawn.lines.iter().map(|l| l.color).collect();
        assert_eq!(colors, [MINOR_ROAD, MAJOR_ROAD]);
        assert!(drawn.lines[1].width > drawn.lines[0].width);
    }

    #[test]
    fn names_the_places_in_the_frame_most_important_first() {
        let projection = meiringen();
        let track = [(0.0, 0.0), (20_000.0, 0.0)];
        let frame = Frame::around(&track).unwrap();
        let place = |name: &str, kind, rank, (e, n)| Place {
            name: name.to_owned(),
            latin: name.to_owned(),
            kind,
            rank,
            position: projection.unproject(e, n),
        };
        let map = MapData {
            places: vec![
                place("Brienz", PlaceKind::Village, 5, (5_000.0, -4_000.0)),
                place("Guttannen", PlaceKind::Village, 11, (15_000.0, 300.0)),
                place("Meiringen", PlaceKind::Village, 10, (0.0, 0.0)),
                place("Interlaken", PlaceKind::Town, 11, (5_000.0, 3_000.0)),
                place("Luzern", PlaceKind::City, 7, (10_000.0, 60_000.0)),
                // Mapped twice.
                place("Meiringen", PlaceKind::Village, 10, (100.0, 0.0)),
            ],
            ..MapData::default()
        };

        let drawn = build(&map, &projection, &track, frame);

        let names: Vec<_> = drawn.labels.iter().map(|l| l.name.as_str()).collect();
        // Luzern lies outside the picture; the villages the route passes through come before
        // Brienz, larger as it is.
        assert_eq!(names, ["Interlaken", "Meiringen", "Guttannen", "Brienz"]);
        assert!((drawn.labels[0].position[0] - 5_000.0).abs() < 1.0);
    }
}
