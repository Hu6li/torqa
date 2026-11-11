//! The 3D worlds a computer keeps of its courses (R101, ADR 0025): built once, then read on
//! **Ride** in a second or two instead of being built for half a minute.
//!
//! Layout, in the cache directory, so nothing of it syncs with the data:
//!
//! ```text
//! worlds/<fingerprint>.world   one zip per course: `whole`, the parts built whole, and
//!                              `chunks/<n>` for each chunk, deflated
//! worlds/<name>.<write>.building/chunks/<n>
//!                              a world being built and ridden (ADR 0040): each chunk once
//!                              finished, as a zip of its one entry `chunks/<n>`, copied into
//!                              the `.world` as it is once the world is complete and kept
//! ```
//!
//! Every world is built into a store, kept or not, and ridden from it or from its kept world:
//! its [chunks](Chunks) are read one at a time, never all held (ADR 0040).
//!
//! A course's world is found by the [fingerprint](crate::course::fingerprint) of what it is
//! built from. It stays kept across updates of ORQWA: one of another
//! [format](orqwa_world::stored::FORMAT) cannot be read and is built again, one of an older
//! [generator](orqwa_world::GENERATOR) is ridden as it is until the rider has it built again.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use orqwa_world::{TerrainChunk, World, stored};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// The directory of kept worlds, in the cache directory.
pub const DIR: &str = "worlds";
const EXTENSION: &str = "world";
const BUILDING: &str = "building";
const WHOLE: &str = "whole";
const CHUNKS: &str = "chunks/";
/// The file in a store that its build holds locked as long as the store is in use, which tells
/// any ORQWA running that it is (ADR 0040).
const LOCK: &str = "lock";
/// The quickest deflate: a third of the size, at a fraction of the time the next levels take.
const LEVEL: i64 = 1;
/// A half-written world, or a world being built that has not grown for this long, was left by
/// a write or a build that never finished.
const ABANDONED: Duration = Duration::from_secs(600);
/// Numbers the writes, so two of the same world at once do not write into one file.
static WRITES: AtomicU64 = AtomicU64::new(0);
/// The stores of this ORQWA's builds, never tidied however long they stay unchanged: a world
/// is ridden from its store long after its last chunk was written.
static LIVE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Whether `path` is the store of one of this ORQWA's builds.
fn ours(path: &Path) -> bool {
    LIVE.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .any(|dir| dir == path)
}

/// Whether the world being written or built at `path` is still in use: a store this or
/// another ORQWA running holds, or a write or build that went on lately. A store is ridden
/// from for hours after its last chunk was written, so its age alone tells nothing; its lock
/// is taken just after it is made, so a young one counts too.
fn in_use(path: &Path) -> bool {
    let unfinished = path
        .extension()
        .is_some_and(|e| e == "part" || e == BUILDING);
    let held = || match File::open(path.join(LOCK)) {
        Ok(lock) => matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
        Err(_) => false,
    };
    let recent = || {
        [path.to_owned(), path.join(CHUNKS)].iter().any(|path| {
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < ABANDONED))
        })
    };
    unfinished && (ours(path) || held() || recent())
}

/// The worlds this computer keeps.
#[derive(Debug, Clone)]
pub struct KeptWorlds {
    root: PathBuf,
}

impl KeptWorlds {
    /// The worlds kept under `cache_dir`.
    #[must_use]
    pub fn new(cache_dir: &Path) -> Self {
        Self {
            root: cache_dir.join(DIR),
        }
    }

    /// Where the world of the course with `fingerprint` is kept, or would be.
    #[must_use]
    pub fn file(&self, fingerprint: &str) -> PathBuf {
        self.root.join(format!("{fingerprint}.{EXTENSION}"))
    }

    /// Whether the world of the course with `fingerprint` is kept.
    #[must_use]
    pub fn has(&self, fingerprint: &str) -> bool {
        self.file(fingerprint).is_file()
    }

    /// How many bytes the kept world of the course with `fingerprint` takes on disk; `None` if
    /// none is kept.
    #[must_use]
    pub fn size_of(&self, fingerprint: &str) -> Option<u64> {
        std::fs::metadata(self.file(fingerprint))
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map(|m| m.len())
    }

    /// The version of the [generator](orqwa_world::GENERATOR) that built the kept world of the
    /// course with `fingerprint`; `None` if none is kept that this ORQWA can read.
    #[must_use]
    pub fn generator(&self, fingerprint: &str) -> Option<u32> {
        let file = File::open(self.file(fingerprint)).ok()?;
        let mut zip = ZipArchive::new(BufReader::new(file)).ok()?;
        let whole = zip.by_name(WHOLE).ok()?;
        stored::read_generator(&mut BufReader::new(whole)).ok()
    }

    /// The kept world of the course with `fingerprint`: its parts built whole, the version of
    /// the generator that built it, and its chunks to read when needed. `None` if there is
    /// none. Only each chunk's centre is read now.
    ///
    /// # Errors
    /// On I/O errors, and if the file is damaged or of another
    /// [format](orqwa_world::stored::FORMAT).
    pub fn open(&self, fingerprint: &str) -> io::Result<Option<(World, u32, Chunks)>> {
        let path = self.file(fingerprint);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut zip = ZipArchive::new(BufReader::new(file))?;
        let (world, generator) = stored::read_whole(&mut BufReader::new(zip.by_name(WHOLE)?))?;
        let centres = (0..zip.len().saturating_sub(1))
            .map(|index| stored::read_center(&mut zip.by_name(&format!("{CHUNKS}{index}"))?))
            .collect::<io::Result<_>>()?;
        let chunks = Chunks {
            centres,
            source: Mutex::new(Source::Kept { file: path, zip }),
            damaged: AtomicBool::new(false),
        };
        Ok(Some((world, generator, chunks)))
    }

    /// Keeps `world`, built by the [generator](orqwa_world::GENERATOR) of version
    /// `generator`, as that of the course with `fingerprint`, in place of one kept before. The
    /// file appears whole or not at all.
    ///
    /// # Errors
    /// On I/O errors.
    pub fn write(&self, fingerprint: &str, world: &World, generator: u32) -> io::Result<()> {
        self.write_parts(fingerprint, world, &world.chunks, generator)
    }

    /// Keeps a world as [`write`](Self::write) does, of `whole`'s parts built whole and
    /// `chunks`.
    ///
    /// # Errors
    /// On I/O errors.
    pub fn write_parts(
        &self,
        fingerprint: &str,
        whole: &World,
        chunks: &[TerrainChunk],
        generator: u32,
    ) -> io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        written_whole(&self.file(fingerprint), |partial| {
            write_to(partial, whole, chunks, generator)
        })
    }

    /// A store to build a world into, chunk by chunk, to ride it from and to
    /// [keep](Chunks::keep) it from once complete (ADR 0040), named after its course's
    /// fingerprint, or anything else for a world of no course yet. Each build has its own, so
    /// one stopped never deletes another's chunks.
    ///
    /// # Errors
    /// On I/O errors.
    pub fn building(&self, name: &str) -> io::Result<Building> {
        std::fs::create_dir_all(&self.root)?;
        // Process ids repeat, after a crash and across sandboxes: a directory already there is
        // another build's, live or left behind.
        let dir = loop {
            let dir = self
                .root
                .join(format!("{name}.{}.{BUILDING}", write_name()));
            match std::fs::create_dir(&dir) {
                Ok(()) => break dir,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        };
        LIVE.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(dir.clone());
        let mut store = Building {
            dir,
            lock: None,
            failed: AtomicBool::new(false),
        };
        let lock = File::create(store.dir.join(LOCK))?;
        // Some filesystems have no file locks (network shares, FUSE mounts): that costs only
        // the protection from another ORQWA's tidy, which this build's own and the store's
        // age still give for a while, not the store.
        store.lock = lock.lock().is_ok().then_some(lock);
        std::fs::create_dir(store.dir.join(CHUNKS))?;
        Ok(store)
    }

    /// Drops the kept world of the course with `fingerprint`, if there is one.
    ///
    /// # Errors
    /// On I/O errors.
    pub fn remove(&self, fingerprint: &str) -> io::Result<()> {
        remove(&self.file(fingerprint))
    }

    /// Drops the worlds of courses not among `fingerprints`, e.g. the library's: a course
    /// deleted or replaced, also on another computer sharing the data. Worlds this ORQWA cannot
    /// read go too: they would only be built again.
    ///
    /// # Errors
    /// On I/O errors.
    pub fn keep_only(&self, fingerprints: &HashSet<String>) -> io::Result<()> {
        for entry in read_dir(&self.root)? {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let live = name
                .strip_suffix(&format!(".{EXTENSION}"))
                .is_some_and(|fingerprint| {
                    fingerprints.contains(fingerprint) && self.generator(fingerprint).is_some()
                });
            if !live && !in_use(&path) {
                remove(&path)?;
            }
        }
        Ok(())
    }

    /// Drops every kept world. The stores in use stay, this ORQWA's and another's running:
    /// worlds are ridden from them.
    ///
    /// # Errors
    /// On I/O errors.
    pub fn clear(&self) -> io::Result<()> {
        for entry in read_dir(&self.root)? {
            if !in_use(&entry.path()) {
                remove(&entry.path())?;
            }
        }
        Ok(())
    }

    /// How many bytes the kept worlds take on disk, without the worlds this ORQWA is building
    /// or riding that are not kept.
    #[must_use]
    pub fn size(&self) -> u64 {
        fn size_of(path: &Path) -> u64 {
            match std::fs::symlink_metadata(path) {
                Ok(m) if m.is_dir() => read_dir(path)
                    .unwrap_or_default()
                    .iter()
                    .map(|entry| size_of(&entry.path()))
                    .sum(),
                Ok(m) => m.len(),
                Err(_) => 0,
            }
        }
        read_dir(&self.root)
            .unwrap_or_default()
            .iter()
            .map(std::fs::DirEntry::path)
            .filter(|path| !ours(path))
            .map(|path| size_of(&path))
            .sum()
    }
}

/// A world being built into the cache, chunk by chunk, so that its chunks need not all be held
/// (ADR 0040). A chunk file in it is whole, also after a build was stopped: each is written
/// aside and renamed. The store is deleted when dropped, also once kept.
#[derive(Debug)]
pub struct Building {
    dir: PathBuf,
    /// Held locked while the store is in use.
    lock: Option<File>,
    /// Whether a chunk could not be written, so the store cannot become the world.
    failed: AtomicBool,
}

impl Building {
    /// Whether the store is locked, so that no other ORQWA running tidies it away (ADR 0040).
    /// Not on a filesystem without file locks: another ORQWA then leaves it only while it is
    /// young.
    #[must_use]
    pub fn locked(&self) -> bool {
        self.lock.is_some()
    }

    /// Writes chunk `index` of the world, deflated as a kept world's chunks are. Chunks may be
    /// written in any order, from several threads at once.
    ///
    /// # Errors
    /// On I/O errors; the store then cannot be [kept](Chunks::keep). The chunks written before
    /// stay readable: they may be the only copy.
    pub fn write(&self, index: usize, chunk: &TerrainChunk) -> io::Result<()> {
        let path = self.chunk(index);
        let partial = path.with_extension("part");
        let written = write_entry(&partial, &format!("{CHUNKS}{index}"), |out| {
            stored::write_chunk(chunk, out)
        })
        .and_then(|()| std::fs::rename(&partial, &path));
        if written.is_err() {
            self.failed.store(true, Ordering::Relaxed);
            // The disk may be full: what the chunk took of it is given back.
            let _ = std::fs::remove_file(&partial);
        }
        written
    }

    /// Reads chunk `index` back.
    ///
    /// # Errors
    /// On I/O errors, if the chunk was not written, and if its file is damaged.
    pub fn read(&self, index: usize) -> io::Result<TerrainChunk> {
        let mut zip = ZipArchive::new(BufReader::new(File::open(self.chunk(index))?))?;
        stored::read_chunk(&mut BufReader::new(zip.by_index(0)?))
    }

    /// Which chunks are written, by index, up to the last written: those a build stopped
    /// before had finished, for the next build of the same world to take up (ADR 0040, 2d step
    /// 4b). A chunk half-written when its build stopped is not.
    ///
    /// # Errors
    /// On I/O errors.
    pub fn written(&self) -> io::Result<Vec<bool>> {
        let mut written = Vec::new();
        for entry in read_dir(&self.dir.join(CHUNKS))? {
            let name = entry.file_name();
            if let Some(index) = name.to_str().and_then(|name| name.parse::<usize>().ok()) {
                if written.len() <= index {
                    written.resize(index + 1, false);
                }
                written[index] = true;
            }
        }
        Ok(written)
    }

    /// Whether no chunk failed to be written so far.
    #[must_use]
    pub fn whole(&self) -> bool {
        !self.failed.load(Ordering::Relaxed)
    }

    /// Packs the world built into the kept world `file`, in place of one kept before:
    /// `whole`'s parts built whole, by the [generator](orqwa_world::GENERATOR) of version
    /// `generator`, and the first `chunks` chunks written here. The chunks are copied as they
    /// are, in index order, neither inflated nor deflated again. The file appears whole or not
    /// at all.
    fn pack(&self, file: &Path, whole: &World, generator: u32, chunks: usize) -> io::Result<()> {
        if !self.whole() {
            return Err(io::Error::other("a chunk of the world was not written"));
        }
        packed(file, whole, generator, chunks, |zip, index| {
            let mut chunk = ZipArchive::new(BufReader::new(File::open(self.chunk(index))?))?;
            zip.raw_copy_file(chunk.by_index_raw(0)?)
        })
    }

    fn chunk(&self, index: usize) -> PathBuf {
        self.dir.join(CHUNKS).join(index.to_string())
    }
}

impl Drop for Building {
    fn drop(&mut self) {
        drop(self.lock.take());
        let _ = remove(&self.dir);
        LIVE.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|dir| *dir != self.dir);
    }
}

/// A world's chunks on disk, read one at a time while it is ridden (ADR 0040): from the store
/// it was built into, or from its kept world.
#[derive(Debug)]
pub struct Chunks {
    centres: Vec<[f32; 3]>,
    source: Mutex<Source>,
    /// Whether a chunk was found damaged, so these chunks are never kept.
    damaged: AtomicBool,
}

#[derive(Debug)]
enum Source {
    Building(Arc<Building>),
    Kept {
        file: PathBuf,
        zip: ZipArchive<BufReader<File>>,
    },
}

impl Chunks {
    /// The chunks built into `store`, centred at `centres` by index. The store may still be
    /// written to: a chunk can be read once it is written.
    #[must_use]
    pub fn built(store: Arc<Building>, centres: Vec<[f32; 3]>) -> Self {
        Self {
            centres,
            source: Mutex::new(Source::Building(store)),
            damaged: AtomicBool::new(false),
        }
    }

    /// How many chunks the world has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.centres.len()
    }

    /// Whether the world has no chunks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.centres.is_empty()
    }

    /// The chunks' centres, by index.
    #[must_use]
    pub fn centres(&self) -> &[[f32; 3]] {
        &self.centres
    }

    /// Reads chunk `index`. A kept world a chunk of which is damaged is deleted, so that the
    /// course's next ride builds it again.
    ///
    /// # Errors
    /// On I/O errors, if `index` is past the last chunk, and if the chunk is missing or
    /// damaged, e.g. the system cleared the cache.
    pub fn read(&self, index: usize) -> io::Result<TerrainChunk> {
        if index >= self.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no such chunk"));
        }
        let mut source = self.source.lock().unwrap_or_else(PoisonError::into_inner);
        let (read, kept) = match &mut *source {
            Source::Building(store) => (store.read(index), None),
            Source::Kept { file, zip } => (read_kept(zip, index), Some(file.as_path())),
        };
        self.check(read, kept)
    }

    /// `read`, noting a chunk found damaged, and deleting the kept world `kept` it was read
    /// from, if any.
    fn check(
        &self,
        read: io::Result<TerrainChunk>,
        kept: Option<&Path>,
    ) -> io::Result<TerrainChunk> {
        if let Err(error) = &read
            && damage(error)
        {
            self.damaged.store(true, Ordering::Relaxed);
            if let Some(file) = kept {
                let _ = remove(file);
            }
        }
        read
    }

    /// Reads every chunk on all cores and gives each to `each` with its index, in no set
    /// order, holding no more than one per core. A chunk that cannot be read is passed over,
    /// as by [`read`](Self::read), and the rest are still read.
    ///
    /// # Errors
    /// The first error a chunk could not be read for, once all others were read.
    pub fn read_each(&self, each: &(dyn Fn(usize, TerrainChunk) + Sync)) -> io::Result<()> {
        // Each core reads through its own handle, where the file is still there to open.
        let from = match &*self.source.lock().unwrap_or_else(PoisonError::into_inner) {
            Source::Building(store) => Reading::Store(Arc::clone(store)),
            Source::Kept { file, .. } => Reading::Kept(file.clone()),
        };
        let count = self.len();
        let threads = std::thread::available_parallelism()
            .map_or(1, NonZeroUsize::get)
            .min(count)
            .max(1);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|first| {
                    let from = &from;
                    scope.spawn(move || -> io::Result<()> {
                        let mut own = match from {
                            Reading::Kept(file) => File::open(file)
                                .ok()
                                .and_then(|file| ZipArchive::new(BufReader::new(file)).ok()),
                            Reading::Store(_) => None,
                        };
                        let mut failed = Ok(());
                        for index in (first..count).step_by(threads) {
                            let read = match (&mut own, from) {
                                (Some(zip), Reading::Kept(file)) => {
                                    self.check(read_kept(zip, index), Some(file))
                                }
                                (_, Reading::Store(store)) => self.check(store.read(index), None),
                                // Deleted since it was opened: through the handle still open.
                                (None, Reading::Kept(_)) => self.read(index),
                            };
                            match read {
                                Ok(chunk) => each(index, chunk),
                                Err(error) => failed = failed.and(Err(error)),
                            }
                        }
                        failed
                    })
                })
                .collect();
            let failed: Vec<_> = workers
                .into_iter()
                .map(|worker| {
                    worker
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect();
            failed.into_iter().collect()
        })
    }

    /// Keeps the world as that of the course with `fingerprint` among `kept`: `whole`'s parts
    /// built whole, by the [generator](orqwa_world::GENERATOR) of version `generator`, and
    /// these chunks. Built into a store, its chunks are copied from it as they are, then read
    /// from the kept world, and the store is deleted once no chunk is being read from it. A
    /// world read from a kept one is kept already. Blocks for seconds.
    ///
    /// # Errors
    /// On I/O errors, if a chunk is missing or failed to be written, and if a chunk was found
    /// damaged; the chunks are then still read from where they were.
    pub fn keep(
        &self,
        kept: &KeptWorlds,
        fingerprint: &str,
        whole: &World,
        generator: u32,
    ) -> io::Result<()> {
        if self.damaged.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a chunk of the world is damaged",
            ));
        }
        std::fs::create_dir_all(&kept.root)?;
        let file = kept.file(fingerprint);
        let mut source = self.source.lock().unwrap_or_else(PoisonError::into_inner);
        let store = match &mut *source {
            Source::Building(store) => Arc::clone(store),
            Source::Kept { file: read, .. } if *read == file && file.is_file() => return Ok(()),
            Source::Kept { zip, .. } => {
                // Deleted since, e.g. keeping worlds was turned off and on again: copied from
                // the file still open. Chunks wait meanwhile, also on Godot's main thread; it
                // takes seconds, and only when keeping is turned on again during a ride.
                packed(&file, whole, generator, self.len(), |out, index| {
                    let name = format!("{CHUNKS}{index}");
                    let at = zip
                        .index_for_name(&name)
                        .ok_or(zip::result::ZipError::FileNotFound)?;
                    out.raw_copy_file(zip.by_index_raw(at)?)
                })?;
                let zip = ZipArchive::new(BufReader::new(File::open(&file)?))?;
                *source = Source::Kept { file, zip };
                return Ok(());
            }
        };
        drop(source);
        // Packed while the ride reads on from the store.
        store.pack(&file, whole, generator, self.len())?;
        let zip = ZipArchive::new(BufReader::new(File::open(&file)?))?;
        let replaced = std::mem::replace(
            &mut *self.source.lock().unwrap_or_else(PoisonError::into_inner),
            Source::Kept { file, zip },
        );
        // The store is deleted here, unless a `read_each` still reads from it: not while a
        // chunk is waited for.
        drop((replaced, store));
        Ok(())
    }
}

/// Where [`Chunks::read_each`]'s cores read from.
enum Reading {
    Store(Arc<Building>),
    Kept(PathBuf),
}

/// Whether `error` means a chunk is damaged or missing, not that it could not be read for now.
/// A broken deflate stream is `InvalidInput` (flate2); a chunk past the last is refused before
/// it is read, so that kind means damage here too.
fn damage(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::InvalidData
            | io::ErrorKind::InvalidInput
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::NotFound
    )
}

fn read_kept(zip: &mut ZipArchive<BufReader<File>>, index: usize) -> io::Result<TerrainChunk> {
    let entry = zip.by_name(&format!("{CHUNKS}{index}"))?;
    stored::read_chunk(&mut BufReader::new(entry))
}

/// A name no other write of this ORQWA, or of another running, uses.
fn write_name() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    )
}

/// Makes the file at `path` with `write`, which writes it to the path it is given, so that it
/// appears whole or not at all.
fn written_whole(path: &Path, write: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    let partial = path.with_extension(format!("{EXTENSION}.{}.part", write_name()));
    let written = write(&partial).and_then(|()| std::fs::rename(&partial, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

/// Makes the kept world `file`, so that it appears whole or not at all: `whole`'s parts built
/// whole, by the [generator](orqwa_world::GENERATOR) of version `generator`, and `chunks`
/// chunks, each copied in index order by `copy`, deflated as they are.
fn packed(
    file: &Path,
    whole: &World,
    generator: u32,
    chunks: usize,
    mut copy: impl FnMut(&mut ZipWriter<BufWriter<File>>, usize) -> zip::result::ZipResult<()>,
) -> io::Result<()> {
    written_whole(file, |partial| {
        let mut zip = ZipWriter::new(BufWriter::new(File::create(partial)?));
        zip.start_file(WHOLE, options())?;
        let mut out = BufWriter::new(&mut zip);
        stored::write_whole(whole, generator, &mut out)?;
        out.flush()?;
        drop(out);
        for index in 0..chunks {
            copy(&mut zip, index)?;
        }
        finish(zip)
    })
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(LEVEL))
}

/// Writes a zip at `path` of one entry, `name`, which `write` writes.
fn write_entry(
    path: &Path,
    name: &str,
    write: impl FnOnce(&mut BufWriter<&mut ZipWriter<BufWriter<File>>>) -> io::Result<()>,
) -> io::Result<()> {
    let mut zip = ZipWriter::new(BufWriter::new(File::create(path)?));
    zip.start_file(name, options())?;
    let mut out = BufWriter::new(&mut zip);
    write(&mut out)?;
    out.flush()?;
    drop(out);
    // Not synced: a store never outlives its build, and the page cache outlives a process
    // killed. Only the kept world it becomes is brought to the disk.
    zip.finish()?
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .flush()
}

fn write_to(path: &Path, world: &World, chunks: &[TerrainChunk], generator: u32) -> io::Result<()> {
    let mut zip = ZipWriter::new(BufWriter::new(File::create(path)?));
    let mut entry = |name: String, write: &dyn Fn(&mut dyn Write) -> io::Result<()>| {
        zip.start_file(name, options())?;
        let mut out = BufWriter::new(&mut zip);
        write(&mut out)?;
        out.flush()
    };
    entry(WHOLE.to_owned(), &|mut out| {
        stored::write_whole(world, generator, &mut out)
    })?;
    for (index, chunk) in chunks.iter().enumerate() {
        entry(format!("{CHUNKS}{index}"), &|mut out| {
            stored::write_chunk(chunk, &mut out)
        })?;
    }
    finish(zip)
}

/// Finishes `zip` and brings its file to the disk.
fn finish(zip: ZipWriter<BufWriter<File>>) -> io::Result<()> {
    let mut file = zip
        .finish()?
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?;
    file.flush()?;
    file.sync_all()
}

/// The entries of directory `path`; none if it does not exist.
fn read_dir(path: &Path) -> io::Result<Vec<std::fs::DirEntry>> {
    match std::fs::read_dir(path) {
        Ok(entries) => entries.collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Removes the file or directory at `path`, if there is one.
fn remove(path: &Path) -> io::Result<()> {
    let removed = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match removed {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use orqwa_world::MeshData;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orqwa-worlds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mesh(seed: f32) -> MeshData {
        MeshData {
            vertices: vec![[seed, 0.0, 1.0]; 3],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            indices: vec![0, 1, 2],
            ..MeshData::default()
        }
    }

    /// A world of `chunks` chunks, each told apart by its centre.
    #[allow(clippy::cast_precision_loss)] // small test numbers
    fn world(chunks: usize) -> World {
        World {
            chunks: (0..chunks)
                .map(|i| TerrainChunk {
                    center: [i as f32, 0.0, 0.0],
                    mesh: mesh(i as f32),
                    buildings: MeshData::default(),
                    modelled: Vec::new(),
                    streets: MeshData::default(),
                    tracks: MeshData::default(),
                    urban: MeshData::default(),
                    water: MeshData::default(),
                    trees: orqwa_world::Trees::default(),
                    props: orqwa_world::Props::default(),
                })
                .collect(),
            road: mesh(-1.0),
            fallback_samples: 3,
            ..World::default()
        }
    }

    impl KeptWorlds {
        /// The kept world of the course with `fingerprint`, its chunks read one by one as a
        /// ride reads them.
        fn read(&self, fingerprint: &str) -> io::Result<Option<(World, u32)>> {
            let Some((mut world, generator, chunks)) = self.open(fingerprint)? else {
                return Ok(None);
            };
            world.chunks = (0..chunks.len())
                .map(|index| chunks.read(index))
                .collect::<io::Result<_>>()?;
            let centres: Vec<_> = world.chunks.iter().map(|chunk| chunk.center).collect();
            assert_eq!(chunks.centres(), centres, "indexed as read");
            Ok(Some((world, generator)))
        }
    }

    /// `world` without its chunks, and its chunks' centres.
    fn parts(world: &World) -> (World, Vec<[f32; 3]>) {
        let whole = World {
            chunks: Vec::new(),
            ..world.clone()
        };
        (
            whole,
            world.chunks.iter().map(|chunk| chunk.center).collect(),
        )
    }

    #[test]
    fn a_kept_world_reads_back_whole_and_in_order() {
        let cache = temp_dir("kept");
        let kept = KeptWorlds::new(&cache);
        assert!(!kept.has("f1"));
        assert_eq!(kept.read("f1").unwrap(), None);
        assert_eq!(kept.generator("f1"), None);
        assert_eq!(kept.size_of("f1"), None);
        // More chunks than cores, so that every worker reads several.
        let built = world(100);
        kept.write("f1", &built, 3).unwrap();
        assert!(kept.has("f1"));
        assert_eq!(kept.read("f1").unwrap(), Some((built, 3)));
        assert_eq!(
            kept.generator("f1"),
            Some(3),
            "told without reading the world"
        );
        // Another course's world is its own; a world kept again replaces the first.
        kept.write("f2", &world(0), 3).unwrap();
        kept.write("f1", &world(2), 4).unwrap();
        assert_eq!(kept.read("f1").unwrap(), Some((world(2), 4)));
        assert_eq!(kept.generator("f1"), Some(4));
        assert_eq!(kept.read("f2").unwrap(), Some((world(0), 3)));
        // Each world's own room, for the settings to list (R136), adds up to the total.
        let (f1, f2) = (kept.size_of("f1").unwrap(), kept.size_of("f2").unwrap());
        assert!(f1 > f2, "the world with more chunks takes more");
        assert_eq!(kept.size(), f1 + f2);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_damaged_world_is_an_error_not_a_world() {
        let cache = temp_dir("damaged");
        let kept = KeptWorlds::new(&cache);
        kept.write("f", &world(5), 1).unwrap();
        let file = kept.file("f");
        let bytes = std::fs::read(&file).unwrap();
        std::fs::write(&file, &bytes[..bytes.len() / 2]).unwrap();
        assert!(kept.read("f").is_err());
        std::fs::write(&file, b"not a zip").unwrap();
        assert!(kept.read("f").is_err());
        assert_eq!(kept.generator("f"), None);
        std::fs::remove_dir_all(cache).unwrap();
    }

    /// Sets when directory `dir` last changed.
    fn age(dir: &Path, at: std::time::SystemTime) {
        #[cfg(unix)]
        let handle = File::open(dir);
        // A directory opens only for backup on Windows.
        #[cfg(windows)]
        let handle = {
            use std::os::windows::fs::OpenOptionsExt;
            File::options()
                .write(true)
                .custom_flags(0x0200_0000)
                .open(dir)
        };
        handle.unwrap().set_modified(at).unwrap();
    }

    /// The entries of `dir`, by name.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = read_dir(dir)
            .unwrap()
            .iter()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_kept_world_is_ridden_one_chunk_at_a_time() {
        let cache = temp_dir("open");
        let kept = KeptWorlds::new(&cache);
        assert!(kept.open("f").unwrap().is_none());
        let built = world(30);
        kept.write("f", &built, 3).unwrap();
        let (whole, generator, chunks) = kept.open("f").unwrap().expect("kept");
        let (parts, centres) = parts(&built);
        assert_eq!(
            (whole, generator),
            (parts.clone(), 3),
            "the parts built whole"
        );
        assert_eq!(chunks.centres(), centres, "indexed without being read");
        assert_eq!(chunks.read(29).unwrap(), built.chunks[29]);
        assert_eq!(chunks.read(0).unwrap(), built.chunks[0]);
        // Every chunk once, on all cores.
        let read = Mutex::new(vec![None; 30]);
        chunks
            .read_each(&|index, chunk| {
                let mut read = read.lock().unwrap();
                assert!(read[index].is_none(), "chunk {index} read twice");
                read[index] = Some(chunk);
            })
            .unwrap();
        let read: Vec<_> = read.into_inner().unwrap().into_iter().flatten().collect();
        assert_eq!(read, built.chunks);
        assert!(chunks.read(30).is_err());
        assert!(kept.has("f"), "no chunk past the last is no damage");
        // Deleted while ridden, e.g. keeping worlds turned off: ridden on, and kept again
        // from the file still open once turned on.
        kept.clear().unwrap();
        assert_eq!(chunks.read(3).unwrap(), built.chunks[3]);
        let count = AtomicU64::new(0);
        chunks
            .read_each(&|_, _| {
                count.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();
        assert_eq!(
            count.into_inner(),
            30,
            "all read through the file still open"
        );
        chunks.keep(&kept, "f", &parts, 3).unwrap();
        assert_eq!(kept.read("f").unwrap(), Some((built.clone(), 3)));
        assert_eq!(chunks.read(4).unwrap(), built.chunks[4]);
        drop(chunks);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_kept_world_a_chunk_cannot_be_read_from_is_built_again() {
        let cache = temp_dir("open-damaged");
        let kept = KeptWorlds::new(&cache);
        let built = world(2);
        // Chunk 1's centre reads, the rest of it is cut off.
        std::fs::create_dir_all(&kept.root).unwrap();
        let mut zip = ZipWriter::new(BufWriter::new(File::create(kept.file("f")).unwrap()));
        zip.start_file(WHOLE, options()).unwrap();
        stored::write_whole(&built, 1, &mut zip).unwrap();
        for index in 0..2 {
            let mut bytes = Vec::new();
            stored::write_chunk(&built.chunks[index], &mut bytes).unwrap();
            if index == 1 {
                bytes.truncate(20);
            }
            zip.start_file(format!("{CHUNKS}{index}"), options())
                .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        finish(zip).unwrap();
        let (whole, _, chunks) = kept.open("f").unwrap().expect("kept");
        let read = Mutex::new(Vec::new());
        let each = chunks.read_each(&|index, _| read.lock().unwrap().push(index));
        assert!(each.is_err());
        assert_eq!(*read.lock().unwrap(), [0], "the others are still read");
        assert!(!kept.has("f"), "damaged, so the next ride builds it again");
        assert_eq!(chunks.read(0).unwrap(), built.chunks[0]);
        assert!(chunks.read(1).is_err());
        assert!(
            chunks.keep(&kept, "f", &whole, 1).is_err(),
            "never kept again as it is"
        );
        assert!(!kept.has("f"));
        drop(chunks);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_kept_world_whose_compressed_chunk_is_corrupt_is_built_again() {
        let cache = temp_dir("corrupt");
        let kept = KeptWorlds::new(&cache);
        let built = world(3);
        // Chunk 1's deflate stream starts with a stored block holding the centre, which opening
        // the world reads, then breaks off into a block of the reserved type: the stream itself
        // is corrupt, not only its checksum.
        let mut chunk = Vec::new();
        stored::write_chunk(&built.chunks[1], &mut chunk).unwrap();
        let mut stream = vec![0b000];
        stream.extend(32u16.to_le_bytes());
        stream.extend((!32u16).to_le_bytes());
        stream.extend(&chunk[..32]);
        stream.push(0b111);
        std::fs::create_dir_all(&kept.root).unwrap();
        let file = kept.file("f");
        let mut zip = ZipWriter::new(BufWriter::new(File::create(&file).unwrap()));
        zip.start_file(WHOLE, options()).unwrap();
        stored::write_whole(&built, 1, &mut zip).unwrap();
        for index in 0..3 {
            if index == 1 {
                let raw =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
                zip.start_file(format!("{CHUNKS}1"), raw).unwrap();
                zip.write_all(&stream).unwrap();
            } else {
                zip.start_file(format!("{CHUNKS}{index}"), options())
                    .unwrap();
                stored::write_chunk(&built.chunks[index], &mut zip).unwrap();
            }
        }
        finish(zip).unwrap();
        // Then marked deflated, in its local header and in the central directory.
        let mut bytes = std::fs::read(&file).unwrap();
        let name = format!("{CHUNKS}1").into_bytes();
        let names: Vec<_> = (0..bytes.len() - name.len())
            .filter(|&at| bytes[at..].starts_with(&name))
            .collect();
        for at in names {
            let method = if at >= 30 && bytes[at - 30..].starts_with(b"PK") {
                at - 30 + 8
            } else if at >= 46 && bytes[at - 46..].starts_with(b"PK") {
                at - 46 + 10
            } else {
                continue;
            };
            bytes[method..method + 2].copy_from_slice(&8u16.to_le_bytes());
        }
        std::fs::write(&file, bytes).unwrap();
        let (whole, _, chunks) = kept.open("f").unwrap().expect("kept");
        let read = Mutex::new(Vec::new());
        let each = chunks.read_each(&|index, _| read.lock().unwrap().push(index));
        assert!(each.is_err());
        let mut read = read.into_inner().unwrap();
        read.sort_unstable();
        assert_eq!(read, [0, 2], "the others are still read");
        assert!(!kept.has("f"), "damaged, so the next ride builds it again");
        assert!(
            chunks.keep(&kept, "f", &whole, 1).is_err(),
            "never kept again as it is"
        );
        drop(chunks);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_store_tells_which_chunks_a_stopped_build_finished_and_is_kept_once_the_rest_are() {
        let cache = temp_dir("taken-up");
        let kept = KeptWorlds::new(&cache);
        let built = world(8);
        let store = kept.building("f").unwrap();
        for index in [0, 2, 5] {
            store.write(index, &built.chunks[index]).unwrap();
        }
        // Stopped while writing chunk 3.
        std::fs::write(store.chunk(3).with_extension("part"), b"half").unwrap();
        let written = store.written().unwrap();
        assert_eq!(written, [true, false, true, false, false, true]);

        // The next build writes the rest into the same store.
        for index in (0..8).filter(|&i| !written.get(i).is_some_and(|&w| w)) {
            store.write(index, &built.chunks[index]).unwrap();
        }
        let (whole, centres) = parts(&built);
        let chunks = Chunks::built(Arc::new(store), centres);
        chunks.keep(&kept, "f", &whole, 3).unwrap();
        assert_eq!(kept.read("f").unwrap(), Some((built, 3)));
        drop(chunks);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_world_built_into_a_store_is_ridden_from_it_and_kept_as_one_written_whole() {
        let cache = temp_dir("store");
        let kept = KeptWorlds::new(&cache);
        let built = world(40);
        let store = kept.building("f").unwrap();
        // Chunks are finished in no set order, several at once (ADR 0040).
        std::thread::scope(|scope| {
            for half in [0, 1] {
                let (store, built) = (&store, &built);
                scope.spawn(move || {
                    for index in (0..40).rev().filter(|i| i % 2 == half) {
                        store.write(index, &built.chunks[index]).unwrap();
                    }
                });
            }
        });
        assert!(store.whole());
        assert_eq!(store.read(7).unwrap(), built.chunks[7], "readable at once");
        let (whole, centres) = parts(&built);
        let chunks = Chunks::built(Arc::new(store), centres.clone());
        assert_eq!(chunks.read(8).unwrap(), built.chunks[8]);
        assert!(!kept.has("f"), "kept only once asked");
        chunks.keep(&kept, "f", &whole, 3).unwrap();
        assert_eq!(kept.read("f").unwrap(), Some((built.clone(), 3)));
        assert_eq!(kept.generator("f"), Some(3));
        assert_eq!(names(&kept.root), ["f.world"], "the store is gone");
        // Ridden on from the kept world.
        assert_eq!(chunks.centres(), centres);
        assert_eq!(chunks.read(39).unwrap(), built.chunks[39]);
        chunks.keep(&kept, "f", &whole, 3).unwrap();
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_store_missing_a_chunk_is_never_kept() {
        let cache = temp_dir("store-missing");
        let kept = KeptWorlds::new(&cache);
        kept.write("f", &world(1), 1).unwrap();
        let before = std::fs::read(kept.file("f")).unwrap();
        let built = world(3);
        let store = kept.building("f").unwrap();
        for index in [0, 2] {
            store.write(index, &built.chunks[index]).unwrap();
        }
        let chunks = Chunks::built(Arc::new(store), parts(&built).1);
        assert!(chunks.keep(&kept, "f", &World::default(), 2).is_err());
        assert_eq!(
            std::fs::read(kept.file("f")).unwrap(),
            before,
            "the world kept before stays"
        );
        assert_eq!(chunks.read(2).unwrap(), built.chunks[2], "still ridden");
        drop(chunks);
        assert_eq!(names(&kept.root), ["f.world"]);

        // A store a chunk could not be written into is never kept. The chunks written before
        // stay: they may be the only copy.
        let store = kept.building("f").unwrap();
        store.write(1, &built.chunks[1]).unwrap();
        std::fs::create_dir(store.chunk(0)).unwrap();
        assert!(store.write(0, &built.chunks[0]).is_err());
        assert!(!store.whole());
        assert_eq!(store.read(1).unwrap(), built.chunks[1]);
        assert!(
            !store.chunk(0).with_extension("part").exists(),
            "room given back"
        );
        std::fs::remove_dir(store.chunk(0)).unwrap();
        store.write(0, &built.chunks[0]).unwrap();
        let chunks = Chunks::built(Arc::new(store), parts(&world(2)).1);
        assert!(chunks.keep(&kept, "f", &World::default(), 2).is_err());
        assert_eq!(std::fs::read(kept.file("f")).unwrap(), before);
        drop(chunks);
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_build_never_takes_a_store_already_there() {
        let cache = temp_dir("store-taken");
        let kept = KeptWorlds::new(&cache);
        // Left by an ORQWA that had this process's id, e.g. before a crash, or in another
        // sandbox: the next names this process would take.
        let next = WRITES.load(Ordering::Relaxed);
        for write in next..next + 64 {
            let taken = kept.root.join(format!(
                "f.{}-{write}.{BUILDING}/{CHUNKS}",
                std::process::id()
            ));
            std::fs::create_dir_all(&taken).unwrap();
            std::fs::write(taken.join("0"), b"theirs").unwrap();
        }
        let store = kept.building("f").unwrap();
        assert!(
            !store.chunk(0).exists(),
            "its own store, not another's: {store:?}"
        );
        drop(store);
        std::fs::remove_dir_all(cache).unwrap();
    }

    /// A store left by an ORQWA that stopped, e.g. killed for memory, is deleted by the next
    /// tidy, while this ORQWA's own stay however long ago they were written: worlds are ridden
    /// from them.
    #[test]
    fn a_store_left_long_ago_is_deleted() {
        let cache = temp_dir("store-left");
        let kept = KeptWorlds::new(&cache);
        let live = kept.building("f").unwrap();
        live.write(0, &world(1).chunks[0]).unwrap();
        let left = kept.root.join(format!("f.1-0.{BUILDING}"));
        std::fs::create_dir_all(left.join(CHUNKS)).unwrap();
        std::fs::write(left.join(CHUNKS).join("0"), b"chunk").unwrap();
        let long_ago = std::time::SystemTime::now() - 2 * ABANDONED;
        for dir in [&left, &live.dir] {
            for dir in [dir.clone(), dir.join(CHUNKS)] {
                age(&dir, long_ago);
            }
        }
        kept.keep_only(&HashSet::from(["f".to_owned()])).unwrap();
        assert!(!left.exists(), "the store left is gone");
        assert!(live.read(0).is_ok(), "this ORQWA's own stays");
        std::fs::create_dir_all(&left).unwrap();
        age(&left, long_ago);
        kept.clear().unwrap();
        assert!(!left.exists());
        assert!(
            live.read(0).is_ok(),
            "also once keeping worlds is turned off"
        );
        assert_eq!(kept.size(), 0, "a world not kept is not counted as kept");
        drop(live);
        std::fs::remove_dir_all(cache).unwrap();
    }

    /// A store another ORQWA running rides from is never tidied, however long ago it was
    /// written: its lock tells.
    #[test]
    fn a_store_another_orqwa_holds_stays() {
        let cache = temp_dir("store-held");
        let kept = KeptWorlds::new(&cache);
        // Another process's store, which this one does not know.
        let theirs = kept.root.join(format!("ride.1-0.{BUILDING}"));
        std::fs::create_dir_all(theirs.join(CHUNKS)).unwrap();
        std::fs::write(theirs.join(CHUNKS).join("0"), b"chunk").unwrap();
        let lock = File::create(theirs.join(LOCK)).unwrap();
        lock.lock().unwrap();
        let long_ago = std::time::SystemTime::now() - 2 * ABANDONED;
        for dir in [theirs.clone(), theirs.join(CHUNKS)] {
            age(&dir, long_ago);
        }
        kept.keep_only(&HashSet::new()).unwrap();
        kept.clear().unwrap();
        assert!(theirs.exists(), "held, so in use");
        // That ORQWA stopped: its lock went with it.
        drop(lock);
        kept.keep_only(&HashSet::new()).unwrap();
        assert!(!theirs.exists(), "left long ago");
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_store_dropped_before_its_world_is_complete_leaves_nothing() {
        let cache = temp_dir("store-dropped");
        let kept = KeptWorlds::new(&cache);
        let built = world(2);
        // Two builds of one world at once, e.g. one stopped while the next began.
        let (first, second) = (kept.building("f").unwrap(), kept.building("f").unwrap());
        first.write(0, &built.chunks[0]).unwrap();
        second.write(0, &built.chunks[0]).unwrap();
        drop(first);
        assert_eq!(
            second.read(0).unwrap(),
            built.chunks[0],
            "each build has its own"
        );
        drop(second);
        assert_eq!(names(&kept.root), Vec::<String>::new());
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn only_the_worlds_of_courses_still_there_are_kept() {
        let cache = temp_dir("sweep");
        let kept = KeptWorlds::new(&cache);
        for fingerprint in ["f1", "f2", "f3", "f5"] {
            kept.write(fingerprint, &world(1), 1).unwrap();
        }
        let writing = kept.file("f4").with_extension("world.part");
        std::fs::write(&writing, b"half").unwrap();
        // A world being built, of a course that is not yet or no longer in the library.
        let building = kept.building("f6").unwrap();
        building.write(0, &world(1).chunks[0]).unwrap();
        // One this ORQWA cannot read, e.g. of another format: it would only be built again.
        std::fs::write(kept.file("f5"), b"unreadable").unwrap();

        // f2's course was deleted.
        let library = ["f1", "f3", "f5"].map(ToOwned::to_owned);
        kept.keep_only(&HashSet::from(library)).unwrap();
        assert!(kept.has("f1") && kept.has("f3"));
        assert!(!kept.has("f2"));
        assert!(!kept.has("f5"), "the unreadable one is gone");
        assert!(writing.exists(), "a world being written stays");
        assert!(building.read(0).is_ok(), "a world being built stays");
        drop(building);

        kept.clear().unwrap();
        assert!(!kept.has("f1"));
        assert!(
            writing.exists(),
            "another ORQWA's write goes on, also when cleared"
        );
        assert_eq!(kept.size(), 4, "only that is left");
        std::fs::remove_file(&writing).unwrap();
        // Clearing nothing is no error.
        kept.clear().unwrap();
        kept.keep_only(&HashSet::new()).unwrap();
        std::fs::remove_dir_all(cache).unwrap();
    }
}
