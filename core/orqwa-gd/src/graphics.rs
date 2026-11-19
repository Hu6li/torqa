//! Frame samples and the offline calibration scene across the rendering boundary (R151).

use godot::classes::{IRefCounted, RefCounted};
use godot::prelude::*;
use orqwa_app::graphics::Benchmark;

use crate::mesh_arrays;

/// One graphics benchmark, independent of the rider's loaded course and settings.
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct OrqwaGraphicsBenchmark {
    run: Benchmark,
    scene: orqwa_world::World,
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for OrqwaGraphicsBenchmark {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            run: Benchmark::default(),
            scene: orqwa_world::benchmark(),
            base,
        }
    }
}

#[godot_api]
impl OrqwaGraphicsBenchmark {
    /// Preset to render now; empty when the run is finished.
    #[func]
    fn quality(&self) -> GString {
        self.run
            .quality()
            .map_or_else(GString::new, |q| q.name().into())
    }

    /// Wall time between rendered frames, in seconds; true when the preset changes.
    #[func]
    fn frame(&mut self, seconds: f64) -> bool {
        let changed = self.run.frame(seconds);
        if changed && let Some(sample) = self.run.measurements().last() {
            godot_print!(
                "Graphics benchmark {}: {:.1} fps, p95 {:.2} ms",
                sample.quality.name(),
                sample.fps,
                sample.p95.as_secs_f64() * 1000.0,
            );
        }
        changed
    }

    /// Complete recommendation, empty while unfinished.
    #[func]
    fn result(&self) -> VarDictionary {
        self.run
            .recommendation()
            .map_or_else(VarDictionary::new, |sample| {
                vdict! {
                    "quality" => sample.quality.name(),
                    "fps" => sample.fps,
                    "meets_target" => Benchmark::meets_target(sample),
                }
            })
    }

    /// Road surface using the same mesh-array contract as a course.
    #[func]
    fn road(&self) -> VarDictionary {
        mesh_arrays(&self.scene.road)
    }

    /// The fixed scene, with trees selected before upload as on a ride.
    #[func]
    fn chunk(&self, density: f64) -> VarDictionary {
        chunk_arrays(&self.scene.chunks[0], density)
    }

    /// Calibrates Ultra with the same close building details as a ride.
    #[func]
    fn chunk_with_building_detail(&self, density: f64, detailed: bool) -> VarDictionary {
        chunk_arrays_for_graphics(
            &self.scene.chunks[0],
            density,
            orqwa_domain::season::Season::Summer,
            detailed,
        )
    }
}

/// The same conversion for course and calibration chunks, including quality's tree subset.
pub(super) fn chunk_arrays(chunk: &orqwa_world::TerrainChunk, tree_density: f64) -> VarDictionary {
    chunk_arrays_for_season(chunk, tree_density, orqwa_domain::season::Season::Summer)
}

/// Uploads a seasonal plant population without changing the stored course.
pub(super) fn chunk_arrays_for_season(
    chunk: &orqwa_world::TerrainChunk,
    tree_density: f64,
    season: orqwa_domain::season::Season,
) -> VarDictionary {
    chunk_arrays_for_graphics(chunk, tree_density, season, false)
}

/// Derives presentation-only building additions only for the preset that draws them.
pub(super) fn chunk_arrays_for_graphics(
    chunk: &orqwa_world::TerrainChunk,
    tree_density: f64,
    season: orqwa_domain::season::Season,
    detailed: bool,
) -> VarDictionary {
    let [x, y, z] = chunk.center;
    let (grass, flowers) = chunk.trees.small_plants(season.flower_share());
    let mut plants = VarDictionary::new();
    for (name, buffer) in chunk.trees.models_for_density(tree_density) {
        if !buffer.is_empty() {
            plants.set(name, &PackedFloat32Array::from(buffer.as_ref()));
        }
    }
    let mut props = VarDictionary::new();
    for (name, buffer) in &chunk.props.models {
        props.set(name.as_str(), &PackedFloat32Array::from(buffer.as_slice()));
    }
    let labels: VarArray = chunk
        .props
        .labels
        .iter()
        .map(|label| {
            vdict! {
                "text" => &GString::from(label.text.as_str()),
                "transform" => &PackedFloat32Array::from(label.transform.as_slice()),
            }
            .to_variant()
        })
        .collect();
    let modelled: VarArray = chunk
        .modelled
        .iter()
        .map(|cell| {
            let mut models = VarDictionary::new();
            for (name, buffer) in &cell.models {
                models.set(name.as_str(), &PackedFloat32Array::from(buffer.as_slice()));
            }
            vdict! {
                "models" => &models,
                "shells" => &mesh_arrays(&cell.shells),
                "walls" => &mesh_arrays(&cell.walls),
            }
            .to_variant()
        })
        .collect();
    let mut result = vdict! {
        "center" => Vector3::new(x, y, z),
        "terrain" => &mesh_arrays(&chunk.mesh),
        "buildings" => &mesh_arrays(&chunk.buildings),
        "modelled" => &modelled,
        "streets" => &mesh_arrays(&chunk.streets),
        "tracks" => &mesh_arrays(&chunk.tracks),
        "urban" => &mesh_arrays(&chunk.urban),
        "water" => &mesh_arrays(&chunk.water),
        "plants" => &plants,
        "props" => &props,
        "labels" => &labels,
        "wires" => &mesh_arrays(&chunk.props.wires),
        "grass" => &PackedFloat32Array::from(grass.as_ref()),
        "flowers" => &PackedFloat32Array::from(flowers.as_ref()),
    };
    if detailed {
        let details: VarArray = orqwa_world::building_details(&chunk.buildings)
            .iter()
            .map(|mesh| mesh_arrays(mesh).to_variant())
            .collect();
        result.set("building_details", &details);
    }
    result
}
