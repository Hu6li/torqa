//! Disposable winter overlays derived from the retained road, never stored with the world.

use std::collections::{BTreeMap, BTreeSet};

use orqwa_domain::units::Meters;
use orqwa_routes::{RoutePoint, Surface};

use crate::{MeshData, road::RING};

/// A bounded batch of roadside banks, with vertices relative to its centre.
#[derive(Debug)]
pub struct Snowbank {
    /// World-space batch centre, including elevation, for distance culling.
    pub center: [f32; 3],
    /// Closed rounded banks outside the ridden carriageway.
    pub mesh: MeshData,
}

/// Approximate settled depth: ten centimetres near the snow line, increasing with altitude.
#[must_use]
pub fn snow_depth(elevation: Meters, snow_line: Meters) -> Meters {
    let above = elevation.0 - snow_line.0;
    let t = ((above + 60.0) / 120.0).clamp(0.0, 1.0);
    Meters((0.10 + above.max(0.0) * 0.0002).min(0.70) * t * t * (3.0 - 2.0 * t))
}

/// Runtime road UV2 exposure: tunnel interiors stay clear, including retained worlds.
#[must_use]
pub fn road_snow_exposure(road: &MeshData, points: &[RoutePoint]) -> Vec<[f32; 2]> {
    road.uvs
        .iter()
        .map(|uv| {
            let index = points
                .partition_point(|point| point.distance.0 <= f64::from(uv[1]))
                .saturating_sub(1);
            let covered = points
                .get(index)
                .is_some_and(|point| point.surface == Surface::Tunnel);
            [if covered { 0.0 } else { 1.0 }, 0.0]
        })
        .collect()
}

/// Snow outside the actual road edges, excluding structures and joining-street openings.
/// Geometry is partitioned into 480 m cells so long courses do not draw all banks at once.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // bounded GPU positions and spatial cell indices
pub fn snowbanks(road: &MeshData, points: &[RoutePoint], snow_line: Meters) -> Vec<Snowbank> {
    let mut links = BTreeSet::new();
    for &[a, b, c] in road.indices.as_chunks::<3>().0 {
        let low = a.min(b).min(c) as usize / RING;
        let high = a.max(b).max(c) as usize / RING;
        if high == low + 1 {
            links.insert((low, high));
        }
    }
    let mut cells = BTreeMap::<(i32, i32), MeshData>::new();
    for (a, b) in links {
        if b * RING + RING > road.vertices.len() || b * RING + RING > road.uvs.len() {
            continue;
        }
        let from = f64::from(road.uvs[a * RING][1]);
        let to = f64::from(road.uvs[b * RING][1]);
        let first = points
            .partition_point(|point| point.distance.0 < from)
            .saturating_sub(1);
        let end = points.partition_point(|point| point.distance.0 <= to);
        if points[first..end]
            .iter()
            .any(|point| point.surface != Surface::Ground)
        {
            continue;
        }
        let midpoint = road.vertices[a * RING + 2];
        let key = (
            (midpoint[0] / 480.0).floor() as i32,
            (midpoint[2] / 480.0).floor() as i32,
        );
        let mesh = cells.entry(key).or_default();
        for (edge, bevel, skirt) in [(2, 1, 0), (3, 4, 5)] {
            // Joined shoulders have the edge's u; bridge shoulders do too, including old worlds.
            if [a, b].iter().any(|&ring| {
                (road.uvs[ring * RING + bevel][0] - road.uvs[ring * RING + edge][0]).abs() < 0.01
            }) {
                continue;
            }
            let profiles = [a, b].map(|ring| profile(road, ring, edge, skirt, snow_line));
            if profiles.iter().all(|(_, depth)| *depth < 0.005) {
                continue;
            }
            prism(mesh, profiles.map(|(vertices, _)| vertices));
        }
    }
    cells
        .into_values()
        .filter(|mesh| !mesh.vertices.is_empty())
        .map(|mut mesh| {
            let mut low = [f32::INFINITY; 3];
            let mut high = [f32::NEG_INFINITY; 3];
            for point in &mesh.vertices {
                for axis in 0..3 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
            let center = std::array::from_fn(|axis| low[axis].midpoint(high[axis]));
            for point in &mut mesh.vertices {
                for axis in 0..3 {
                    point[axis] -= center[axis];
                }
            }
            Snowbank { center, mesh }
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation)] // metre elevations become GPU positions
fn profile(
    road: &MeshData,
    ring: usize,
    edge: usize,
    skirt: usize,
    line: Meters,
) -> ([[f32; 3]; 6], f32) {
    let inner = road.vertices[ring * RING + edge];
    let outer = road.vertices[ring * RING + skirt];
    let bevel = road.vertices[ring * RING + if edge == 2 { 1 } else { 4 }];
    let depth = snow_depth(Meters(f64::from(inner[1])), line).0 as f32;
    let width = ((outer[0] - inner[0]).powi(2) + (outer[2] - inner[2]).powi(2)).sqrt();
    // Keep the base on the drawn shoulder rather than the uncarved terrain behind it.
    let section: [(f32, f32); 6] = [
        (0.06, 0.0),
        (0.10, 0.65),
        (0.25, 1.0),
        (0.50, 0.95),
        (0.80, 0.60),
        (1.1, 0.0),
    ];
    (
        section.map(|(offset, height)| {
            let offset = offset.min(width * 0.95);
            let t = (offset / width.max(0.01)).min(0.95);
            let bevel_width =
                ((bevel[0] - inner[0]).powi(2) + (bevel[2] - inner[2]).powi(2)).sqrt();
            let base = if offset <= bevel_width {
                inner[1] + (bevel[1] - inner[1]) * offset / bevel_width.max(0.01)
            } else {
                bevel[1]
                    + (outer[1] - bevel[1]) * (offset - bevel_width)
                        / (width - bevel_width).max(0.01)
            };
            [
                inner[0] + (outer[0] - inner[0]) * t,
                base * (1.0 - height) + (inner[1] + depth) * height,
                inner[2] + (outer[2] - inner[2]) * t,
            ]
        }),
        depth,
    )
}

fn prism(mesh: &mut MeshData, mut profiles: [[[f32; 3]; 6]; 2]) {
    if face_normal([profiles[0][2], profiles[1][2], profiles[1][3]])[1] < 0.0 {
        profiles.swap(0, 1);
    }
    let smooth: [[f32; 3]; 6] = std::array::from_fn(|side| {
        face_normal([
            profiles[0][side.saturating_sub(1)],
            profiles[1][side],
            profiles[0][(side + 1).min(5)],
        ])
    });
    // Duplicate face vertices for robust caps and smooth each curved cross-section's normals.
    for side in 0..6 {
        let next = (side + 1) % 6;
        quad(
            mesh,
            [
                profiles[0][side],
                profiles[1][side],
                profiles[1][next],
                profiles[0][next],
            ],
        );
        if side < 5 {
            let start = mesh.normals.len() - 6;
            for (index, profile_index) in
                [side, side, next, side, next, next].into_iter().enumerate()
            {
                mesh.normals[start + index] = smooth[profile_index];
            }
        }
    }
    for (end, profile) in profiles.into_iter().enumerate() {
        for side in 1..5 {
            let triangle = if end == 0 {
                [profile[0], profile[side], profile[side + 1]]
            } else {
                [profile[0], profile[side + 1], profile[side]]
            };
            triangle_mesh(mesh, triangle);
        }
    }
}

fn quad(mesh: &mut MeshData, vertices: [[f32; 3]; 4]) {
    triangle_mesh(mesh, [vertices[0], vertices[1], vertices[2]]);
    triangle_mesh(mesh, [vertices[0], vertices[2], vertices[3]]);
}

fn triangle_mesh(mesh: &mut MeshData, vertices: [[f32; 3]; 3]) {
    let normal = face_normal(vertices);
    for vertex in vertices {
        mesh.indices
            .push(u32::try_from(mesh.vertices.len()).expect("bounded snow batch"));
        mesh.vertices.push(vertex);
        mesh.normals.push(normal);
        mesh.uvs.push([vertex[0], vertex[2]]);
    }
}

fn face_normal(vertices: [[f32; 3]; 3]) -> [f32; 3] {
    let a = std::array::from_fn::<_, 3, _>(|i| vertices[1][i] - vertices[0][i]);
    let b = std::array::from_fn::<_, 3, _>(|i| vertices[2][i] - vertices[0][i]);
    let normal = [
        a[2] * b[1] - a[1] * b[2],
        a[0] * b[2] - a[2] * b[0],
        a[1] * b[0] - a[0] * b[1],
    ];
    let length = normal.iter().map(|n| n * n).sum::<f32>().sqrt().max(1e-8);
    normal.map(|n| n / length)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road() -> MeshData {
        let mut road = MeshData::default();
        for z in [0.0, 4.0, 8.0] {
            for (x, u) in [
                (-4.0, -0.2),
                (-3.3, -0.08),
                (-3.0, 0.0),
                (3.0, 1.0),
                (3.3, 1.08),
                (4.0, 1.2),
            ] {
                road.vertices.push([x, 700.0, z]);
                road.uvs.push([u, z]);
            }
        }
        road.indices = vec![2, 8, 9, 8, 14, 15];
        road
    }

    #[test]
    fn depth_grows_above_the_climate_line_and_tropical_lowlands_have_none() {
        assert_eq!(snow_depth(Meters(20.0), Meters(4200.0)), Meters(0.0));
        let low = snow_depth(Meters(100.0), Meters(0.0)).0;
        let high = snow_depth(Meters(2000.0), Meters(0.0)).0;
        assert!((0.10..0.13).contains(&low));
        assert!(high > low && high <= 0.70);
    }

    #[test]
    fn banks_preserve_the_road_and_leave_carriageway_and_joining_streets_open() {
        let mut road = road();
        let retained = road.clone();
        let banks = snowbanks(&road, &[], Meters(300.0));
        assert!(!banks.is_empty());
        assert_eq!(road, retained);
        for bank in banks {
            assert!(bank.center[1] > 700.0);
            for vertex in bank.mesh.vertices {
                assert!((vertex[0] + bank.center[0]).abs() > 3.0);
            }
        }
        for ring in 0..3 {
            road.uvs[ring * RING + 1][0] = 0.0;
        }
        for bank in snowbanks(&road, &[], Meters(300.0)) {
            for vertex in bank.mesh.vertices {
                assert!(vertex[0] + bank.center[0] > 3.0);
            }
        }
        assert!(snowbanks(&road, &[], Meters(4200.0)).is_empty());
    }

    #[test]
    fn structures_and_disconnected_runs_never_get_banks() {
        for surface in [Surface::Bridge, Surface::Tunnel] {
            let point = RoutePoint {
                lat: 47.0,
                lon: 7.0,
                elevation: Meters(700.0),
                distance: Meters(0.0),
                surface,
            };
            assert!(snowbanks(&road(), &[point], Meters(300.0)).is_empty());
            assert!(
                road_snow_exposure(&road(), &[point])
                    .iter()
                    .all(|uv| (uv[0] > 0.5) == (surface != Surface::Tunnel))
            );
        }
        let mut road = road();
        road.indices.clear();
        assert!(snowbanks(&road, &[], Meters(300.0)).is_empty());
    }
}
