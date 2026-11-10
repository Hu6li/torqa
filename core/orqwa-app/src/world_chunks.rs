//! The loaded world's chunks, read one at a time while it is ridden (ADR 0040, R240): every
//! world is built into a store in the cache, or read from its kept world, so the core holds
//! the parts built whole and the chunks' centres, not the chunks.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use orqwa_storage::worlds::{Building, Chunks};
use orqwa_world::{SnowGroundMask, TerrainChunk, World};
use tracing::warn;

/// A world's chunks, by index.
#[derive(Debug)]
pub struct WorldChunks {
    held: Held,
    /// The winter ground's mask, made in the background once it is first asked for: it needs
    /// every chunk.
    snow: OnceLock<SnowGroundMask>,
    /// Whether the mask was asked for.
    asked: AtomicBool,
}

#[derive(Debug)]
enum Held {
    /// On disk.
    Stored(Chunks),
    /// In memory, where no store could be written (ADR 0040: desktops only rely on it).
    /// A chunk missing here could not be read back from the store it began in.
    Memory {
        centres: Vec<[f32; 3]>,
        chunks: Vec<Option<TerrainChunk>>,
    },
}

impl WorldChunks {
    /// Chunks read from a kept world.
    pub(crate) fn kept(chunks: Chunks) -> Self {
        Self::new(Held::Stored(chunks))
    }

    fn new(held: Held) -> Self {
        Self {
            held,
            snow: OnceLock::new(),
            asked: AtomicBool::new(false),
        }
    }

    /// How many chunks the world has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.centres().len()
    }

    /// Whether the world has no chunks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The chunks' centres by index, known without reading the chunks.
    #[must_use]
    pub fn centres(&self) -> &[[f32; 3]] {
        match &self.held {
            Held::Stored(chunks) => chunks.centres(),
            Held::Memory { centres, .. } => centres,
        }
    }

    /// Chunk `index`, read from disk unless held. `None` past the last chunk, and for one that
    /// cannot be read, e.g. the system cleared the cache: it stays undrawn, and a kept world
    /// it is missing from is built again on the course's next ride.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Cow<'_, TerrainChunk>> {
        if index >= self.len() {
            return None;
        }
        match &self.held {
            Held::Stored(chunks) => chunks
                .read(index)
                .inspect_err(|error| warn!(%error, index, "cannot read a chunk of the world"))
                .ok()
                .map(Cow::Owned),
            Held::Memory { chunks, .. } => chunks[index].as_ref().map(Cow::Borrowed),
        }
    }

    /// The mask that keeps the winter ground's snow off the ways, once
    /// [made](Self::make_snow_ground_mask).
    pub(crate) fn snow_ground_mask(&self) -> Option<&SnowGroundMask> {
        self.snow.get()
    }

    /// Whether the mask is to be made: true for the first who asks.
    pub(crate) fn ask_snow_ground_mask(&self) -> bool {
        !self.asked.swap(true, Ordering::Relaxed)
    }

    /// Makes the mask that keeps the winter ground's snow off the ways of the world whose
    /// parts built whole are `world`, from every chunk: seconds for a long, densely mapped
    /// course. A chunk that cannot be read is undrawn, so the mask leaves it out.
    pub(crate) fn make_snow_ground_mask(&self, world: &World) {
        self.snow.get_or_init(|| {
            let mask = Mutex::new(SnowGroundMask::around(world, self.centres()));
            match &self.held {
                Held::Stored(chunks) => {
                    let cleared = chunks.read_each(&|_, chunk| {
                        lock(&mask).clear_chunk(&chunk);
                    });
                    if let Err(error) = cleared {
                        warn!(%error, "cannot read all of the world for its winter ground");
                    }
                }
                Held::Memory { chunks, .. } => {
                    for chunk in chunks.iter().flatten() {
                        lock(&mask).clear_chunk(chunk);
                    }
                }
            }
            mask.into_inner().unwrap_or_else(PoisonError::into_inner)
        });
    }

    /// Keeps the world as the course with `fingerprint`'s among `kept`, its parts built whole
    /// `world` and by the generator of version `generator`. Blocks for seconds.
    pub(crate) fn keep(
        &self,
        kept: &orqwa_storage::worlds::KeptWorlds,
        fingerprint: &str,
        world: &World,
        generator: u32,
    ) {
        let kept = match &self.held {
            Held::Stored(chunks) => chunks.keep(kept, fingerprint, world, generator),
            Held::Memory { chunks, .. } => {
                let Some(chunks) = chunks.iter().cloned().collect::<Option<Vec<_>>>() else {
                    warn!("cannot keep the course's world: a chunk of it is missing");
                    return;
                };
                kept.write_parts(fingerprint, world, &chunks, generator)
            }
        };
        if let Err(error) = kept {
            warn!(%error, "cannot keep the course's world");
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Takes each chunk of a world as it is finished: into its store, or into memory when none can
/// be written.
pub(crate) struct Sink {
    store: Option<Building>,
    /// Whether the store failed already, so that is reported once.
    failed: AtomicBool,
    centres: Mutex<Vec<[f32; 3]>>,
    /// The chunks no store holds, by index.
    held: Mutex<Vec<Option<TerrainChunk>>>,
}

impl Sink {
    /// A sink into `store`, or into memory without one.
    pub(crate) fn new(store: Option<Building>) -> Self {
        Self {
            store,
            failed: AtomicBool::new(false),
            centres: Mutex::new(Vec::new()),
            held: Mutex::new(Vec::new()),
        }
    }

    /// Takes chunk `index`, just finished. Called on the cores that finish chunks.
    pub(crate) fn take(&self, index: usize, chunk: TerrainChunk) {
        set(&self.centres, index, chunk.center);
        if let Some(store) = self.store.as_ref().filter(|store| store.whole()) {
            match store.write(index, &chunk) {
                Ok(()) => return,
                Err(error) => {
                    if !self.failed.swap(true, Ordering::Relaxed) {
                        warn!(%error, "cannot store the world being built; holding it in memory");
                    }
                }
            }
        }
        set(&self.held, index, Some(chunk));
    }

    /// The world's chunks, once every one was taken. Where the store failed, those written
    /// before are read back into memory, and the store is deleted. Blocks for that.
    pub(crate) fn finish(self) -> WorldChunks {
        let centres = self
            .centres
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self
            .held
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        match self.store {
            Some(store) if store.whole() => {
                WorldChunks::new(Held::Stored(Chunks::built(store, centres)))
            }
            store => {
                held.resize_with(centres.len(), || None);
                if let Some(store) = store {
                    for (index, chunk) in held.iter_mut().enumerate() {
                        if chunk.is_none() {
                            *chunk = store
                                .read(index)
                                .inspect_err(|error| {
                                    warn!(%error, index, "a chunk of the world is lost");
                                })
                                .ok();
                        }
                    }
                }
                WorldChunks::new(Held::Memory {
                    centres,
                    chunks: held,
                })
            }
        }
    }
}

/// Sets item `index` of `items`, growing it as needed: chunks are finished in no set order.
fn set<T: Default + Clone>(items: &Mutex<Vec<T>>, index: usize, item: T) {
    let mut items = lock(items);
    if items.len() <= index {
        items.resize(index + 1, T::default());
    }
    items[index] = item;
}
