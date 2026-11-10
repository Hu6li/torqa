//! A world built chunk by chunk, holding only the chunks around those being built (ADR 0040).
//!
//! A chunk is *built* by `Corridor::chunk`; its ground and its pavement as built (before any
//! barrier is cleared from it) are kept for its neighbours. It is *finished* once its 3×3
//! neighbours are built: protection is set on their ground, its barriers and fences are
//! cleared against their pavement, and it goes to the sink. Its ground and pavement are
//! *dropped* once every neighbour is finished and every bridge over them is built. Each step
//! reads only what the whole-world passes it replaces read near the chunk, so the world comes
//! out byte for byte the same: in debug builds, a lookup beyond what is at hand stops.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Mutex;

use crate::foundations::Ground;
use crate::pavement::{Asked, Barrier, Pavement};
use crate::structures::{Below, Piece};
use crate::{CHUNK_SIZE, MeshData, TerrainChunk, chunk_of, in_parallel, props};

/// How far a chunk's pavement, and the checks its barriers and fences make, may reach beyond
/// its square: streets are cut at the square's edges, so only their width, junction fans and
/// shores reach over. Neighbours then hold all the pavement a check can meet, as long as both
/// reaches together stay below a chunk's width.
const SPILL: f64 = 200.0;

/// A built chunk waiting for its neighbours.
struct Waiting {
    chunk: TerrainChunk,
    barriers: [Vec<Barrier>; 2],
}

/// What neighbours need of a built chunk until it is dropped.
struct AsBuilt {
    center: [f32; 3],
    ground: MeshData,
    pavement: Vec<[[f64; 3]; 3]>,
}

/// A bridge waiting for the chunks under it.
struct Bridge {
    piece: usize,
    /// The chunks its ground and pavement are read from, in index order.
    needs: Vec<usize>,
    /// The course's chunks around them, which it must not reach.
    beyond: Vec<usize>,
}

/// The chunks and bridges of a world being built.
pub(crate) struct Stages<'a> {
    /// The corridor's cells, in the order of `World::chunks`.
    cells: Vec<(i32, i32)>,
    index: HashMap<(i32, i32), usize>,
    /// The road ridden's pavement, which every check also meets.
    road: &'a Pavement,
    built: BTreeMap<usize, AsBuilt>,
    waiting: BTreeMap<usize, Waiting>,
    is_built: Vec<bool>,
    finished: Vec<bool>,
    /// How many bridges waiting still read each chunk.
    holds: Vec<usize>,
    bridges: Vec<Bridge>,
    /// Bridges built, by piece: the mesh and the index ranges of barriers pavement crosses.
    structures: Vec<Option<(MeshData, Vec<Range<usize>>)>>,
}

impl<'a> Stages<'a> {
    /// The stages of the corridor's `cells` (in the order of `World::chunks`) and of the
    /// bridges among `pieces`.
    pub(crate) fn new(cells: Vec<(i32, i32)>, road: &'a Pavement, pieces: &[Piece]) -> Self {
        let index: HashMap<_, _> = cells.iter().enumerate().map(|(i, &c)| (c, i)).collect();
        let mut holds = vec![0; cells.len()];
        let mut bridges = Vec::new();
        for (piece, structure) in pieces.iter().enumerate() {
            if !structure.is_bridge() {
                continue;
            }
            let (low, high) = structure.bounds();
            let around = |margin: f64| {
                let (low, high) = (
                    chunk_of(low.0 - margin, low.1 - margin),
                    chunk_of(high.0 + margin, high.1 + margin),
                );
                let mut found: Vec<usize> = (low.0..=high.0)
                    .flat_map(|cx| (low.1..=high.1).map(move |cn| (cx, cn)))
                    .filter_map(|cell| index.get(&cell).copied())
                    .collect();
                found.sort_unstable();
                found
            };
            // The bridge's checks stay near its line, and no chunk's pavement reaches further
            // than `SPILL` beyond its square: chunks a chunk's width away hold all it can meet.
            let needs = around(CHUNK_SIZE);
            let beyond = around(2.0 * CHUNK_SIZE)
                .into_iter()
                .filter(|i| needs.binary_search(i).is_err())
                .collect();
            for &i in &needs {
                holds[i] += 1;
            }
            bridges.push(Bridge {
                piece,
                needs,
                beyond,
            });
        }
        Self {
            is_built: vec![false; cells.len()],
            finished: vec![false; cells.len()],
            structures: pieces.iter().map(|_| None).collect(),
            cells,
            index,
            road,
            built: BTreeMap::new(),
            waiting: BTreeMap::new(),
            holds,
            bridges,
        }
    }

    /// Index in `World::chunks` of the chunk at `cell`.
    pub(crate) fn index(&self, cell: (i32, i32)) -> usize {
        self.index[&cell]
    }

    /// Takes chunk `index` as built, with its street and track barriers and the length of its
    /// tracks before shore gravel.
    pub(crate) fn built(
        &mut self,
        index: usize,
        chunk: TerrainChunk,
        (barriers, track_length): ([Vec<Barrier>; 2], usize),
    ) {
        let pavement = Pavement::as_built(&chunk, track_length);
        debug_assert!(
            pavement
                .iter()
                .flatten()
                .all(|p| within(chunk.center, p[0], p[2])),
            "chunk {:?} has pavement beyond its neighbours' reach",
            self.cells[index]
        );
        self.built.insert(
            index,
            AsBuilt {
                center: chunk.center,
                ground: chunk.mesh.clone(),
                pavement,
            },
        );
        self.waiting.insert(index, Waiting { chunk, barriers });
        self.is_built[index] = true;
    }

    /// The course's chunks among the 3×3 around `index`, in index order, and those in the ring
    /// around them.
    fn neighbours(&self, index: usize, ring: i32) -> Vec<usize> {
        let (cx, cn) = self.cells[index];
        let mut found: Vec<usize> = (-ring..=ring)
            .flat_map(|de| (-ring..=ring).map(move |dn| (cx + de, cn + dn)))
            .filter_map(|cell| self.index.get(&cell).copied())
            .collect();
        found.sort_unstable();
        found
    }

    fn ground(&self, near: &[usize], beyond: impl Iterator<Item = usize>) -> Ground<'_> {
        Ground::near(
            near.iter()
                .map(|i| (self.built[i].center, &self.built[i].ground)),
            beyond
                .filter(|i| near.binary_search(i).is_err())
                .map(|i| centre(self.cells[i]))
                .collect(),
        )
    }

    /// Finishes every chunk whose neighbours are built, and gives them to `sink` with their
    /// indices. Chunks go on all `threads`.
    pub(crate) fn finish(&mut self, threads: usize, sink: &mut dyn FnMut(usize, TerrainChunk)) {
        let ready: Vec<usize> = self
            .waiting
            .keys()
            .copied()
            .filter(|&i| self.neighbours(i, 1).iter().all(|&n| self.is_built[n]))
            .collect();
        let jobs: Vec<(usize, Mutex<Option<Waiting>>)> = ready
            .iter()
            .map(|i| (*i, Mutex::new(self.waiting.remove(i))))
            .collect();
        let this = &*self;
        let done = in_parallel(&jobs, threads, |(index, waiting)| {
            let waiting = waiting
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .expect("a chunk is finished once");
            this.finish_one(*index, waiting)
        });
        for (index, chunk) in ready.into_iter().zip(done) {
            self.finished[index] = true;
            sink(index, chunk);
        }
    }

    fn finish_one(&self, index: usize, waiting: Waiting) -> TerrainChunk {
        let Waiting {
            mut chunk,
            barriers: [paved, unpaved],
        } = waiting;
        let near = self.neighbours(index, 1);
        props::ground_protection(
            &mut chunk,
            &self.ground(&near, self.neighbours(index, 2).into_iter()),
        );
        let mut asked = Asked::default();
        asked.barriers(&chunk.streets, &paved, chunk.center);
        asked.barriers(&chunk.tracks, &unpaved, chunk.center);
        for ([a, b], half_width) in props::fence_spans(&chunk) {
            asked.span(a, b, half_width);
        }
        debug_assert!(
            asked.all(|x, z| within(chunk.center, x, z)),
            "chunk {:?} checks pavement beyond its neighbours",
            self.cells[index]
        );
        let pavement = Pavement::near_parts(
            self.road,
            near.iter().map(|i| self.built[i].pavement.as_slice()),
            asked,
        );
        pavement.clear(&mut chunk.streets, paved, chunk.center);
        pavement.clear(&mut chunk.tracks, unpaved, chunk.center);
        props::clear_fences(&mut chunk, &pavement);
        chunk
    }

    /// Builds every bridge whose chunks are built, standing on `below`'s ways.
    pub(crate) fn build_bridges(&mut self, pieces: &[Piece], below: &Below<'_>) {
        let (ready, waiting): (Vec<Bridge>, Vec<Bridge>) = std::mem::take(&mut self.bridges)
            .into_iter()
            .partition(|bridge| bridge.needs.iter().all(|&i| self.is_built[i]));
        self.bridges = waiting;
        for bridge in ready {
            let ground = self.ground(&bridge.needs, bridge.beyond.iter().copied());
            let (mut mesh, barriers) = pieces[bridge.piece].bridge(below, &ground);
            let mut asked = Asked::default();
            asked.barriers(&mesh, &barriers, [0.0; 3]);
            let pavement = Pavement::near_parts(
                self.road,
                bridge
                    .needs
                    .iter()
                    .map(|i| self.built[i].pavement.as_slice()),
                asked,
            );
            let blocked = pavement.recess(&mut mesh, barriers, [0.0; 3]);
            self.structures[bridge.piece] = Some((mesh, blocked));
            for &i in &bridge.needs {
                self.holds[i] -= 1;
            }
        }
    }

    /// Drops the ground and pavement no chunk or bridge still waiting needs.
    pub(crate) fn drop_unneeded(&mut self) {
        let unneeded: Vec<usize> = self
            .built
            .keys()
            .copied()
            .filter(|&i| {
                self.holds[i] == 0 && self.neighbours(i, 1).iter().all(|&n| self.finished[n])
            })
            .collect();
        for i in unneeded {
            self.built.remove(&i);
        }
    }

    /// The bridges built, by piece (`None` for tunnels): each mesh and the index ranges of
    /// its barriers pavement crosses.
    pub(crate) fn take_bridges(&mut self) -> Vec<Option<(MeshData, Vec<Range<usize>>)>> {
        std::mem::take(&mut self.structures)
    }

    /// Whether every chunk and bridge is done and nothing is held.
    pub(crate) fn done(&self) -> bool {
        self.waiting.is_empty() && self.bridges.is_empty() && self.built.is_empty()
    }
}

/// Whether a point (Godot `x`, `z`) lies within [`SPILL`] of the square of the chunk centred at
/// `center`.
fn within(center: [f32; 3], x: f64, z: f64) -> bool {
    let reach = CHUNK_SIZE / 2.0 + SPILL;
    (x - f64::from(center[0])).abs() <= reach && (z - f64::from(center[2])).abs() <= reach
}

/// Centre of the chunk at `cell`, as `Corridor::chunk` sets it.
#[allow(clippy::cast_possible_truncation)] // geometry is stored as f32 for the GPU
fn centre((cx, cn): (i32, i32)) -> [f32; 3] {
    [
        ((f64::from(cx) + 0.5) * CHUNK_SIZE) as f32,
        0.0,
        (-(f64::from(cn) + 0.5) * CHUNK_SIZE) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn chunk(cell: (i32, i32)) -> TerrainChunk {
        TerrainChunk {
            center: centre(cell),
            mesh: MeshData::default(),
            buildings: MeshData::default(),
            modelled: Vec::new(),
            streets: MeshData::default(),
            tracks: MeshData::default(),
            urban: MeshData::default(),
            water: MeshData::default(),
            trees: crate::Trees::default(),
            props: crate::Props::default(),
        }
    }

    /// Builds a corridor three chunks wide along a straight route `length` chunks long, in
    /// batches along it as `generate_into` does. Checks that each chunk is finished once, only
    /// after its neighbours are built, and gives the most chunks held at once.
    fn build_along(length: i32) -> usize {
        let cells: Vec<(i32, i32)> = (0..length)
            .flat_map(|cx| (-1..=1).map(move |cn| (cx, cn)))
            .collect();
        let road = Pavement::default();
        let mut stages = Stages::new(cells.clone(), &road, &[]);
        let mut built = HashSet::new();
        let mut finished = vec![0; cells.len()];
        let mut held = 0;
        for batch in cells.chunks(8) {
            for &cell in batch {
                stages.built(
                    stages.index(cell),
                    chunk(cell),
                    ([Vec::new(), Vec::new()], 0),
                );
                built.insert(cell);
            }
            stages.finish(2, &mut |index, chunk| {
                let (cx, cn) = cells[index];
                assert_eq!(chunk.center, centre((cx, cn)));
                let around = (cx - 1..=cx + 1).flat_map(|x| (cn - 1..=cn + 1).map(move |n| (x, n)));
                for neighbour in around.filter(|cell| cells.contains(cell)) {
                    assert!(built.contains(&neighbour), "{neighbour:?} not built yet");
                }
                finished[index] += 1;
            });
            held = held.max(stages.built.len());
            stages.drop_unneeded();
        }
        assert!(stages.done(), "nothing is left held");
        assert!(finished.iter().all(|&n| n == 1), "each chunk finished once");
        held
    }

    #[test]
    fn chunks_held_while_building_follow_the_surroundings_not_the_route_length() {
        let short = build_along(40);
        assert_eq!(build_along(400), short);
        assert!(
            short <= 8 * 3,
            "{short} chunks held of a corridor three wide"
        );
    }
}
