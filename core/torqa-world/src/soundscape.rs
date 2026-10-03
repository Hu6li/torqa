//! What surrounds the road, for the ambient sound (R26): forest, water and town along the route.

use torqa_domain::units::Meters;
use torqa_osm::LandCover;
use torqa_routes::{LocalProjection, Route};

use crate::landcover::LandIndex;

/// Distance between soundscape samples along the route.
pub const SOUNDSCAPE_STEP: f64 = 50.0;
/// Sideways distances from the road at which the land is sampled, on both sides.
const OFFSETS: [f64; 3] = [15.0, 40.0, 80.0];

/// Shares of the land around one point of the road, each 0–1; the rest is open land (fields,
/// meadows, rock or unmapped).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Surroundings {
    /// Forest or woodland: birds.
    pub forest: f32,
    /// Lakes and rivers: lapping water.
    pub water: f32,
    /// Built-up areas: no birdsong, more echo.
    pub town: f32,
}

/// The surroundings every [`SOUNDSCAPE_STEP`] metres along `route`, from the start.
pub(crate) fn along(
    route: &Route,
    projection: &LocalProjection,
    land: &LandIndex,
) -> Vec<Surroundings> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // routes are short
    let steps = (route.length().0 / SOUNDSCAPE_STEP).ceil() as usize + 1;
    (0..steps)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let at = route.position(Meters(i as f64 * SOUNDSCAPE_STEP));
            let (east, north) = projection.project(at.lat, at.lon);
            // Headings run clockwise from north; the left of the road is a quarter turn back.
            let (left_east, left_north) = (-at.heading.cos(), at.heading.sin());
            let mut counts = [0u8; 3];
            let mut samples = 0u8;
            for offset in OFFSETS {
                for side in [-1.0, 1.0] {
                    let cover = land.cover_at(
                        east + left_east * offset * side,
                        north + left_north * offset * side,
                    );
                    samples += 1;
                    match cover {
                        Some(LandCover::Forest) => counts[0] += 1,
                        Some(LandCover::Water) => counts[1] += 1,
                        Some(LandCover::Residential) => counts[2] += 1,
                        _ => {}
                    }
                }
            }
            let share = |count: u8| f32::from(count) / f32::from(samples);
            Surroundings {
                forest: share(counts[0]),
                water: share(counts[1]),
                town: share(counts[2]),
            }
        })
        .collect()
}
