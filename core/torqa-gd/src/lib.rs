//! Godot GDExtension bindings exposing the Torqa core to the presentation layer.
//!
//! This crate only translates between Godot types and [`torqa_app`]; it holds no logic.

// This crate is the FFI boundary: gdext's entry point is an `unsafe impl`, and the
// `#[gdextension]` macro drops item-level attributes, so the allow must be crate-wide.
#![allow(unsafe_code)]

use std::path::PathBuf;
use std::time::Duration;

use godot::classes::{Engine, INode, Node};
use godot::prelude::*;
use torqa_app::view;
use torqa_app::{App, AppEvent, TrainerChoice, paths};
use torqa_devices::ble::DeviceKind;
use torqa_devices::fake::FakeRider;
use torqa_domain::units::{Kilograms, Percent, Rpm, Watts};
use torqa_physics::{DescentMode, RiderSetup};
use torqa_routes::{ElevationSource, LocalProjection};
use torqa_session::RideConfig;

struct TorqaExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TorqaExtension {}

/// Static information about the Torqa core.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TorqaCore;

#[godot_api]
impl TorqaCore {
    /// Version of the Torqa core.
    #[func]
    fn version() -> GString {
        GString::from(torqa_domain::version())
    }
}

/// The Torqa application as a node: add it to the scene tree, call its commands and listen to
/// its signals. It advances rides in `_process`.
#[derive(GodotClass)]
#[class(base = Node)]
pub struct TorqaApp {
    base: Base<Node>,
    app: Option<App>,
}

#[godot_api]
impl INode for TorqaApp {
    fn init(base: Base<Node>) -> Self {
        // The editor instantiates nodes too; it must not start runtimes or touch Bluetooth.
        let app = if Engine::singleton().is_editor_hint() {
            None
        } else {
            App::new(paths::data_dir(), paths::cache_dir())
                .inspect_err(|error| {
                    godot_error!("Torqa: {error}");
                })
                .ok()
        };
        Self { base, app }
    }

    fn process(&mut self, delta: f64) {
        let Some(app) = self.app.as_mut() else {
            return;
        };
        let events = app.update(Duration::from_secs_f64(delta.max(0.0)));
        for event in events {
            self.emit(event);
        }
    }

    fn exit_tree(&mut self) {
        if let Some(app) = self.app.as_mut() {
            app.shutdown();
        }
    }
}

#[godot_api]
impl TorqaApp {
    /// A scan finished: an array of dictionaries `{index, name, kind ("trainer" or
    /// "heart_rate"), rssi}`.
    #[signal]
    fn devices_found(devices: VarArray);

    /// Preparing a course advanced: what is being done, the unit counted, done and total.
    #[signal]
    fn loading_progress(step: GString, unit: GString, done: i64, total: i64);

    /// A route was imported: `{name, length_m, elevation_gain_m, max_grade, elevation_source}`.
    #[signal]
    fn route_loaded(route: VarDictionary);

    /// The 3D world for the loaded route is ready: `{chunks, fallback_samples}`.
    #[signal]
    fn world_ready(world: VarDictionary);

    /// A device connected (also after reconnecting).
    #[signal]
    fn device_connected(name: GString);

    /// A device lost its connection and is reconnecting.
    #[signal]
    fn device_disconnected(name: GString);

    /// The rider reached the finish.
    #[signal]
    fn ride_finished();

    /// The ride was saved as a FIT file.
    #[signal]
    fn ride_saved(path: GString);

    /// A course was saved or imported into the library.
    #[signal]
    fn course_added(path: GString);

    /// Something went wrong.
    #[signal]
    fn failed(message: GString);

    /// Scans for trainers and heart-rate sensors.
    #[func]
    fn scan(&mut self, seconds: f64) {
        if let Some(app) = self.app.as_mut() {
            app.scan(Duration::from_secs_f64(seconds.max(1.0)));
        }
    }

    /// Imports a GPX file; `offline` uses cached terrain only.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn load_route(&mut self, path: GString, offline: bool) {
        if let Some(app) = self.app.as_mut() {
            app.load_route(PathBuf::from(path.to_string()), offline);
        }
    }

    /// Opens a course file from the library or elsewhere; emits `route_loaded` like
    /// `load_route`.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn open_course(&mut self, path: GString) {
        if let Some(app) = self.app.as_mut() {
            app.open_course(PathBuf::from(path.to_string()));
        }
    }

    /// Saves the loaded route as a course in the library (emits `course_added` or `failed`).
    /// False until the world is ready.
    #[func]
    fn save_course(&mut self) -> bool {
        self.command(App::save_course)
    }

    /// Copies a course file into the library (emits `course_added` or `failed`).
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn import_course(&mut self, path: GString) {
        if let Some(app) = self.app.as_mut() {
            app.import_course(PathBuf::from(path.to_string()));
        }
    }

    /// The courses in the library: `[{path, name, length_m, elevation_gain_m, max_grade}]`.
    #[func]
    fn courses(&self) -> VarArray {
        let mut array = VarArray::new();
        for course in self.app.as_ref().map(App::courses).unwrap_or_default() {
            let path = course.path.display().to_string();
            array.push(
                &vdict! {
                    "path" => path.as_str(),
                    "name" => course.manifest.name.as_str(),
                    "length_m" => course.manifest.length_m,
                    "elevation_gain_m" => course.manifest.elevation_gain_m,
                    "max_grade" => course.manifest.max_grade_percent,
                }
                .to_variant(),
            );
        }
        array
    }

    /// Connects the simulated trainer.
    #[func]
    fn connect_fake_trainer(&mut self, power: f64, cadence: f64) -> bool {
        let choice = TrainerChoice::Fake(FakeRider {
            power: Watts(power),
            cadence: Rpm(cadence),
        });
        self.command(|app| app.connect_trainer(choice))
    }

    /// Connects a scanned trainer by its index.
    #[func]
    fn connect_trainer(&mut self, index: i64) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        self.command(|app| app.connect_trainer(TrainerChoice::Discovered(index)))
    }

    /// Connects a scanned heart-rate sensor by its index.
    #[func]
    fn connect_heart_rate(&mut self, index: i64) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        self.command(|app| app.connect_heart_rate(index))
    }

    /// Starts riding the loaded route.
    #[func]
    fn start_ride(&mut self, difficulty: f64, flat_descents: bool, mass_kg: f64) -> bool {
        let config = RideConfig {
            setup: RiderSetup {
                mass: Kilograms(mass_kg),
                ..RiderSetup::default()
            },
            difficulty: Percent(difficulty),
            descent: if flat_descents {
                DescentMode::Flat
            } else {
                DescentMode::Coast
            },
        };
        self.command(|app| app.start_ride(config))
    }

    /// Ends the ride and saves it (emits `ride_saved` or `failed`).
    #[func]
    fn finish_ride(&mut self) {
        let Some(app) = self.app.as_mut() else {
            return;
        };
        for event in app.finish_ride() {
            self.emit(event);
        }
    }

    /// The ride state: `{elapsed_s, distance_m, remaining_m, speed_kmh, grade, elevation_m, x, y,
    /// heading, power, cadence, heart_rate}`; sensor values are `null` when unknown. Empty when
    /// not riding. `x`/`y` are metres east/north of the route start, as in `track()`; `heading`
    /// is the direction of travel in radians clockwise from north.
    #[func]
    fn ride_state(&self) -> VarDictionary {
        let Some(app) = self.app.as_ref() else {
            return VarDictionary::new();
        };
        let (Some(state), Some(route)) = (app.ride_state(), app.route()) else {
            return VarDictionary::new();
        };
        let (x, y) =
            LocalProjection::for_route(route).project(state.position.lat, state.position.lon);
        let optional = |value: Option<f64>| value.map_or_else(Variant::nil, |v| v.to_variant());
        let t = state.telemetry;
        vdict! {
            "elapsed_s" => state.elapsed.as_secs_f64(),
            "distance_m" => state.distance.0,
            "remaining_m" => state.remaining.0,
            "speed_kmh" => state.speed.as_kilometers_per_hour(),
            "grade" => state.position.grade.0,
            "elevation_m" => state.position.elevation.0,
            "x" => x,
            "y" => y,
            "heading" => state.position.heading,
            "power" => &optional(t.power.map(|p| p.0)),
            "cadence" => &optional(t.cadence.map(|c| c.0)),
            "heart_rate" => &optional(t.heart_rate.map(|h| h.0)),
        }
    }

    /// Number of terrain chunks in the generated world (0 until `world_ready`).
    #[func]
    fn world_chunk_count(&self) -> i64 {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or(0, |world| i64::try_from(world.chunks.len()).unwrap_or(0))
    }

    /// World chunk `index`: `{center, terrain, buildings, conifers, broadleaves}`. `terrain`
    /// and `buildings` are mesh arrays (`{vertices, normals, uvs, colors, indices}`), the tree
    /// entries `MultiMesh` transform buffers. Geometry is relative to `center`, in Godot
    /// coordinates (x east, y up, −z north, metres from the route start).
    #[func]
    fn world_chunk(&self, index: i64) -> VarDictionary {
        let chunk = self
            .app
            .as_ref()
            .and_then(App::world)
            .zip(usize::try_from(index).ok())
            .and_then(|(world, index)| world.chunks.get(index));
        let Some(chunk) = chunk else {
            return VarDictionary::new();
        };
        let [x, y, z] = chunk.center;
        let conifers = PackedFloat32Array::from(chunk.trees.conifers.as_slice());
        let broadleaves = PackedFloat32Array::from(chunk.trees.broadleaves.as_slice());
        vdict! {
            "center" => Vector3::new(x, y, z),
            "terrain" => &mesh_arrays(&chunk.mesh),
            "buildings" => &mesh_arrays(&chunk.buildings),
            "conifers" => &conifers,
            "broadleaves" => &broadleaves,
        }
    }

    /// Bridges and tunnels as mesh arrays (vertex-coloured), in route coordinates.
    #[func]
    fn structures_mesh(&self) -> VarDictionary {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or_else(VarDictionary::new, |world| mesh_arrays(&world.structures))
    }

    /// The minimap as coloured triangles: `{vertices, colors, background}`, vertices in metres
    /// east/north of the route start (as in `track()`).
    #[func]
    fn minimap_mesh(&self) -> VarDictionary {
        let Some(world) = self.app.as_ref().and_then(App::world) else {
            return VarDictionary::new();
        };
        let vertices: PackedVector2Array = world
            .minimap
            .vertices
            .iter()
            .map(|&[x, y]| Vector2::new(x, y))
            .collect();
        let colors: PackedColorArray = world
            .minimap
            .colors
            .iter()
            .map(|&[r, g, b, a]| Color::from_rgba(r, g, b, a))
            .collect();
        let [r, g, b, a] = torqa_world::MINIMAP_BACKGROUND;
        vdict! {
            "vertices" => &vertices,
            "colors" => &colors,
            "background" => Color::from_rgba(r, g, b, a),
        }
    }

    /// Rivers and streams as mesh arrays, in route coordinates.
    #[func]
    fn water_mesh(&self) -> VarDictionary {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or_else(VarDictionary::new, |world| mesh_arrays(&world.water))
    }

    /// The road as mesh arrays `{vertices, normals, uvs, indices}`; `uv.y` is the distance
    /// along the route in metres.
    #[func]
    fn road_mesh(&self) -> VarDictionary {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or_else(VarDictionary::new, |world| mesh_arrays(&world.road))
    }

    /// `(distance m, elevation m)` points of the loaded route, at most `max_points`.
    #[func]
    fn elevation_profile(&self, max_points: i64) -> PackedVector2Array {
        self.points(max_points, view::elevation_profile)
    }

    /// The loaded route in metres east/north of its start, at most `max_points`.
    #[func]
    fn track(&self, max_points: i64) -> PackedVector2Array {
        self.points(max_points, view::track)
    }
}

impl TorqaApp {
    fn command(&mut self, run: impl FnOnce(&mut App) -> Result<(), torqa_app::AppError>) -> bool {
        let Some(app) = self.app.as_mut() else {
            return false;
        };
        match run(app) {
            Ok(()) => true,
            Err(error) => {
                let message = error.to_string();
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
                false
            }
        }
    }

    fn points(
        &self,
        max_points: i64,
        pick: fn(&torqa_routes::Route, usize) -> Vec<(f64, f64)>,
    ) -> PackedVector2Array {
        let Some(route) = self.app.as_ref().and_then(App::route) else {
            return PackedVector2Array::new();
        };
        let max_points = usize::try_from(max_points).unwrap_or(2);
        pick(route, max_points)
            .into_iter()
            .map(|(a, b)| vector2(a, b))
            .collect()
    }

    fn emit(&mut self, event: AppEvent) {
        match event {
            AppEvent::DevicesFound(devices) => {
                let mut array = VarArray::new();
                for device in devices {
                    let kind = match device.kind {
                        DeviceKind::Trainer => "trainer",
                        DeviceKind::HeartRateSensor => "heart_rate",
                    };
                    let rssi = device
                        .rssi
                        .map_or_else(Variant::nil, |rssi| i64::from(rssi).to_variant());
                    let index = i64::try_from(device.index).unwrap_or(-1);
                    array.push(
                        &vdict! {
                            "index" => index,
                            "name" => device.name.as_str(),
                            "kind" => kind,
                            "rssi" => &rssi,
                        }
                        .to_variant(),
                    );
                }
                self.signals().devices_found().emit(&array);
            }
            AppEvent::RouteLoaded(route) => {
                let source = match route.elevation_source {
                    ElevationSource::Terrain => "terrain",
                    ElevationSource::File => "file",
                };
                let info = vdict! {
                    "name" => route.name.as_str(),
                    "length_m" => route.length,
                    "elevation_gain_m" => route.elevation_gain,
                    "max_grade" => route.max_grade,
                    "elevation_source" => source,
                };
                self.signals().route_loaded().emit(&info);
            }
            AppEvent::LoadProgress { stage, done, total } => {
                self.signals().loading_progress().emit(
                    &GString::from(stage.label()),
                    &GString::from(stage.unit()),
                    i64::try_from(done).unwrap_or(i64::MAX),
                    i64::try_from(total).unwrap_or(i64::MAX),
                );
            }
            AppEvent::WorldReady {
                chunks,
                fallback_samples,
            } => {
                let info = vdict! {
                    "chunks" => i64::try_from(chunks).unwrap_or(0),
                    "fallback_samples" => i64::try_from(fallback_samples).unwrap_or(0),
                };
                self.signals().world_ready().emit(&info);
            }
            AppEvent::Connected(name) => {
                self.signals()
                    .device_connected()
                    .emit(&GString::from(name.as_str()));
            }
            AppEvent::Disconnected(name) => {
                self.signals()
                    .device_disconnected()
                    .emit(&GString::from(name.as_str()));
            }
            AppEvent::RideFinished => self.signals().ride_finished().emit(),
            AppEvent::RideSaved(path) => {
                let path = path.display().to_string();
                self.signals()
                    .ride_saved()
                    .emit(&GString::from(path.as_str()));
            }
            AppEvent::CourseAdded(path) => {
                let path = path.display().to_string();
                self.signals()
                    .course_added()
                    .emit(&GString::from(path.as_str()));
            }
            AppEvent::Error(message) => {
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
            }
        }
    }
}

/// Converts mesh data to the arrays Godot's `ArrayMesh` takes; `colors` is empty when the
/// mesh has none.
fn mesh_arrays(mesh: &torqa_world::MeshData) -> VarDictionary {
    let vertices: PackedVector3Array = mesh
        .vertices
        .iter()
        .map(|&[x, y, z]| Vector3::new(x, y, z))
        .collect();
    let normals: PackedVector3Array = mesh
        .normals
        .iter()
        .map(|&[x, y, z]| Vector3::new(x, y, z))
        .collect();
    let uvs: PackedVector2Array = mesh.uvs.iter().map(|&[u, v]| Vector2::new(u, v)).collect();
    let colors: PackedColorArray = mesh
        .colors
        .iter()
        .map(|&[r, g, b, a]| Color::from_rgba(r, g, b, a))
        .collect();
    let indices: PackedInt32Array = mesh
        .indices
        .iter()
        .map(|&i| i32::try_from(i).unwrap_or(0))
        .collect();
    vdict! {
        "vertices" => &vertices,
        "normals" => &normals,
        "uvs" => &uvs,
        "colors" => &colors,
        "indices" => &indices,
    }
}

// Godot vectors are f32; metre precision is ample for drawing.
#[allow(clippy::cast_possible_truncation)]
fn vector2(x: f64, y: f64) -> Vector2 {
    Vector2::new(x as f32, y as f32)
}
