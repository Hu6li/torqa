//! The loaded world's chunks, read one at a time while it is ridden (ADR 0040, R240): every
//! world is built into a store in the cache, or read from its kept world, so the core holds
//! the parts built whole and the chunks' centres, not the chunks. A world is ridden while the
//! rest of it is still being built: each chunk can be read once it is finished.

use std::borrow::Cow;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use orqwa_storage::worlds::{Building, Chunks};
use orqwa_world::{SnowGroundMask, TerrainChunk, World};
use tracing::warn;

/// The winter ground's mask of a world, as far as it is made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WinterMask<'a> {
    /// No world is loaded, or the mask could not be made.
    None,
    /// Being made in the background, or waiting for the world to be complete.
    Pending,
    /// Made.
    Made(&'a SnowGroundMask),
}

/// A world's chunks, by index.
#[derive(Debug)]
pub struct WorldChunks {
    /// The chunks on disk, readable once their centres are known: the store the world is
    /// built into, or its kept world.
    stored: OnceLock<Chunks>,
    /// The chunks' centres, where no store could be made.
    centres: OnceLock<Vec<[f32; 3]>>,
    /// The store chunks are written to while the world is built. Let go of once it is
    /// complete, so a kept world it was packed into is ridden on and the store goes.
    store: Mutex<Option<Arc<Building>>>,
    /// Whether the store failed, so chunks finished since are held in memory (ADR 0040:
    /// desktops only rely on it).
    failed: AtomicBool,
    /// The chunks no store holds, by index.
    memory: Mutex<Vec<Option<TerrainChunk>>>,
    /// Which chunks are finished, by index.
    finished: Mutex<Vec<bool>>,
    /// Whether every chunk is finished.
    complete: AtomicBool,
    /// The winter ground's mask, made in the background once it is first asked for and the
    /// world is complete: it needs every chunk. `None` if making it failed.
    snow: OnceLock<Option<SnowGroundMask>>,
    /// Whether the mask was asked for.
    asked: AtomicBool,
}

impl WorldChunks {
    /// Chunks read from a kept world.
    pub(crate) fn kept(chunks: Chunks) -> Self {
        let this = Self::building(None);
        let _ = this.stored.set(chunks);
        this.complete.store(true, Ordering::Relaxed);
        this
    }

    /// The chunks of a world about to be built into `store`, or into memory without one.
    pub(crate) fn building(store: Option<Arc<Building>>) -> Self {
        Self {
            stored: OnceLock::new(),
            centres: OnceLock::new(),
            store: Mutex::new(store),
            failed: AtomicBool::new(false),
            memory: Mutex::new(Vec::new()),
            finished: Mutex::new(Vec::new()),
            complete: AtomicBool::new(false),
            snow: OnceLock::new(),
            asked: AtomicBool::new(false),
        }
    }

    /// Notes the chunks a stopped build of the world wrote into the store, by index, as
    /// finished: the build taking it up finishes only the others (ADR 0040, 2d step 4b).
    pub(crate) fn taken_up(&self, written: &[bool]) {
        for (index, _) in written.iter().enumerate().filter(|(_, written)| **written) {
            set(&self.finished, index, true);
        }
    }

    /// Takes chunk `index`, just finished. Called on the cores that finish chunks.
    pub(crate) fn take(&self, index: usize, chunk: TerrainChunk) {
        let store = lock(&self.store).clone();
        let stored = store
            .filter(|_| !self.failed.load(Ordering::Relaxed))
            .is_some_and(|store| match store.write(index, &chunk) {
                Ok(()) => true,
                Err(error) => {
                    if !self.failed.swap(true, Ordering::Relaxed) {
                        warn!(%error, "cannot store the world being built; holding it in memory");
                    }
                    false
                }
            });
        if !stored {
            set(&self.memory, index, Some(chunk));
        }
        set(&self.finished, index, true);
    }

    /// Notes where the chunks lie, by index, once the rider's surroundings are finished:
    /// from then on they can be read.
    pub(crate) fn locate(&self, centres: &[[f32; 3]]) {
        match lock(&self.store).as_ref() {
            Some(store) => {
                let _ = self
                    .stored
                    .set(Chunks::built(Arc::clone(store), centres.to_vec()));
            }
            None => {
                let _ = self.centres.set(centres.to_vec());
            }
        }
    }

    /// Notes that every chunk is finished. True if the winter ground's mask was asked for
    /// meanwhile, so it is to be made now.
    pub(crate) fn completed(&self) -> bool {
        self.complete.store(true, Ordering::Relaxed);
        lock(&self.store).take();
        self.asked.load(Ordering::Relaxed)
    }

    /// Whether every chunk is finished.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.complete.load(Ordering::Relaxed)
    }

    /// Whether chunk `index` is finished, so it can be read.
    #[must_use]
    pub fn finished(&self, index: usize) -> bool {
        index < self.len()
            && (self.complete() || lock(&self.finished).get(index).copied().unwrap_or(false))
    }

    /// How many chunks are finished.
    #[must_use]
    pub fn finished_count(&self) -> usize {
        if self.complete() {
            return self.len();
        }
        lock(&self.finished).iter().filter(|&&done| done).count()
    }

    /// How many chunks the world has: none until their centres are known.
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
        match (self.stored.get(), self.centres.get()) {
            (Some(stored), _) => stored.centres(),
            (None, Some(centres)) => centres,
            (None, None) => &[],
        }
    }

    /// Chunk `index`, read from disk unless held. `None` past the last chunk, for one not
    /// finished yet, and for one that cannot be read, e.g. the system cleared the cache: it
    /// stays undrawn, and a kept world it is missing from is built again on the course's next
    /// ride.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Cow<'_, TerrainChunk>> {
        if !self.finished(index) {
            return None;
        }
        if let Some(chunk) = lock(&self.memory).get(index).cloned().flatten() {
            return Some(Cow::Owned(chunk));
        }
        self.stored
            .get()?
            .read(index)
            .inspect_err(|error| warn!(%error, index, "cannot read a chunk of the world"))
            .ok()
            .map(Cow::Owned)
    }

    /// The mask that keeps the winter ground's snow off the ways, once
    /// [made](Self::make_snow_ground_mask).
    pub(crate) fn snow_ground_mask(&self) -> WinterMask<'_> {
        match self.snow.get() {
            None => WinterMask::Pending,
            Some(None) => WinterMask::None,
            Some(Some(mask)) => WinterMask::Made(mask),
        }
    }

    /// Asks for the mask: true for the first who asks once the world is complete, who is to
    /// make it. Asked before, it is to be made once the world is [completed](Self::completed).
    pub(crate) fn ask_snow_ground_mask(&self) -> bool {
        !self.asked.swap(true, Ordering::Relaxed) && self.complete()
    }

    /// Makes the mask that keeps the winter ground's snow off the ways of the world whose
    /// parts built whole are `world`, from every chunk: seconds for a long, densely mapped
    /// course. A chunk that cannot be read is undrawn, so the mask leaves it out.
    pub(crate) fn make_snow_ground_mask(&self, world: &World) {
        // A mask that cannot be made is recorded as such, or it would be waited for all ride.
        self.snow.get_or_init(|| {
            std::panic::catch_unwind(AssertUnwindSafe(|| self.mask(world)))
                .inspect_err(|_| warn!("cannot make the winter ground's mask"))
                .ok()
        });
    }

    fn mask(&self, world: &World) -> SnowGroundMask {
        let mask = Mutex::new(SnowGroundMask::around(world, self.centres()));
        match self.stored.get() {
            Some(stored) if !self.failed.load(Ordering::Relaxed) => {
                let cleared = stored.read_each(&|_, chunk| {
                    lock(&mask).clear_chunk(&chunk);
                });
                if let Err(error) = cleared {
                    warn!(%error, "cannot read all of the world for its winter ground");
                }
            }
            _ => {
                for index in 0..self.len() {
                    if let Some(chunk) = self.get(index) {
                        lock(&mask).clear_chunk(&chunk);
                    }
                }
            }
        }
        mask.into_inner().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps the world as the course with `fingerprint`'s among `kept`, its parts built whole
    /// `world` and by the generator of version `generator`, once it is complete. Blocks for
    /// seconds.
    pub(crate) fn keep(
        &self,
        kept: &orqwa_storage::worlds::KeptWorlds,
        fingerprint: &str,
        world: &World,
        generator: u32,
    ) {
        if !self.complete() {
            warn!("cannot keep the course's world before it is complete");
            return;
        }
        let kept = match self.stored.get() {
            Some(stored) if !self.failed.load(Ordering::Relaxed) => {
                stored.keep(kept, fingerprint, world, generator)
            }
            _ => {
                let Some(chunks) = (0..self.len())
                    .map(|index| self.get(index).map(Cow::into_owned))
                    .collect::<Option<Vec<_>>>()
                else {
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

/// Sets item `index` of `items`, growing it as needed: chunks are finished in no set order.
fn set<T: Default + Clone>(items: &Mutex<Vec<T>>, index: usize, item: T) {
    let mut items = lock(items);
    if items.len() <= index {
        items.resize(index + 1, T::default());
    }
    items[index] = item;
}
