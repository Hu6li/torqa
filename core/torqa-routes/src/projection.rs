//! Flat local coordinates around a route.

use crate::Route;

const EARTH_RADIUS: f64 = 6_371_000.0;

/// Converts positions to metres east/north of the route start (equirectangular projection,
/// accurate to well under 1 % over the extent of a ride).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalProjection {
    origin_lat: f64,
    origin_lon: f64,
    meters_per_degree_lon: f64,
}

impl LocalProjection {
    /// A projection centred on the first point of `route`.
    #[must_use]
    pub fn for_route(route: &Route) -> Self {
        let start = route.points()[0];
        Self {
            origin_lat: start.lat,
            origin_lon: start.lon,
            meters_per_degree_lon: EARTH_RADIUS.to_radians() * start.lat.to_radians().cos(),
        }
    }

    /// Metres east and north of the origin.
    #[must_use]
    pub fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        (
            (lon - self.origin_lon) * self.meters_per_degree_lon,
            (lat - self.origin_lat) * EARTH_RADIUS.to_radians(),
        )
    }

    /// Latitude and longitude of a point `east`/`north` metres from the origin.
    #[must_use]
    pub fn unproject(&self, east: f64, north: f64) -> (f64, f64) {
        (
            self.origin_lat + north / EARTH_RADIUS.to_radians(),
            self.origin_lon + east / self.meters_per_degree_lon,
        )
    }
}
