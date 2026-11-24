//! A world file for the scene (ADR 0042): its description as GDScript reads it, in the scene's
//! axes (x east, y up, z south), and its parts as the scene takes them.

use godot::prelude::*;
use orqwa_app::authored::{Description, Lod, Part, Sky, WorldInfo};

fn colour([r, g, b]: [f64; 3]) -> Color {
    #[allow(clippy::cast_possible_truncation)] // colours in 0–1
    Color::from_rgb(r as f32, g as f32, b as f32)
}

#[allow(clippy::cast_possible_truncation)] // the scene's own precision
fn place([east, north, elevation]: [f64; 3]) -> Vector3 {
    Vector3::new(east as f32, elevation as f32, -north as f32)
}

fn lods(lods: &[Lod]) -> VarArray {
    lods.iter()
        .map(|lod| {
            vdict! {
                "file" => lod.file.as_str(),
                // The last level reaches as far as the view.
                "until_m" => lod.until_m.unwrap_or(f64::INFINITY),
            }
            .to_variant()
        })
        .collect()
}

/// The description of the world being ridden: `{name, environment: {sun: {elevation_deg,
/// azimuth_deg, colour, energy}, ambient: {colour, energy}, sky: {kind: "atmosphere", clouds,
/// overcast} or {kind: "panorama", image, energy}, fog: {colour, density} or empty},
/// sections: [{center, radius, lods: [{file, until_m}], instances}], assets: [{name, lods}],
/// horizon}`, `center` in the scene's axes, `instances` and `horizon` empty without.
pub fn description(world: &Description) -> VarDictionary {
    let environment = &world.environment;
    let sky = match &environment.sky {
        Sky::Atmosphere { clouds, overcast } => vdict! {
            "kind" => "atmosphere",
            "clouds" => *clouds,
            "overcast" => *overcast,
        },
        Sky::Panorama { image, energy } => vdict! {
            "kind" => "panorama",
            "image" => image.as_str(),
            "energy" => *energy,
        },
    };
    let fog = environment.fog.map_or_else(VarDictionary::new, |fog| {
        vdict! { "colour" => colour(fog.colour), "density" => fog.density }
    });
    let sections: VarArray = world
        .sections
        .iter()
        .map(|section| {
            vdict! {
                "center" => place(section.center),
                "radius" => section.radius,
                "lods" => &lods(&section.lods),
                "instances" => section.instances.as_deref().unwrap_or(""),
            }
            .to_variant()
        })
        .collect();
    let assets: VarArray = world
        .assets
        .iter()
        .map(|asset| {
            vdict! { "name" => asset.name.as_str(), "lods" => &lods(&asset.lods) }.to_variant()
        })
        .collect();
    vdict! {
        "name" => world.name.as_str(),
        "environment" => &vdict! {
            "sun" => &vdict! {
                "elevation_deg" => environment.sun.elevation_deg,
                "azimuth_deg" => environment.sun.azimuth_deg,
                "colour" => colour(environment.sun.colour),
                "energy" => environment.sun.energy,
            },
            "ambient" => &vdict! {
                "colour" => colour(environment.ambient.colour),
                "energy" => environment.ambient.energy,
            },
            "sky" => &sky,
            "fog" => &fog,
        },
        "sections" => &sections,
        "assets" => &assets,
        "horizon" => world.horizon.as_deref().unwrap_or(""),
    }
}

/// What the library tells of a world's route: `{world, version, publisher, licence, routes}`.
pub fn info(world: &WorldInfo) -> VarDictionary {
    let routes: PackedStringArray = world.routes.iter().map(GString::from).collect();
    vdict! {
        "world" => world.world.as_str(),
        "version" => world.version.as_deref().unwrap_or(""),
        "publisher" => world.publisher.as_deref().unwrap_or(""),
        "licence" => world.licence.as_str(),
        "routes" => &routes,
    }
}

/// A part read for the scene: a `.glb`'s or image's bytes as `PackedByteArray`, an `instances`
/// file as `[{asset, tinted, buffer}]` (`buffer` a `MultiMesh` buffer, with colours if
/// `tinted`), or the reason it could not be read as a `String`.
pub fn part(part: Result<Part, String>) -> Variant {
    match part {
        Ok(Part::Bytes(bytes)) => PackedByteArray::from(bytes.as_slice()).to_variant(),
        Ok(Part::Instances(groups)) => groups
            .iter()
            .map(|group| {
                vdict! {
                    "asset" => i64::try_from(group.asset).unwrap_or(-1),
                    "tinted" => group.tinted,
                    "buffer" => &PackedFloat32Array::from(group.floats.as_slice()),
                }
                .to_variant()
            })
            .collect::<VarArray>()
            .to_variant(),
        Err(reason) => GString::from(reason.as_str()).to_variant(),
    }
}
