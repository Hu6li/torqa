//! 3D world geometry for ORQWA (R16): terrain chunks in a corridor around the route, coloured
//! by land cover, with buildings and trees from OpenStreetMap, rivers, and the road.
//!
//! Coordinates follow Godot: metres with x east, y up and z south (−z is north), relative to
//! the route start for the road and water, and to each chunk's centre for chunk geometry.
//! Triangles wind clockwise seen from their front, Godot's front-face order.

mod appearance;
mod benchmark;
mod buildings;
mod cells;
mod chains;
mod channels;
mod climate;
pub mod course_map;
mod drape;
mod earthworks;
mod forest_mask;
mod foundations;
mod horizon;
mod junctions;
mod landcover;
mod minimap;
mod palette;
mod pavement;
mod props;
mod railways;
mod riding;
mod road;
mod roundabouts;
mod route_distances;
mod seasonal_snow;
mod snow_ground;
mod stages;
pub mod stored;
mod streaming;
mod streets;
mod structures;
mod urban;
mod vegetation;
mod water;
mod water_levels;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use orqwa_osm::{LandCover, MapData, RoadClass};
use orqwa_routes::{ElevationModel, LocalProjection, Route, Surface};
use tracing::{info, warn};

pub use benchmark::benchmark;
pub use climate::{SeasonClimate, season_climate};
pub use forest_mask::{ForestMask, forest_mask};
pub use horizon::{HORIZON, Horizon, horizon};
use landcover::LandIndex;
pub use minimap::{FlatMap, MapFeature, minimap};
pub use props::{Label, Props};
pub use riding::RoadWidths;
use road::RoadIndex;
pub use route_distances::{DENSE_FOREST, DISTANCE_STEP, RouteDistances, route_distances};
pub use seasonal_snow::{Snowbank, road_snow_exposure, snow_depth, snowbanks};
pub use snow_ground::{SnowGroundMask, snow_ground_mask};
pub use streaming::{ChunkPlan, REPLAN_DISTANCE, chunk_plan};
pub use vegetation::Trees;

/// What worlds look like. Raise it whenever a change makes the same course come out
/// differently: a world kept with an older one is still ridden, and its course offers to build
/// it again (R101). A change that also makes kept worlds unreadable raises
/// [`stored::FORMAT`] instead or as well.
pub const GENERATOR: u32 = 40;

/// Edge length of a terrain chunk.
pub(crate) const CHUNK_SIZE: f64 = 480.0;
/// Distance between terrain vertices.
const GRID: f64 = 16.0;
/// Terrain (and map data) is used up to this far from the route.
pub const CORRIDOR: f64 = 1500.0;
/// Half the road width.
const ROAD_HALF_WIDTH: f64 = 3.0;
/// Terrain cells near the road are split into this many pieces a side, so the ground can follow
/// the road's verge, cuttings and embankments closely.
const SUB: usize = 6;
/// Walls and narrow shelves on steep hillsides need finer pieces than the usual ones...
const STEEP_SUB: usize = 12;
/// ...close, independently graded roads room for a bank between their carriageways.
const CLOSE_SUB: usize = 24;
/// Sub-metre gaps at joining streets need another step of local refinement.
const TIGHT_SUB: usize = 48;
/// Terrain within this distance of a close bed beside the ridden road or a railway is split
/// as finely as that bed's bank needs (`Shapers::detail_count`).
const CLOSE_REACH: f64 = 8.0;
/// Beyond the ridden carriageway and the diagonal of the local terrain pieces, this margin
/// keeps every triangle touching the road below it before surrounding ways may shape the
/// ground (R141)...
const HARD_MARGIN: f64 = 0.2;
/// ...and their ground takes over across this much more.
const YIELD_RAMP: f64 = 0.5;
/// Behind a surrounding way, the ridden road's earthworks give way over this distance.
const SHADOW_RAMP: f64 = 4.0;
/// A way passing this far or more over a tunnel's cover keeps its own bed above it.
const TUNNEL_KEEP_DEPTH: f64 = 6.0;
/// The ground a portal keeps (`structures::Portal::embeds`) lies within this distance of its
/// tunnel's line, which the pieces found within `LEVEL_REACH` always include.
const PORTAL_REACH: f64 = 24.0;
/// The size of those pieces.
#[allow(clippy::cast_precision_loss)] // a small constant
const FINE: f64 = GRID / SUB as f64;
/// Ground up to this distance from the road centre is level just below the road: the road and
/// its verge. Wide enough that no triangle of the fine ground touching the road can rise above
/// it (half the road plus the diagonal of a fine piece, and a little).
pub(crate) const VERGE: f64 = ROAD_HALF_WIDTH + FINE * std::f64::consts::SQRT_2 + 0.2;
/// Beyond the verge the ground climbs to the hillside at most this steeply (a cutting)...
const CUT_SLOPE: f64 = 1.2;
/// ...or falls to the valley at most this steeply (an embankment).
const FILL_SLOPE: f64 = 0.6;
/// Cuttings and embankments reach at most this far from the road; over the last
/// `REACH_FADE` metres the shaped ground blends into the natural.
pub(crate) const LEVEL_REACH: f64 = 45.0;
const REACH_FADE: f64 = 12.0;
/// The hill heaped over a tunnel of the road ridden where the terrain has none (#135) lies this
/// far over the tube's arch...
const TUNNEL_HILL_COVER: f64 = 1.0;
/// ...and reaches this far beyond the tube either side, over the ring round its opening, before
/// banking down at `FILL_SLOPE`.
const TUNNEL_HILL_SHOULDER: f64 = 1.0;
/// Level ground sits this far below the road surface, so the two never flicker; the road's edge
/// bevels down to it (road.rs). Other streets lie between the two where they join the road.
pub(crate) const ROAD_SINK: f64 = 0.15;
/// A cell levelled across a street is split into fine pieces only where they would depart from
/// its plain triangles by more than this.
const PLAIN_TOLERANCE: f64 = 0.02;
/// Without terrain data, heights follow the road found within this distance.
const FALLBACK_RADIUS: f64 = 400.0;

/// Triangle geometry ready for a GPU mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshData {
    /// Vertex positions.
    pub vertices: Vec<[f32; 3]>,
    /// Unit vertex normals.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates; for the road `u` is 0–1 across and `v` the distance in metres.
    pub uvs: Vec<[f32; 2]>,
    /// Vertex colours or material metadata; empty when the mesh has none. Roads encode
    /// `[material / 8, width_metres / 32, marking_style / 4, (lanes + track_flag) / 8]`,
    /// with track flag 4 for a grassy centre (ADR 0031). Other meshes retain their own
    /// colour contracts, including alpha 1 marking water.
    pub colors: Vec<[f32; 4]>,
    /// Three vertex indices per triangle.
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Gives back the room the arrays grew into while the mesh was built. Built by pushing, a
    /// chunk's meshes held about two fifths more than they used: over 1 GB on a long course.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.vertices.shrink_to_fit();
        self.normals.shrink_to_fit();
        self.uvs.shrink_to_fit();
        self.colors.shrink_to_fit();
        self.indices.shrink_to_fit();
    }

    /// Adds another mesh's triangles. Missing colours default to white when either mesh
    /// carries colours, keeping the optional vertex array complete.
    ///
    /// # Panics
    /// If the combined mesh has more than `u32::MAX` vertices.
    pub fn append(&mut self, mut other: MeshData) {
        let offset = u32::try_from(self.vertices.len()).expect("mesh fits u32");
        if !self.colors.is_empty() || !other.colors.is_empty() {
            self.colors.resize(self.vertices.len(), [1.0; 4]);
            other.colors.resize(other.vertices.len(), [1.0; 4]);
        }
        self.vertices.extend(other.vertices);
        self.normals.extend(other.normals);
        self.uvs.extend(other.uvs);
        self.colors.extend(other.colors);
        self.indices
            .extend(other.indices.into_iter().map(|i| i + offset));
    }
}

/// The ways and water of the map around the route, ready for the chunks.
struct Ways {
    streets: streets::Shared,
    /// The railways, on their own lines (#85).
    network: railways::Network,
    streams: Vec<water::Stream>,
    pools: Vec<water::Pool>,
    /// The rounded corners where streets meet.
    corners: Vec<junctions::Corner>,
    /// Where plants keep off.
    clearance: streets::Clearance,
    /// Roundabouts' islands.
    islands: Vec<roundabouts::Island>,
    /// Where water cuts the ground.
    channels: channels::Channels,
    water_levels: std::sync::Arc<water_levels::Levels>,
    /// Shared road beds for surrounding roads, vehicle tracks and cycleways.
    levels: streets::Levels,
}

impl Ways {
    async fn new<M: ElevationModel>(
        map: &MapData,
        projection: &LocalProjection,
        road: &RoadIndex,
        model: &mut M,
    ) -> Self {
        let mut streets = streets::lines(map, projection, model).await;
        streets::connect(&mut streets, road);
        let streets: streets::Shared = streets.into();
        let network = railways::network(map, projection, road, model).await;
        let streams = water::streams(&map.waterways, projection, road);
        let pools = water::pools(&map.areas, projection, road);
        let water_levels =
            water_levels::Levels::new(&streams, &pools, projection, road, model).await;
        let corners = junctions::corners(&streets, road);
        Self {
            clearance: streets::Clearance::new(&streets, &corners, &network.railways, &pools),
            corners,
            islands: roundabouts::islands(map, projection),
            channels: channels::Channels::new(&streams, &pools, &streets),
            water_levels,
            levels: streets::Levels::new(
                &streets,
                &appearance::Index::new(map, projection),
                road,
                &network.index,
            ),
            streets,
            network,
            streams,
            pools,
        }
    }
}

/// A square piece of the world.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainChunk {
    /// Centre of the chunk at sea level; the chunk's geometry is relative to it.
    pub center: [f32; 3],
    /// Ground, coloured by land cover.
    pub mesh: MeshData,
    /// Buildings standing in the chunk that no model fits, built as shells.
    pub buildings: MeshData,
    /// Buildings drawn as Blender-made models up close, by cell.
    pub modelled: Vec<BuildingCell>,
    /// Paved ways around the route (not the road ridden), on this chunk's ground.
    pub streets: MeshData,
    /// Unpaved ways of the map, likewise.
    pub tracks: MeshData,
    /// Sidewalks, curbs, building access and forecourts, with urban material codes in colours.
    pub urban: MeshData,
    /// Lakes, ponds, rivers and streams of the map, likewise; they pass under streets and the
    /// road.
    pub water: MeshData,
    /// Trees standing in the chunk.
    pub trees: Trees,
    /// Guard rails, posts and poles by the road ridden (R89).
    pub props: Props,
}

impl TerrainChunk {
    /// Gives back the room every mesh and buffer of the chunk grew into while built.
    fn shrink_to_fit(&mut self) {
        for mesh in [
            &mut self.mesh,
            &mut self.buildings,
            &mut self.streets,
            &mut self.tracks,
            &mut self.urban,
            &mut self.water,
            &mut self.props.wires,
        ] {
            mesh.shrink_to_fit();
        }
        for cell in &mut self.modelled {
            cell.shells.shrink_to_fit();
            cell.models.values_mut().for_each(Vec::shrink_to_fit);
        }
        let plants = &mut self.trees;
        for buffer in plants
            .models
            .values_mut()
            .chain(self.props.models.values_mut())
            .chain([&mut plants.grass, &mut plants.flowers])
        {
            buffer.shrink_to_fit();
        }
        self.modelled.shrink_to_fit();
    }
}

/// Buildings drawn as Blender-made models (`app/assets/models/buildings`) up close, in one cell
/// of a chunk; each cell switches to the shells by its own distance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BuildingCell {
    /// Instances by model name, as Godot `MultiMesh` buffers of 20 floats each: the transform
    /// relative to the chunk centre (row-major 3×4), the plaster colour (sRGB, alpha 1) as
    /// instance colour, and the roof colour (sRGB) with a variant in 0–1 as custom data.
    pub models: BTreeMap<String, Vec<f32>>,
    /// The same buildings as shells, for the distance.
    pub shells: MeshData,
}

/// The generated world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    /// Terrain chunks around the route.
    pub chunks: Vec<TerrainChunk>,
    /// The road along the route.
    pub road: MeshData,
    /// Bridges and tunnels, of the road and the railways.
    pub structures: MeshData,
    /// The railways of the map near the route, in route coordinates (`u` across the bed of
    /// ballast, below 0 and above 1 on its shoulders, `v` metres along).
    pub railways: MeshData,
    /// The land beyond the corridor, from [`horizon`] (empty until it is made: it needs a coarse
    /// terrain model of its own).
    pub horizon: Horizon,
    /// Terrain samples that had no elevation data and followed the road instead.
    pub fallback_samples: usize,
}

/// Wearing surfaces are independent of terrain chunks. Structures follow the completed
/// terrain so their foundations can reach the ground actually drawn.
fn whole(shapers: &Shapers<'_>, ways: &Ways) -> World {
    let (road, rails) = (shapers.road, shapers.rails);
    World {
        road: road.mesh(ROAD_HALF_WIDTH, &streets::mouths(&ways.streets, road)),
        railways: rails.mesh(railways::BED_M / 2.0, &[]),
        ..World::default()
    }
}

/// Builds the world for `route`, sampling heights from `model` (e.g. the terrain tiles) and
/// placing `map` features. Where the model has no data, the terrain follows the road.
/// `progress` is called with (chunks built, chunks total).
pub async fn generate<M: ElevationModel>(
    route: &Route,
    model: &mut M,
    map: &MapData,
    progress: &mut (dyn FnMut(usize, usize) + Send),
) -> World {
    let chunks = Mutex::new(Vec::new());
    let mut world = generate_into(route, model, map, progress, &|index, chunk| {
        let mut chunks = chunks.lock().unwrap_or_else(PoisonError::into_inner);
        if chunks.len() <= index {
            chunks.resize_with(index + 1, || None);
        }
        chunks[index] = Some(chunk);
    })
    .await;
    // `generate_into` finishes every chunk once.
    world.chunks = chunks
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .into_iter()
        .flatten()
        .collect();
    world
}

/// Builds the world as [`generate`] does, but gives each chunk to `sink` as soon as it is
/// finished, with its index in [`World::chunks`], instead of keeping it: only the chunks
/// around those being built are held (ADR 0040). Chunks are finished in no set order, each
/// once, several at a time on all cores. The world returned has every part but its chunks.
#[allow(clippy::too_many_lines)] // the preparation, then one pass over the chunks
pub async fn generate_into<M: ElevationModel>(
    route: &Route,
    model: &mut M,
    map: &MapData,
    progress: &mut (dyn FnMut(usize, usize) + Send),
    sink: &(dyn Fn(usize, TerrainChunk) + Sync),
) -> World {
    let projection = LocalProjection::for_route(route);
    let mut road = RoadIndex::new(route, &projection);
    road.set_appearances(&appearance::Index::new(map, &projection));
    let reached = cells_near_route(&road);
    let total = reached.len();
    // Announce the step before the slower preparation below.
    progress(0, total);
    let land = LandIndex::new(&map.areas, &projection);
    let climate = climate::Climate::at(route.points()[0].lat);
    let lone_lighthouses = buildings::lone_lighthouses(map, &projection);
    let buildings =
        buildings_by_chunk((map, &lone_lighthouses), &projection, &road, &land, climate);
    let mut ways = Ways::new(map, &projection, &road, model).await;
    let portals = structures::open_portals(
        &mut road,
        &mut ways.network.index,
        &ways.levels,
        &projection,
        model,
    )
    .await;
    let hollows = structures::hollows(&road, &ways.network.index, &portals);
    let shapers = Shapers {
        road: &road,
        rails: &ways.network.index,
        streets: &ways.levels,
        portals: &portals,
        hollows: &hollows,
        hills: true,
    };
    let below = structures::Below::new(&road, &ways.network.index, &ways.streets, &ways.islands);
    let mut world = whole(&shapers, &ways);
    let mut urban = urban::Urban::build(
        buildings.values().flatten(),
        &ways.streets,
        &ways.corners,
        &ways.clearance,
        &road,
        &land,
        pavement::Pavement::new(&world, []),
    );
    // Walking joins need one datum even when their profile anchors lie in another chunk.
    let mut grades = Vec::new();
    for point in urban.grade_points() {
        if let Some(height) = drawn_ground(model, &projection, &shapers, point).await {
            grades.push((point, height));
        }
    }
    urban.pin_grades(grades);
    let mouths = streets::mouths(&ways.streets, &road);
    let places: Vec<(String, (f64, f64))> = map
        .places
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                projection.project(p.position.0, p.position.1),
            )
        })
        .collect();
    let corridor = Corridor {
        shapers: &shapers,
        land: &land,
        ways: &ways,
        below: &below,
        buildings: &buildings,
        urban: &urban,
        climate,
        mouths: &mouths,
        places: &places,
    };
    let pieces = structures::pieces(&road, &ways.network.index);
    let mut stages =
        stages::Stages::new(reached.keys().copied().collect(), urban.pavement(), &pieces);
    // Again once the whole-route preparation is done, so its memory can be told from the
    // chunks'.
    progress(0, total);
    // Along the route, so neighbours are built close together and few wait for theirs.
    let mut order: Vec<((i32, i32), usize)> = reached.into_iter().collect();
    order.sort_by_key(|&(cell, reach)| (reach, cell));
    let (chunks, trees) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let counted = |index, chunk: TerrainChunk| {
        chunks.fetch_add(1, Ordering::Relaxed);
        trees.fetch_add(chunk.trees.len(), Ordering::Relaxed);
        sink(index, chunk);
    };

    // Chunks go in batches: their elevations one after the other, as the model is not shared,
    // then the chunks themselves on all cores, where nearly all the time goes. A batch's chunks
    // are all held before any is finished, so they add to the peak: two per core keep the cores
    // busy enough (Wada: about a tenth slower than four, 0.2 GB less at the peak; one per core
    // peaks no lower, as the chunks kept for their neighbours set the floor then).
    let threads = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let mut built = 0;
    for batch in order.chunks(threads * 2) {
        let mut jobs = Vec::with_capacity(batch.len());
        for &((cx, cn), _) in batch {
            let elevations = HeightGrid::elevations(cx, cn, &projection, model).await;
            jobs.push((cx, cn, elevations));
        }
        let results = in_parallel(&jobs, threads, |(cx, cn, elevations)| {
            corridor.chunk(*cx, *cn, elevations)
        });
        for (&(cell, _), (chunk, fallbacks, barriers)) in batch.iter().zip(results) {
            stages.built(stages.index(cell), chunk, barriers);
            world.fallback_samples += fallbacks;
        }
        built += batch.len();
        progress(built, total);
        stages.finish(threads, &counted);
        stages.build_bridges(&pieces, &below);
        stages.drop_unneeded();
    }
    debug_assert!(stages.done(), "a chunk or bridge was left waiting");
    // Tunnels stand on nothing drawn; all are joined in the pieces' order.
    let mut parts = Vec::with_capacity(pieces.len());
    for (piece, bridge) in pieces.iter().zip(stages.take_bridges()) {
        parts.push(match bridge {
            Some(bridge) => bridge,
            None => (
                piece.tunnel(&road, &shapers, &projection, model).await,
                Vec::new(),
            ),
        });
    }
    world.structures = structures::join(parts);
    if world.fallback_samples > 0 {
        warn!(
            samples = world.fallback_samples,
            "terrain data missing in places; terrain follows the road there"
        );
    }
    let (chunks, trees) = (chunks.into_inner(), trees.into_inner());
    info!(chunks, trees, "world generated");
    world
}

/// Buildings near the route, the map's and `extra` ones (lighthouses standing alone), grouped
/// by the chunk containing their first corner.
fn buildings_by_chunk<'a>(
    (map, extra): (&'a MapData, &'a [orqwa_osm::Building]),
    projection: &LocalProjection,
    road: &RoadIndex,
    land: &LandIndex,
    climate: climate::Climate,
) -> HashMap<(i32, i32), Vec<buildings::Plot<'a>>> {
    let project = |points: &[(f64, f64)]| -> Vec<(f64, f64)> {
        points
            .iter()
            .map(|&(lat, lon)| projection.project(lat, lon))
            .collect()
    };
    let streets = map
        .roads
        .iter()
        .filter(|r| {
            r.structure.is_none()
                && matches!(
                    r.class,
                    RoadClass::Major | RoadClass::Street | RoadClass::Service
                )
        })
        .flat_map(|r| drape::densify(&project(&r.line), 3.0))
        .chain(road.samples(3.0));
    let frontage = buildings::Frontage::new(streets);
    let mut plots = Vec::new();
    for building in map.buildings.iter().chain(extra) {
        let footprint = buildings::footprint(building, projection);
        let Some(&(east, north)) = footprint.first() else {
            continue;
        };
        // Roads stay clear (#100): buildings mapped across them (e.g. bad data) are left out,
        // those reaching into the road ridden by a corner or a wall as drawn (#138), and those
        // a street runs through.
        let mut outline = buildings::drawn_outline(&footprint);
        outline.push(outline[0]);
        let on_road = drape::densify(&outline, 2.0)
            .iter()
            .any(|&(e, n)| road.edge_distance(e, n) < 1.0);
        if on_road
            || frontage.runs_through(&footprint)
            || road.nearest(east, north, CORRIDOR).is_none()
        {
            continue;
        }
        let (e, n) = buildings::centroid(&footprint);
        let setting = if land.has(e, n, LandCover::Industrial) {
            buildings::Setting::Industrial
        } else if land.has(e, n, LandCover::Commercial) {
            buildings::Setting::Commercial
        } else if land.has(e, n, LandCover::Public) {
            buildings::Setting::Public
        } else if land.has(e, n, LandCover::Residential) {
            buildings::Setting::Town
        } else {
            buildings::Setting::Countryside
        };
        plots.push(buildings::Plot {
            building,
            footprint,
            setting,
            church: false,
            shop: None,
            purpose: None,
            landmark: None,
            climate,
            region: buildings::Region::at(projection.unproject(e, n)),
        });
    }
    buildings::mark_churches(&mut plots, &project(&map.churches));
    for (points, landmark) in [
        (&map.castles, buildings::Landmark::Castle),
        (&map.lighthouses, buildings::Landmark::Lighthouse),
    ] {
        buildings::mark_landmarks(&mut plots, &project(points), landmark);
    }
    for (points, purpose) in [
        (&map.offices, buildings::Purpose::Office),
        (&map.hotels, buildings::Purpose::Hotel),
        (&map.public, buildings::Purpose::Public),
    ] {
        buildings::mark_purpose(&mut plots, &project(points), purpose);
    }
    buildings::mark_shops(&mut plots, &project(&map.shops), &frontage);

    let mut by_chunk: HashMap<_, Vec<_>> = HashMap::new();
    for plot in plots {
        let (east, north) = plot.footprint[0];
        by_chunk
            .entry(chunk_of(east, north))
            .or_default()
            .push(plot);
    }
    by_chunk
}

/// What every chunk is built from, shared by the chunks built at the same time.
struct Corridor<'a> {
    shapers: &'a Shapers<'a>,
    land: &'a LandIndex,
    ways: &'a Ways,
    below: &'a structures::Below<'a>,
    buildings: &'a HashMap<(i32, i32), Vec<buildings::Plot<'a>>>,
    urban: &'a urban::Urban,
    climate: climate::Climate,
    /// Where streets join the road ridden.
    mouths: &'a [road::Mouth],
    /// Places' names and where they are, for the signs.
    places: &'a [(String, (f64, f64))],
}

impl Corridor<'_> {
    /// Chunk (`cx`, `cn`) from its [`HeightGrid::elevations`], and how many of its terrain
    /// samples had no elevation data.
    fn chunk(
        &self,
        cx: i32,
        cn: i32,
        elevations: &[Option<f64>],
    ) -> (TerrainChunk, usize, ([Vec<pavement::Barrier>; 2], usize)) {
        let Ways {
            streets,
            streams,
            pools,
            ..
        } = self.ways;
        let (heights, fallbacks) = HeightGrid::build(
            cx,
            cn,
            elevations,
            self.shapers,
            (&self.ways.channels, self.land, &self.ways.water_levels),
        );
        let origin = [
            heights.origin.0 + CHUNK_SIZE / 2.0,
            0.0,
            -(heights.origin.1 + CHUNK_SIZE / 2.0),
        ];
        let mut chunk_buildings = buildings::ChunkBuildings::default();
        for plot in self.buildings.get(&(cx, cn)).into_iter().flatten() {
            buildings::add(&mut chunk_buildings, plot, &heights, origin);
        }
        #[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
        let center = [origin[0] as f32, 0.0, origin[2] as f32];
        // Buildings of this chunk and its neighbours: plants near a border keep out of those
        // across it too.
        let footprints: Vec<vegetation::Footprint> = (-1..=1)
            .flat_map(|de| (-1..=1).map(move |dn| (cx + de, cn + dn)))
            .filter_map(|key| self.buildings.get(&key))
            .flatten()
            .map(|plot| vegetation::Footprint::around(&plot.footprint))
            .collect();
        let ground = vegetation::Ground {
            heights: &heights,
            land: self.land,
            road: self.shapers.road,
            beds: &self.ways.levels,
            streets: &self.ways.clearance,
            buildings: &footprints,
            climate: self.climate,
            urban: self.urban,
        };
        let mut trees = vegetation::place(heights.origin, CHUNK_SIZE, &ground, origin);
        let roadside = props::Roadside {
            heights: &heights,
            land: self.land,
            road: self.shapers.road,
            mouths: self.mouths,
            streets: &self.ways.clearance,
            buildings: &footprints,
            places: self.places,
            urban: self.urban,
        };
        let props = props::place(heights.origin, CHUNK_SIZE, &roadside, origin);
        vegetation::place_grass(&mut trees, heights.origin, CHUNK_SIZE, &ground, origin);
        let (mut paved, mut unpaved, barriers) = streets::meshes(
            (streets, &self.ways.corners),
            heights.origin,
            CHUNK_SIZE,
            &heights,
            self.below,
            origin,
        );
        let track_length = unpaved.indices.len();
        unpaved.append(water::shore_mesh(
            pools,
            heights.origin,
            CHUNK_SIZE,
            &heights,
            origin,
        ));
        let mut mesh = heights.mesh(self.land, origin, self.shapers, &self.ways.islands);
        paved.append(earthworks::retaining_faces(
            &mesh,
            center,
            self.shapers,
            &heights,
        ));
        self.urban.paint(&mut mesh, origin, self.land, &heights);
        let mut chunk = TerrainChunk {
            center,
            mesh,
            buildings: chunk_buildings.shells,
            modelled: chunk_buildings.cells.into_values().collect(),
            streets: paved,
            tracks: unpaved,
            urban: self.urban.mesh(&heights, origin),
            water: water::mesh(streams, pools, heights.origin, CHUNK_SIZE, &heights, origin),
            trees,
            props,
        };
        chunk.shrink_to_fit();
        (chunk, fallbacks, (barriers, track_length))
    }
}

/// `work` done for each of `jobs` on up to `threads` threads, the results in the order of
/// `jobs`. A panic in `work` is passed on.
fn in_parallel<J: Sync, R: Send>(
    jobs: &[J],
    threads: usize,
    work: impl Fn(&J) -> R + Sync,
) -> Vec<R> {
    // Each thread takes the next job when done with one: chunks differ a lot in work (a town
    // against a field).
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<R>> = std::iter::repeat_with(|| None).take(jobs.len()).collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads.clamp(1, jobs.len().max(1)))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(index) else {
                            return done;
                        };
                        done.push((index, work(job)));
                    }
                })
            })
            .collect();
        for worker in workers {
            for (index, result) in worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            {
                results[index] = Some(result);
            }
        }
    });
    results.into_iter().flatten().collect()
}

fn chunk_of(east: f64, north: f64) -> (i32, i32) {
    #[allow(clippy::cast_possible_truncation)] // world coordinates are far below 2^31 chunks
    (
        (east / CHUNK_SIZE).floor() as i32,
        (north / CHUNK_SIZE).floor() as i32,
    )
}

/// Chunk grid cells (east, north) within the corridor of any part of the route.
pub(crate) fn chunks_near_route(road: &RoadIndex) -> BTreeSet<(i32, i32)> {
    cells_near_route(road).into_keys().collect()
}

/// Chunk grid cells (east, north) within the corridor of any part of the route, each with
/// how far along the route it is first reached (in road samples).
fn cells_near_route(road: &RoadIndex) -> BTreeMap<(i32, i32), usize> {
    let mut cells = BTreeMap::new();
    let reach = CORRIDOR + CHUNK_SIZE / 2.0 * std::f64::consts::SQRT_2;
    // Sampling the road every half chunk is enough to touch every chunk in reach.
    for (sample, (east, north)) in road.samples(CHUNK_SIZE / 2.0).into_iter().enumerate() {
        let (low, high) = (
            chunk_of(east - reach, north - reach),
            chunk_of(east + reach, north + reach),
        );
        for cx in low.0..=high.0 {
            for cn in low.1..=high.1 {
                let center_e = (f64::from(cx) + 0.5) * CHUNK_SIZE;
                let center_n = (f64::from(cn) + 0.5) * CHUNK_SIZE;
                if (center_e - east).hypot(center_n - north) <= reach {
                    cells.entry((cx, cn)).or_insert(sample);
                }
            }
        }
    }
    cells
}

/// Terrain heights of one chunk: a vertex grid with a one-vertex border, and finer cells near
/// the road where the ground is shaped around it.
pub(crate) struct HeightGrid {
    /// South-west corner in metres east/north.
    origin: (f64, f64),
    /// Vertices per side, without the border.
    side: usize,
    /// Shaped heights of the grid's vertices, border included.
    heights: Vec<f64>,
    /// Natural heights of the grid's vertices, border included.
    natural: Vec<f64>,
    /// Fine cells, normally `SUB` pieces a side and more finely split between close roads.
    /// Their square lattice's heights run row by row from the south-west.
    fine: HashMap<(usize, usize), Vec<f64>>,
    /// Local downward correction beneath ground crossings (culverts), coarse and fine.
    water_drop: Vec<f64>,
    fine_water_drop: HashMap<(usize, usize), Vec<f64>>,
    water_levels: std::sync::Arc<water_levels::Levels>,
    /// Openings also cut paving laid on the hill, including approach caps and junctions.
    hollows: Vec<structures::TunnelCut>,
}

/// What a strip or area is laid on (`drape`): the ground, or the water's surface in its channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum On {
    Ground,
    Water,
}

impl HeightGrid {
    /// Vertices per side of a chunk's grid, without the border.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    const SIDE: usize = (CHUNK_SIZE / GRID) as usize + 1;

    /// Positions (east, north) of the vertices of chunk (`cx`, `cn`), border included, row by
    /// row from the south-west.
    fn vertices(cx: i32, cn: i32) -> impl Iterator<Item = (f64, f64)> {
        let bordered = Self::SIDE + 2;
        let origin = (f64::from(cx) * CHUNK_SIZE, f64::from(cn) * CHUNK_SIZE);
        (0..bordered).flat_map(move |j| {
            (0..bordered).map(move |i| {
                #[allow(clippy::cast_precision_loss)] // small grid indices
                (
                    origin.0 + (i as f64 - 1.0) * GRID,
                    origin.1 + (j as f64 - 1.0) * GRID,
                )
            })
        })
    }

    /// The model's elevations at the vertices of chunk (`cx`, `cn`), `None` where it has none.
    /// Kept apart from [`HeightGrid::build`]: the model is the one part chunks cannot share.
    async fn elevations<M: ElevationModel>(
        cx: i32,
        cn: i32,
        projection: &LocalProjection,
        model: &mut M,
    ) -> Vec<Option<f64>> {
        let mut elevations = Vec::with_capacity((Self::SIDE + 2).pow(2));
        for (east, north) in Self::vertices(cx, cn) {
            let (lat, lon) = projection.unproject(east, north);
            elevations.push(model.elevation(lat, lon).await.ok());
        }
        elevations
    }

    /// The grid of chunk (`cx`, `cn`) from its [`HeightGrid::elevations`], and how many
    /// vertices had none and follow the road instead.
    fn build(
        cx: i32,
        cn: i32,
        elevations: &[Option<f64>],
        shapers: &Shapers<'_>,
        water: (
            &channels::Channels,
            &LandIndex,
            &std::sync::Arc<water_levels::Levels>,
        ),
    ) -> (Self, usize) {
        let side = Self::SIDE;
        let bordered = side + 2;
        let origin = (f64::from(cx) * CHUNK_SIZE, f64::from(cn) * CHUNK_SIZE);
        let chunk_road_elevation = shapers
            .road
            .nearest(
                origin.0 + CHUNK_SIZE / 2.0,
                origin.1 + CHUNK_SIZE / 2.0,
                f64::INFINITY,
            )
            .map_or(0.0, |(_, elevation, _)| elevation);

        let mut natural = vec![0.0; bordered * bordered];
        let mut fallbacks = 0;
        for (k, ((east, north), elevation)) in Self::vertices(cx, cn).zip(elevations).enumerate() {
            let height = elevation.unwrap_or_else(|| {
                fallbacks += 1;
                shapers
                    .road
                    .nearest(east, north, FALLBACK_RADIUS)
                    .map_or(chunk_road_elevation, |(_, elevation, _)| elevation)
            });
            natural[k] = height;
        }
        let mut grid = Self {
            origin,
            side,
            heights: vec![0.0; bordered * bordered],
            natural,
            fine: HashMap::new(),
            water_drop: vec![0.0; bordered * bordered],
            fine_water_drop: HashMap::new(),
            water_levels: std::sync::Arc::clone(water.2),
            hollows: shapers
                .hollows
                .iter()
                .copied()
                .filter(|p| {
                    p.near(
                        origin.0 + CHUNK_SIZE / 2.0,
                        origin.1 + CHUNK_SIZE / 2.0,
                        CHUNK_SIZE,
                    )
                })
                .collect(),
        };
        // Streets level the ground across them from the natural ground at their centre lines:
        // all of it is known first.
        for (k, (east, north)) in Self::vertices(cx, cn).enumerate() {
            let shaped = grid.shaped_at(east, north, shapers);
            grid.heights[k] = shaped - carve_at(water, east, north, shaped, shapers);
            grid.water_drop[k] = crossing_drop(water, east, north, shaped, shapers);
        }
        let half_diagonal = GRID * std::f64::consts::FRAC_1_SQRT_2;
        for j in 0..side - 1 {
            for i in 0..side - 1 {
                let (east, north) = grid.position(i, j, 0.5, 0.5);
                let detail = if grid.needs_detail(i, j, shapers)
                    || water.0.near(east, north, half_diagonal)
                {
                    Some(grid.detail(i, j, shapers, water))
                } else if shapers
                    .streets
                    .nearest(east, north, half_diagonal)
                    .is_some()
                {
                    // Levelled across a street: split only where that changes the cell's
                    // ground, so cells on flat land stay plain. On the chunk's edge always, as
                    // the neighbouring chunk's cell along it may be split.
                    let edge = i == 0 || j == 0 || i == side - 2 || j == side - 2;
                    Some(grid.detail(i, j, shapers, water))
                        .filter(|(fine, _)| edge || grid.departs(i, j, fine))
                } else {
                    None
                };
                if let Some((fine, drops)) = detail {
                    grid.fine.insert((i, j), fine);
                    grid.fine_water_drop.insert((i, j), drops);
                }
            }
        }
        grid.close_edges(shapers);
        (grid, fallbacks)
    }

    /// Where a split cell meets a plain one in the chunk, its edge takes the plain cell's
    /// straight edge, so no gap opens between them: a plain cell beside a street departs a
    /// little from the levelled ground (`PLAIN_TOLERANCE`).
    fn close_edges(&mut self, shapers: &Shapers<'_>) {
        #[allow(clippy::cast_possible_wrap)] // small grid
        let cells = (self.side - 1) as isize;
        let split: Vec<(usize, usize)> = self.fine.keys().copied().collect();
        for (i, j) in split {
            #[allow(clippy::cast_possible_wrap)] // small grid indices
            let (ci, cj) = (i as isize, j as isize);
            let count = self.fine[&(i, j)].len().isqrt() - 1;
            let neighbour = |di: isize, dj: isize| {
                let (ni, nj) = (ci + di, cj + dj);
                if (0..cells).contains(&ni) && (0..cells).contains(&nj) {
                    #[allow(clippy::cast_sign_loss)]
                    return self
                        .fine
                        .get(&(ni as usize, nj as usize))
                        .map_or(1, |f| f.len().isqrt() - 1);
                }
                #[allow(clippy::cast_precision_loss)]
                let (east, north) = (
                    self.origin.0 + (ni as f64 + 0.5) * GRID,
                    self.origin.1 + (nj as f64 + 0.5) * GRID,
                );
                shapers.detail_count(east, north)
            };
            let neighbours = [
                neighbour(-1, 0),
                neighbour(1, 0),
                neighbour(0, -1),
                neighbour(0, 1),
            ];
            let (sw, se, nw, ne) = (
                self.vertex(ci, cj),
                self.vertex(ci + 1, cj),
                self.vertex(ci, cj + 1),
                self.vertex(ci + 1, cj + 1),
            );
            let Some(fine) = self.fine.get_mut(&(i, j)) else {
                continue;
            };
            for (edge, neighbour) in neighbours.into_iter().enumerate() {
                if neighbour >= count {
                    continue;
                }
                let slot = |k| match edge {
                    0 => k * (count + 1),
                    1 => k * (count + 1) + count,
                    2 => k,
                    _ => count * (count + 1) + k,
                };
                let stride = count / neighbour;
                if neighbour == 1 {
                    let (a, b) = match edge {
                        0 => (sw, nw),
                        1 => (se, ne),
                        2 => (sw, se),
                        _ => (nw, ne),
                    };
                    fine[slot(0)] = a;
                    fine[slot(count)] = b;
                }
                for start in (0..count).step_by(stride) {
                    let (a, b) = (fine[slot(start)], fine[slot(start + stride)]);
                    for k in 1..stride {
                        #[allow(clippy::cast_precision_loss)]
                        let t = k as f64 / stride as f64;
                        fine[slot(start + k)] = a + (b - a) * t;
                    }
                }
            }
        }
    }

    /// The ground at a point before channels cut it (`Shapers::shape_at`).
    fn shaped_at(&self, east: f64, north: f64, shapers: &Shapers<'_>) -> f64 {
        let natural = self.natural_at(east, north);
        let (roads, references) = shapers.near_with_references(east, north, LEVEL_REACH);
        let level = shapers.bed(east, north, natural, &roads);
        let datum = level.as_ref().map(|l| {
            l.height
                .unwrap_or_else(|| self.natural_at(l.foot.0, l.foot.1))
        });
        shapers.shape_at(
            natural,
            east,
            north,
            (&roads, &references),
            level.as_ref().zip(datum),
        )
    }

    /// Whether the fine heights of cell (`i`, `j`) depart from its plain triangles by more than
    /// `PLAIN_TOLERANCE`.
    fn departs(&self, i: usize, j: usize, fine: &[f64]) -> bool {
        #[allow(clippy::cast_possible_wrap)] // small grid indices
        let (i, j) = (i as isize, j as isize);
        let (sw, se, nw, ne) = (
            self.vertex(i, j),
            self.vertex(i + 1, j),
            self.vertex(i, j + 1),
            self.vertex(i + 1, j + 1),
        );
        let count = fine.len().isqrt() - 1;
        (0..=count).any(|b| {
            (0..=count).any(|a| {
                #[allow(clippy::cast_precision_loss)] // small counts
                let (u, v) = (a as f64 / count as f64, b as f64 / count as f64);
                let plain = on_triangles(sw, se, nw, ne, u, v);
                (fine[b * (count + 1) + a] - plain).abs() > PLAIN_TOLERANCE
            })
        })
    }

    /// Whether the ground anywhere in cell (`i`, `j`) is shaped around the road. Elsewhere it
    /// is natural, and the plain cell's flat triangles match the neighbours' edges exactly.
    fn needs_detail(&self, i: usize, j: usize, shapers: &Shapers<'_>) -> bool {
        let half_diagonal = GRID * std::f64::consts::FRAC_1_SQRT_2;
        let (east, north) = self.position(i, j, 0.5, 0.5);
        // At a tunnel portal the cutting ends: the ground steps up to the hill there.
        if shapers
            .portals
            .iter()
            .any(|p| p.near(east, north, half_diagonal + FINE))
        {
            return true;
        }
        let roads = shapers.near(east, north, LEVEL_REACH + half_diagonal);
        if roads.is_empty() {
            return false;
        }
        let nearest = roads.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
        let closest = (nearest - half_diagonal).max(0.0);
        if closest <= VERGE {
            return true;
        }
        // Natural ground within every road's cutting and embankment slopes is left as it is.
        let (low_road, high_road) = roads
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |a, r| {
                (a.0.min(r.1), a.1.max(r.1))
            });
        let corners = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(di, dj)| {
            #[allow(clippy::cast_possible_wrap)] // small grid indices
            self.natural_vertex(i as isize + di, j as isize + dj)
        });
        let (low, high) = corners
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |a, &h| {
                (a.0.min(h), a.1.max(h))
            });
        let room = closest - VERGE;
        high > low_road - ROAD_SINK + room * CUT_SLOPE
            || low < high_road - ROAD_SINK - room * FILL_SLOPE
    }

    /// The shaped bed and culvert corrections at a cell's fine vertices.
    fn detail(
        &self,
        i: usize,
        j: usize,
        shapers: &Shapers<'_>,
        water: (
            &channels::Channels,
            &LandIndex,
            &std::sync::Arc<water_levels::Levels>,
        ),
    ) -> (Vec<f64>, Vec<f64>) {
        let (east, north) = self.position(i, j, 0.5, 0.5);
        let count = shapers.detail_count(east, north);
        let mut fine = Vec::with_capacity((count + 1) * (count + 1));
        let mut drops = Vec::with_capacity((count + 1) * (count + 1));
        for b in 0..=count {
            for a in 0..=count {
                #[allow(clippy::cast_precision_loss)] // small counts
                let (u, v) = (a as f64 / count as f64, b as f64 / count as f64);
                let (east, north) = self.position(i, j, u, v);
                let shaped = self.shaped_at(east, north, shapers);
                let cut = carve_at(water, east, north, shaped, shapers);
                fine.push(shaped - cut);
                drops.push(crossing_drop(water, east, north, shaped, shapers));
            }
        }
        (fine, drops)
    }

    /// Metres east/north of the point (`u`, `v`) (0–1) of cell (`i`, `j`).
    fn position(&self, i: usize, j: usize, u: f64, v: f64) -> (f64, f64) {
        #[allow(clippy::cast_precision_loss)] // small grid indices
        (
            self.origin.0 + (i as f64 + u) * GRID,
            self.origin.1 + (j as f64 + v) * GRID,
        )
    }

    /// Shaped height at a grid vertex; `i`/`j` may be −1 or `side` (the border).
    /// The ground's triangles (corners in metres east/north) overlapping the rectangle
    /// `low`–`high`, as the mesh draws them: cells, or their fine pieces near the road, split
    /// along the south-west to north-east diagonal, clockwise seen from above.
    pub(crate) fn triangles(&self, low: (f64, f64), high: (f64, f64)) -> Vec<[(f64, f64); 3]> {
        // Cells (or pieces) of `size` from `start` that `from`–`to` touches, of `count`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped, small grid
        let touched = |from: f64, to: f64, start: f64, size: f64, count: usize| {
            let first = (((from - start) / size).floor().max(0.0) as usize).min(count - 1);
            let last = (((to - start) / size).floor().max(0.0) as usize).min(count - 1);
            first..=last
        };
        let cells = self.side - 1;
        let mut triangles = Vec::new();
        for j in touched(low.1, high.1, self.origin.1, GRID, cells) {
            for i in touched(low.0, high.0, self.origin.0, GRID, cells) {
                #[allow(clippy::cast_precision_loss)] // small grid
                let corner = (
                    self.origin.0 + i as f64 * GRID,
                    self.origin.1 + j as f64 * GRID,
                );
                let (size, count) = if let Some(fine) = self.fine.get(&(i, j)) {
                    let count = fine.len().isqrt() - 1;
                    #[allow(clippy::cast_precision_loss)]
                    let size = GRID / count as f64;
                    (size, count)
                } else {
                    (GRID, 1)
                };
                for row in touched(low.1, high.1, corner.1, size, count) {
                    for column in touched(low.0, high.0, corner.0, size, count) {
                        #[allow(clippy::cast_precision_loss)] // a few pieces
                        let sw = (
                            corner.0 + column as f64 * size,
                            corner.1 + row as f64 * size,
                        );
                        let (se, nw, ne) = (
                            (sw.0 + size, sw.1),
                            (sw.0, sw.1 + size),
                            (sw.0 + size, sw.1 + size),
                        );
                        triangles.push([sw, nw, ne]);
                        triangles.push([sw, ne, se]);
                    }
                }
            }
        }
        triangles
    }

    /// Draped ground surfaces share the terrain's tunnel openings. Cutting only the terrain
    /// leaves the approach's paving stretched across the mouth on the uncut height grid.
    pub(crate) fn cut_paving(&self, triangle: [[f64; 3]; 3]) -> Option<Vec<[[f64; 3]; 3]>> {
        if self.hollows.is_empty() {
            return None;
        }
        cut_out(&self.hollows.iter().collect::<Vec<_>>(), triangle)
    }

    /// Foundation height for a mapped bridge built in this chunk, over the whole footprint.
    pub(crate) fn foundation(
        &self,
        centre: (f64, f64),
        forward: (f64, f64),
        dimensions: (f64, f64),
    ) -> Option<f64> {
        let reach = dimensions.0.hypot(dimensions.1);
        let triangles = self
            .triangles(
                (centre.0 - reach, centre.1 - reach),
                (centre.0 + reach, centre.1 + reach),
            )
            .into_iter()
            .flat_map(|t| {
                let corners = t.map(|(e, n)| [e, n, self.at(e, n)]);
                self.cut_paving(corners).unwrap_or_else(|| vec![corners])
            });
        foundations::lowest(triangles, centre, forward, dimensions)
    }

    fn vertex(&self, i: isize, j: isize) -> f64 {
        self.heights[self.slot(i, j)]
    }

    fn natural_vertex(&self, i: isize, j: isize) -> f64 {
        self.natural[self.slot(i, j)]
    }

    fn slot(&self, i: isize, j: isize) -> usize {
        let bordered = self.side + 2;
        let clamp = |k: isize| usize::try_from(k + 1).unwrap_or(0).min(bordered - 1);
        clamp(j) * bordered + clamp(i)
    }

    /// Natural height anywhere in or around the chunk, between the grid's samples.
    fn natural_at(&self, east: f64, north: f64) -> f64 {
        let u = (east - self.origin.0) / GRID;
        let v = (north - self.origin.1) / GRID;
        let (i, j) = (u.floor(), v.floor());
        let (fu, fv) = (u - i, v - j);
        #[allow(clippy::cast_possible_truncation)] // clamped to the small grid by `slot`
        let (i, j) = (i as isize, j as isize);
        let bottom = self.natural_vertex(i, j) * (1.0 - fu) + self.natural_vertex(i + 1, j) * fu;
        let top =
            self.natural_vertex(i, j + 1) * (1.0 - fu) + self.natural_vertex(i + 1, j + 1) * fu;
        bottom * (1.0 - fv) + top * fv
    }

    /// The ground's height at any point of (or slightly around) the chunk, exactly as the mesh
    /// has it.
    /// The ground's height at a point, channels cut into it.
    pub(crate) fn at(&self, east: f64, north: f64) -> f64 {
        self.interpolate(east, north, &self.heights, &self.fine)
    }

    /// The height of what lies `on` the ground at a point: the ground, or the water's surface
    /// in its channel (the shared water datum, less the channel's drop).
    pub(crate) fn level(&self, on: On, east: f64, north: f64) -> f64 {
        match on {
            On::Ground => self.at(east, north),
            On::Water => {
                self.water_levels
                    .at(east, north)
                    .unwrap_or_else(|| self.natural_at(east, north))
                    + self.interpolate(east, north, &self.water_drop, &self.fine_water_drop)
                    - channels::DROP
            }
        }
    }

    /// A value given at the grid's vertices (`coarse`, and `fine` in detailed cells) at a point,
    /// on the triangles the mesh draws.
    fn interpolate(
        &self,
        east: f64,
        north: f64,
        coarse: &[f64],
        fine: &HashMap<(usize, usize), Vec<f64>>,
    ) -> f64 {
        let u = (east - self.origin.0) / GRID;
        let v = (north - self.origin.1) / GRID;
        // Points on the chunk's far edges belong to its last cells, not to the border.
        #[allow(clippy::cast_precision_loss)] // small grid
        let last = (self.side - 2) as f64;
        let cell = |w: f64| {
            let floor = w.floor();
            if (last + 1.0..=last + 1.0 + 1e-9).contains(&w) {
                last
            } else {
                floor
            }
        };
        let (i, j) = (cell(u), cell(v));
        let (fu, fv) = (u - i, v - j);
        #[allow(clippy::cast_possible_truncation)] // clamped to the small grid by `slot`
        let (i, j) = (i as isize, j as isize);
        if let (Ok(ci), Ok(cj)) = (usize::try_from(i), usize::try_from(j))
            && let Some(fine) = fine.get(&(ci, cj))
        {
            let count = fine.len().isqrt() - 1;
            #[allow(clippy::cast_precision_loss)] // fixed small subdivision counts
            let (su, sv) = (fu * count as f64, fv * count as f64);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (column, row) = (
                (su.floor() as usize).min(count - 1),
                (sv.floor() as usize).min(count - 1),
            );
            #[allow(clippy::cast_precision_loss)]
            let (pu, pv) = (su - column as f64, sv - row as f64);
            let piece = |column: usize, row: usize| fine[row * (count + 1) + column];
            return on_triangles(
                piece(column, row),
                piece(column + 1, row),
                piece(column, row + 1),
                piece(column + 1, row + 1),
                pu,
                pv,
            );
        }
        let vertex = |i: isize, j: isize| coarse[self.slot(i, j)];
        on_triangles(
            vertex(i, j),
            vertex(i + 1, j),
            vertex(i, j + 1),
            vertex(i + 1, j + 1),
            fu,
            fv,
        )
    }

    /// Terrain normals include channel carving near water.
    fn normal_at(&self, east: f64, north: f64, shapers: &Shapers<'_>) -> [f32; 3] {
        // Channels change the bank's slope: lighting and triplanar rock must follow the
        // carved mesh, rather than treating a newly exposed face as level ground.
        let channel = self.water_levels.at(east, north).is_some();
        let border = (east - self.origin.0).abs() < 1e-6
            || (east - self.origin.0 - CHUNK_SIZE).abs() < 1e-6
            || (north - self.origin.1).abs() < 1e-6
            || (north - self.origin.1 - CHUNK_SIZE).abs() < 1e-6;
        // Both chunks know this same coarse border, while only one knows each fine cell.
        let plain = HashMap::new();
        let height = |e: f64, n: f64| {
            if channel && border {
                self.interpolate(e, n, &self.heights, &plain)
            } else if channel {
                self.at(e, n)
            } else {
                self.shaped_at(e, n, shapers)
            }
        };
        let step = 1.0;
        let slope_east = (height(east + step, north) - height(east - step, north)) / (2.0 * step);
        let slope_north = (height(east, north + step) - height(east, north - step)) / (2.0 * step);
        unit([-slope_east, 1.0, slope_north])
    }

    /// The ground mesh relative to `origin`, coloured by land cover, with the roundabouts'
    /// `islands` raised on it.
    fn mesh(
        &self,
        land: &LandIndex,
        origin: [f64; 3],
        shapers: &Shapers<'_>,
        islands: &[roundabouts::Island],
    ) -> MeshData {
        let mut mesh = self.plain_mesh(land, origin, shapers);
        mesh.append(roundabouts::mesh(
            islands,
            self.origin,
            CHUNK_SIZE,
            self,
            origin,
        ));
        mesh
    }

    /// The ground mesh relative to `origin`, coloured by land cover.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // f32 GPU data; small grid
    fn plain_mesh(&self, land: &LandIndex, origin: [f64; 3], shapers: &Shapers<'_>) -> MeshData {
        let side = self.side;
        let mut mesh = MeshData::default();
        let push = |mesh: &mut MeshData, east: f64, north: f64, height: f64| {
            mesh.vertices.push([
                (east - origin[0]) as f32,
                (height - origin[1]) as f32,
                (-north - origin[2]) as f32,
            ]);
            mesh.normals.push(self.normal_at(east, north, shapers));
            mesh.uvs.push([(east / GRID) as f32, (north / GRID) as f32]);
            mesh.colors
                .push(landcover::color(land.cover_at(east, north)));
            u32::try_from(mesh.vertices.len() - 1).expect("chunk fits u32")
        };
        // The plain grid's vertices, shared by its cells.
        let mut grid = Vec::with_capacity(side * side);
        for j in 0..side {
            for i in 0..side {
                let (east, north) = self.position(i, j, 0.0, 0.0);
                grid.push(push(
                    &mut mesh,
                    east,
                    north,
                    self.vertex(i as isize, j as isize),
                ));
            }
        }
        for j in 0..side - 1 {
            for i in 0..side - 1 {
                let (east, north) = self.position(i, j, 0.5, 0.5);
                let hollows: Vec<&structures::TunnelCut> = self
                    .hollows
                    .iter()
                    .filter(|p| p.near(east, north, GRID))
                    .collect();
                // A piece's two triangles from its corners (index, [east, north, height]), cut
                // to a tunnel's opening where they reach into one.
                let add = |mesh: &mut MeshData, corners: [(u32, [f64; 3]); 4]| {
                    let [sw, se, nw, ne] = corners;
                    for triangle in [[sw, nw, ne], [sw, ne, se]] {
                        match cut_out(&hollows, triangle.map(|c| c.1)) {
                            None => mesh.indices.extend(triangle.map(|c| c.0)),
                            Some(pieces) => {
                                for piece in pieces {
                                    for [east, north, height] in piece {
                                        let index = push(mesh, east, north, height);
                                        mesh.indices.push(index);
                                    }
                                }
                            }
                        }
                    }
                };
                if let Some(fine) = self.fine.get(&(i, j)) {
                    let count = fine.len().isqrt() - 1;
                    let mut corners = Vec::with_capacity((count + 1) * (count + 1));
                    for b in 0..=count {
                        for a in 0..=count {
                            #[allow(clippy::cast_precision_loss)]
                            let (east, north) = self.position(
                                i,
                                j,
                                a as f64 / count as f64,
                                b as f64 / count as f64,
                            );
                            let height = fine[b * (count + 1) + a];
                            corners.push((
                                push(&mut mesh, east, north, height),
                                [east, north, height],
                            ));
                        }
                    }
                    for b in 0..count {
                        for a in 0..count {
                            let at = |a: usize, b: usize| corners[b * (count + 1) + a];
                            add(
                                &mut mesh,
                                [at(a, b), at(a + 1, b), at(a, b + 1), at(a + 1, b + 1)],
                            );
                        }
                    }
                } else {
                    let at = |i: usize, j: usize| {
                        let (east, north) = self.position(i, j, 0.0, 0.0);
                        (
                            grid[j * side + i],
                            [east, north, self.vertex(i as isize, j as isize)],
                        )
                    };
                    add(
                        &mut mesh,
                        [at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)],
                    );
                }
            }
        }
        mesh
    }
}

/// The pieces of a ground triangle (corners east, north, height) outside the openings of the
/// tunnels at `portals`, where it reaches into one; `None` where it does not. Nothing closes an
/// opening, and the ground meets the tube's ring round it (#135).
fn cut_out(
    portals: &[&structures::TunnelCut],
    triangle: [[f64; 3]; 3],
) -> Option<Vec<[[f64; 3]; 3]>> {
    let mut pieces = vec![triangle];
    let mut touched = false;
    for portal in portals {
        let mut next = Vec::new();
        for piece in pieces {
            match portal.cut(piece) {
                None => next.push(piece),
                Some(cut) => {
                    touched = true;
                    next.extend(cut);
                }
            }
        }
        pieces = next;
    }
    touched.then_some(pieces)
}

/// Keep water below a ground crossing even when its road cuts below the natural DEM.
fn crossing_drop(
    (channels, _, levels): (
        &channels::Channels,
        &LandIndex,
        &std::sync::Arc<water_levels::Levels>,
    ),
    east: f64,
    north: f64,
    ground: f64,
    shapers: &Shapers<'_>,
) -> f64 {
    levels.at(east, north).map_or(0.0, |datum| {
        (ground - datum).min(0.0) * (1.0 - channels.open(east, north, shapers))
    })
}

/// How far channels cut the ground at a point (`channels::Channels::depth`).
fn carve_at(
    (channels, land, levels): (
        &channels::Channels,
        &LandIndex,
        &std::sync::Arc<water_levels::Levels>,
    ),
    east: f64,
    north: f64,
    ground: f64,
    shapers: &Shapers<'_>,
) -> f64 {
    let inside = land.cover_at(east, north) == Some(LandCover::Water);
    if !inside && !channels.near(east, north, 0.0) {
        return 0.0;
    }
    let datum = levels.at(east, north).unwrap_or(ground);
    channels.depth(east, north, inside, ground - datum, shapers)
}

/// Height at (`u`, `v`) (0–1) of a cell with corner heights south-west, south-east,
/// north-west and north-east, split into the two triangles the mesh draws (along the
/// south-west to north-east diagonal).
fn on_triangles(sw: f64, se: f64, nw: f64, ne: f64, u: f64, v: f64) -> f64 {
    if v >= u {
        sw + (ne - nw) * u + (nw - sw) * v
    } else {
        sw + (se - sw) * u + (ne - se) * v
    }
}

/// What shapes the ground: the road ridden and the railways (`railways`), each level across just
/// below it, with cuttings and embankments; and, giving way to them, surrounding formed ways;
/// and the portals of their tunnels, where those cuttings end and the ground
/// keeps out of the openings (#135).
pub(crate) struct Shapers<'a> {
    pub(crate) road: &'a RoadIndex,
    pub(crate) rails: &'a RoadIndex,
    pub(crate) streets: &'a streets::Levels,
    pub(crate) portals: &'a [structures::Portal],
    pub(crate) hollows: &'a [structures::TunnelCut],
    /// Whether the road's tunnels get a hill heaped over them where the terrain has none
    /// (`shape`, #135): not while their portals are placed, which looks for the hill the
    /// terrain has.
    pub(crate) hills: bool,
}

impl Shapers<'_> {
    /// Fine pieces a side in the cell centred at (`east`, `north`). Beds close beside the
    /// ridden road or a railway need room for the bank between them; every cell that may hold
    /// a narrowed hard verge (`Shapers::piece_diagonal`) shares that detail.
    fn detail_count(&self, east: f64, north: f64) -> usize {
        let half_diagonal = GRID * std::f64::consts::FRAC_1_SQRT_2;
        self.streets
            .detail_near(east, north, half_diagonal, false)
            .max(
                self.streets
                    .detail_near(east, north, half_diagonal + CLOSE_REACH, true),
            )
    }

    /// Diagonal of the coarsest terrain piece that can lie at (`east`, `north`) in a split
    /// cell: any cell holding the point is split at least this finely (`detail_count`).
    fn piece_diagonal(&self, east: f64, north: f64) -> f64 {
        let count = self.streets.detail_near(east, north, CLOSE_REACH, true);
        #[allow(clippy::cast_precision_loss)] // one of the fixed subdivision counts
        let size = GRID / count as f64;
        size * std::f64::consts::SQRT_2
    }

    /// The formed way shaping the ground at a point: the nearest within its own banks, or,
    /// where the ridden road's or a railway's earthworks change the ground, the nearest lying
    /// nearer than them, whose ground those earthworks must not cross.
    fn bed(
        &self,
        east: f64,
        north: f64,
        natural: f64,
        roads: &[(f64, f64, Surface)],
    ) -> Option<streets::Level> {
        if self.streets.is_empty() {
            return None;
        }
        if let Some(level) = self.streets.nearest(east, north, 0.0) {
            return Some(level);
        }
        let ground = nearest_ground(roads)?;
        if (shape(natural, roads) - natural).abs() < 0.01 {
            return None;
        }
        // Only a way nearer than the ridden road or railway can lie between them.
        let reach = streets::LEVEL_VERGE_M + streets::LEVEL_REACH_M;
        self.streets.nearest(east, north, (ground - reach).max(0.0))
    }

    /// The terrain at a point (R141). A formed surrounding way keeps its bed and banks where it
    /// holds the ground; elsewhere the ridden road's and railways' earthworks shape it as
    /// without the way. The ridden carriageway with its verge, the hill over its tunnels and
    /// the ground its portals stand in never yield, so the ridden road looks and rides as
    /// before; the way's ribbon follows whatever ground it is left.
    fn shape_at(
        &self,
        natural: f64,
        east: f64,
        north: f64,
        (roads, references): (&[(f64, f64, Surface)], &[f64]),
        bed: Option<(&streets::Level, f64)>,
    ) -> f64 {
        let Some((level, height)) = bed else {
            return shape(natural, roads);
        };
        let beyond = level.distance - level.half;
        // A ridden structure's own ground keeps none of the way's earthworks.
        let structure = self.structure_keep(east, north, height.min(natural), roads);
        let formed = level_across(natural, height, beyond, level);
        let formed = formed + (natural - formed) * structure;
        let ground = ground_reference(roads, references);
        // Without a ridden carriageway or railway near, the verge's width is never needed.
        let hard = if ground.is_finite() {
            hard_verge(
                ground,
                beyond,
                level.padding,
                self.piece_diagonal(east, north),
            )
        } else {
            VERGE
        };
        let keep = self.keep((east, north), level, (ground, roads), hard) * (1.0 - structure);
        shape_with(formed, (roads, references), keep, hard)
    }

    /// How firmly a formed way holds the ground at a point against the soft earthworks of the
    /// ridden road and railways (0–1): fully on its bed, fading across its banks and towards
    /// their carriageways (`hard`), so no step opens at either end. Ground behind the way, seen
    /// from the ridden road, is released as well: an embankment or cutting ends at the way
    /// rather than burying it or rising again beyond it.
    fn keep(
        &self,
        (east, north): (f64, f64),
        level: &streets::Level,
        (ground, roads): (f64, &[(f64, f64, Surface)]),
        hard: f64,
    ) -> f64 {
        let beyond = level.distance - level.half;
        let own = 1.0
            - earthworks::ease(((beyond - level.verge) / streets::LEVEL_REACH_M).clamp(0.0, 1.0));
        if !ground.is_finite() {
            return own;
        }
        let street_room = (beyond - level.padding).max(0.0);
        let claim = own * bank_blend((ground - hard).max(0.0), street_room);
        if claim >= 1.0 {
            return claim;
        }
        claim.max(self.behind((east, north), level, roads))
    }

    /// Whether the formed way `level` lies between a point and the nearest ridden ground road
    /// or railway, rather than beside the point or beyond it (0–1).
    fn behind(
        &self,
        (east, north): (f64, f64),
        level: &streets::Level,
        roads: &[(f64, f64, Surface)],
    ) -> f64 {
        // Only a point outside the way, with the ridden road or railway beyond it, can be.
        let Some(nearest) = nearest_ground(roads) else {
            return 0.0;
        };
        if level.distance <= level.half || nearest <= level.half + VERGE {
            return 0.0;
        }
        let Some(((foot_east, foot_north), distance, _)) = [
            self.road.ground_foot(east, north, nearest + 1.0),
            self.rails.ground_foot(east, north, nearest + 1.0),
        ]
        .into_iter()
        .flatten()
        .min_by(|a, b| a.1.total_cmp(&b.1)) else {
            return 0.0;
        };
        if distance < 1e-6 {
            return 0.0;
        }
        let toward = (
            (foot_east - east) / distance,
            (foot_north - north) / distance,
        );
        let (de, dn) = (level.foot.0 - east, level.foot.1 - north);
        let ahead = de * toward.0 + dn * toward.1;
        let aside = (de - ahead * toward.0).hypot(dn - ahead * toward.1);
        let ramp = |t: f64| earthworks::ease((t / SHADOW_RAMP).clamp(0.0, 1.0));
        ramp(ahead - level.half) * ramp(distance - ahead - VERGE) * (1.0 - ramp(aside - level.half))
    }

    /// How much of the ground at a point a ridden structure keeps for itself (0–1): the hill
    /// over a shallow tunnel tube, and the ground before and around each portal. A way above
    /// a deep tunnel keeps its own bed.
    fn structure_keep(
        &self,
        east: f64,
        north: f64,
        low: f64,
        roads: &[(f64, f64, Surface)],
    ) -> f64 {
        let (half, crown) = structures::tube(false);
        let side = half + structures::PORTAL_SIDE;
        // The pieces found for shaping include the ridden road's tunnels once portals are
        // placed; while they are being placed, look them up.
        let tunnels: Vec<_> = if self.hills {
            roads
                .iter()
                .copied()
                .filter(|r| r.2 == Surface::Tunnel)
                .collect()
        } else {
            self.road
                .near_shaped(east, north, side + structures::PORTAL_FADE)
                .into_iter()
                .filter(|r| r.2 == Surface::Tunnel)
                .collect()
        };
        // A portal's ground lies beside its tunnel: without one near, there is none.
        let near_tunnel = !tunnels.is_empty()
            || self
                .rails
                .near(east, north, PORTAL_REACH)
                .iter()
                .any(|r| r.2 == Surface::Tunnel);
        let mut keep: f64 = if near_tunnel {
            self.portals
                .iter()
                .map(|portal| portal.embeds(east, north))
                .fold(0.0, f64::max)
        } else {
            0.0
        };
        for (distance, elevation, _) in tunnels {
            if distance > side + structures::PORTAL_FADE {
                continue;
            }
            let beside =
                earthworks::ease(((distance - side) / structures::PORTAL_FADE).clamp(0.0, 1.0));
            let deep = earthworks::ease(
                ((low - (elevation + crown + TUNNEL_HILL_COVER)) / TUNNEL_KEEP_DEPTH)
                    .clamp(0.0, 1.0),
            );
            keep = keep.max((1.0 - beside) * (1.0 - deep));
        }
        keep
    }

    /// How firmly a formed way holds the ground at a point (`Shapers::keep`), for dressing the
    /// banks it alone formed.
    pub(crate) fn holds(&self, east: f64, north: f64, level: &streets::Level, natural: f64) -> f64 {
        let (roads, references) = self.near_with_references(east, north, LEVEL_REACH);
        let height = level.height.unwrap_or(natural);
        let ground = ground_reference(&roads, &references);
        let hard = hard_verge(
            ground,
            level.distance - level.half,
            level.padding,
            self.piece_diagonal(east, north),
        );
        self.keep((east, north), level, (ground, &roads), hard)
            * (1.0 - self.structure_keep(east, north, height.min(natural), &roads))
    }

    /// Every piece of road or railway within `reach` that shapes the ground, as
    /// [`RoadIndex::near`] gives them: the road's tunnels only with `hills`, a railway's never.
    pub(crate) fn near(&self, east: f64, north: f64, reach: f64) -> Vec<(f64, f64, Surface)> {
        self.near_with_references(east, north, reach).0
    }

    /// [`Shapers::near`], with each piece's distance against the six-metre reference
    /// cross-section of `VERGE`: a railway's narrower bed keeps a narrower hard verge.
    fn near_with_references(
        &self,
        east: f64,
        north: f64,
        reach: f64,
    ) -> (Vec<(f64, f64, Surface)>, Vec<f64>) {
        let mut found: Vec<_> = self
            .road
            .near_shaped(east, north, reach)
            .into_iter()
            .filter(|r| self.hills || r.2 != Surface::Tunnel)
            .collect();
        // The ridden carriageway and its verge have one datum. A lower parallel railway
        // may cut its own bank, but must not drag the road's shoulder and protection down.
        let ground_road = found
            .iter()
            .filter(|r| r.2 == Surface::Ground)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .copied();
        found.extend(self.streets.bridges.near_shaped(east, north, reach));
        let mut references: Vec<f64> = found.iter().map(|r| r.0).collect();
        for mut rail in self
            .rails
            .near(east, north, reach)
            .into_iter()
            .filter(|r| r.2 != Surface::Tunnel)
        {
            if let Some((distance, height, _)) = ground_road
                && height > rail.1
            {
                // Include the protection's footing and the fine triangles
                // touching it, beyond the margin needed by the asphalt alone.
                let roadside = (distance - VERGE - 1.0).max(0.0);
                let trackside =
                    (rail.0 - railways::BED_M / 2.0 - FINE * std::f64::consts::SQRT_2).max(0.0);
                let share = if roadside + trackside > 1e-9 {
                    roadside / (roadside + trackside)
                } else {
                    0.0
                };
                let blend = share * share * (3.0 - 2.0 * share);
                let level = height + (rail.1 - height) * blend;
                // Ease the railway cutting between the protected road verge and
                // its own bed, rather than dropping it at a hard distance threshold.
                let room = (rail.0 - VERGE).max(0.0);
                rail.1 = rail.1.max(level - room * CUT_SLOPE);
            }
            references.push(rail.0 + ROAD_HALF_WIDTH - railways::BED_M / 2.0);
            found.push(rail);
        }
        (found, references)
    }
}

/// The reference distance (`Shapers::near_with_references`) to the nearest ground piece.
fn ground_reference(roads: &[(f64, f64, Surface)], references: &[f64]) -> f64 {
    roads
        .iter()
        .zip(references)
        .filter(|(r, _)| r.2 == Surface::Ground)
        .map(|(_, &reference)| reference)
        .fold(f64::INFINITY, f64::min)
}

/// Where the ground a ridden carriageway or railway bed keeps for itself ends, as a reference
/// distance from its centre (R141): its verge as without surrounding ways, narrowed only as
/// far as a formed way `beyond` metres away needs room for its own bed, and never past the
/// margin that keeps every terrain piece touching the carriageway below it.
fn hard_verge(ground: f64, beyond: f64, padding: f64, diagonal: f64) -> f64 {
    let room = (ground - ROAD_HALF_WIDTH).max(0.0) + beyond.max(0.0);
    let needed = padding + YIELD_RAMP + diagonal;
    ROAD_HALF_WIDTH
        + (room - needed)
            .min(VERGE - ROAD_HALF_WIDTH)
            .max(diagonal + HARD_MARGIN)
}

/// The distance to the nearest piece on the ground among `roads`.
fn nearest_ground(roads: &[(f64, f64, Surface)]) -> Option<f64> {
    roads
        .iter()
        .filter(|r| r.2 == Surface::Ground)
        .map(|r| r.0)
        .min_by(f64::total_cmp)
}

/// Blend the bank between protected footprints, retaining the ridden datum on overlap.
fn bank_blend(road_room: f64, street_room: f64) -> f64 {
    let t = if road_room + street_room > 1e-9 {
        road_room / (road_room + street_room)
    } else {
        0.0
    };
    t * t * (3.0 - 2.0 * t)
}

/// The ground at a point with natural height `natural`, shaped around the pieces of road near
/// it (distance, road elevation, surface): level just below the road out to the verge, then
/// cut into the hillside or banked down to the valley at most as steeply as cuttings and
/// embankments are, natural again further away. Under bridges the ground is only lowered.
/// Over a tunnel of the road ridden the ground is only raised: a tunnel runs under a hill, and
/// where the terrain has none, as on a cliff the samples smooth away or under a gallery, the
/// ground is heaped over the tube and banked down beyond it, so no tube lies bare on the
/// hillside (#135). Where the road passes more than once (hairpins), the ground stays below
/// every pass.
pub(crate) fn shape(natural: f64, roads: &[(f64, f64, Surface)]) -> f64 {
    shape_yielding(natural, roads, |_| 0.0)
}

/// [`shape`] around ground a surrounding way formed (R141): each piece's cuttings, embankments
/// and cut-off slopes beyond its carriageway and verge (`hard`, `hard_verge`) yield by `keep`
/// (0–1). Within that verge, and over the ridden road's tunnels, nothing yields.
fn shape_with(
    ground: f64,
    (roads, references): (&[(f64, f64, Surface)], &[f64]),
    keep: f64,
    hard: f64,
) -> f64 {
    if keep <= 0.0 {
        return shape(ground, roads);
    }
    shape_yielding(ground, roads, |i| {
        keep * earthworks::ease(((references[i] - hard) / YIELD_RAMP).clamp(0.0, 1.0))
    })
}

/// [`shape`], where piece `i` lets the ground keep `yielding(i)` (0–1) of its own height
/// beyond the earthworks it would otherwise impose.
fn shape_yielding(
    natural: f64,
    roads: &[(f64, f64, Surface)],
    yielding: impl Fn(usize) -> f64,
) -> f64 {
    let nearest = roads
        .iter()
        .enumerate()
        .filter(|(_, r)| r.2 != Surface::Tunnel)
        .min_by(|a, b| a.1.0.total_cmp(&b.1.0));
    // Past a portal the line's pieces count as its tunnel's (`road`): the hill's ground there.
    let mut height = match nearest {
        Some((i, &(distance, elevation, surface))) => {
            let level = elevation - ROAD_SINK;
            let room = (distance - VERGE).max(0.0);
            let (floor, ceiling) = (level - room * FILL_SLOPE, level + room * CUT_SLOPE);
            let shaped = match surface {
                // The valley under a bridge stays open: only ground above the deck is cut away.
                Surface::Bridge => natural.min(ceiling),
                Surface::Ground | Surface::Tunnel => natural.clamp(floor, ceiling),
            };
            let shaped = shaped + (natural - shaped) * fade(distance);
            shaped + (natural - shaped) * yielding(i)
        }
        None => natural,
    };
    for &(distance, elevation, surface) in roads {
        if surface == Surface::Tunnel {
            let (half, crown) = structures::tube(false);
            let top = elevation + crown + TUNNEL_HILL_COVER;
            let hill = top - (distance - (half + TUNNEL_HILL_SHOULDER)).max(0.0) * FILL_SLOPE;
            height = height.max(hill + (natural - hill) * fade(distance));
        }
    }
    for (i, &(distance, elevation, surface)) in roads.iter().enumerate() {
        if surface == Surface::Tunnel {
            continue;
        }
        let ceiling = elevation - ROAD_SINK + (distance - VERGE).max(0.0) * CUT_SLOPE;
        let allowed = ceiling + (natural - ceiling).max(0.0) * fade(distance).max(yielding(i));
        height = height.min(allowed);
    }
    height
}

/// The natural ground at a point as the chunks draw it: the terrain sampled on their `GRID`
/// lattice and interpolated between the samples, as [`HeightGrid::natural_at`] does. The
/// terrain tiles are finer than the lattice, so where the ground rises abruptly, as the hill
/// over a tunnel's portal does, the drawn hill lies lower than the tiles have it: whatever
/// stands against the drawn ground is placed by the drawn ground (#135). `None` where the model
/// has no data.
pub(crate) async fn drawn_natural<M: ElevationModel>(
    model: &mut M,
    projection: &LocalProjection,
    (east, north): (f64, f64),
) -> Option<f64> {
    let (u, v) = (east / GRID, north / GRID);
    let (i, j) = (u.floor(), v.floor());
    let (fu, fv) = (u - i, v - j);
    let mut corners = [0.0; 4];
    for (slot, (di, dj)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        let (lat, lon) = projection.unproject((i + di) * GRID, (j + dj) * GRID);
        corners[slot] = model.elevation(lat, lon).await.ok()?;
    }
    let [sw, se, nw, ne] = corners;
    let bottom = sw * (1.0 - fu) + se * fu;
    let top = nw * (1.0 - fu) + ne * fu;
    Some(bottom * (1.0 - fv) + top * fv)
}

/// The ground at a point as the chunks draw it, before channels cut it: the drawn natural
/// ground shaped around the formed way nearest to it and around the road
/// ridden and the railways, as [`HeightGrid::shaped_at`] does. `None` where the model has no
/// data.
pub(crate) async fn drawn_ground<M: ElevationModel>(
    model: &mut M,
    projection: &LocalProjection,
    shapers: &Shapers<'_>,
    (east, north): (f64, f64),
) -> Option<f64> {
    let natural = drawn_natural(model, projection, (east, north)).await?;
    let (roads, references) = shapers.near_with_references(east, north, LEVEL_REACH);
    let level = shapers.bed(east, north, natural, &roads);
    let datum = match &level {
        Some(level) => Some(match level.height {
            Some(height) => height,
            None => drawn_natural(model, projection, level.foot).await?,
        }),
        None => None,
    };
    Some(shapers.shape_at(
        natural,
        east,
        north,
        (&roads, &references),
        level.as_ref().zip(datum),
    ))
}

/// The ground at a point with natural height `natural`, `beyond` metres outside the edge of
/// formed `way` whose shared profile lies at `level` (`earthworks::shape`): level across rather
/// than tilted with the hillside, then its earth banks or, for paved roads on steep hillsides,
/// battered retaining banks.
fn level_across(natural: f64, level: f64, beyond: f64, way: &streets::Level) -> f64 {
    earthworks::shape(natural, level, beyond, (way.verge, way.facing))
}

/// 0 where the ground is fully shaped around the road, rising to 1 at `LEVEL_REACH`.
fn fade(distance: f64) -> f64 {
    let t = ((distance - (LEVEL_REACH - REACH_FADE)) / REACH_FADE).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[allow(clippy::cast_possible_truncation)]
fn unit(v: [f64; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [
        (v[0] / length) as f32,
        (v[1] / length) as f32,
        (v[2] / length) as f32,
    ]
}

/// A deterministic pseudo-random number in `[0, 1)` from a seed (`SplitMix64`), so the world
/// looks the same every time a route is loaded.
pub(crate) fn hash(seed: i64) -> f64 {
    #[allow(clippy::cast_sign_loss)] // bit reinterpretation
    let mut z = (seed as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    #[allow(clippy::cast_precision_loss)] // 53 significant bits are plenty
    let value = (z >> 11) as f64 / (1u64 << 53) as f64;
    value
}

#[cfg(test)]
mod tests;
