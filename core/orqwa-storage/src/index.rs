//! The history index (ADR 0002, R79, #262): a SQLite cache of the rides' metadata in the
//! platform's cache directory, so the history and the analysis across rides read one file
//! instead of the JSON next to every FIT activity: hundreds of small files for a regular rider,
//! which a sync client may fetch on demand (R30). The ride files stay the source of truth: the
//! index knows each metadata file by its size and modification time, reads one again when that
//! changed (a rename, an upload, a rebuild), forgets rides whose files are gone, and is built
//! afresh when it is missing or from another version of ORQWA. What it has read in this
//! session it keeps in memory, so a second look costs one `stat` per ride.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, params};

use crate::rides::{self, RideRecord};

/// The index file's name, in the cache directory.
pub const FILE: &str = "history.sqlite";
/// The index's layout; an index of another layout is thrown away and built afresh.
const SCHEMA: i64 = 1;

/// The index could not be read or written; the ride files are still there to read.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// The database refused.
    #[error("history index: {0}")]
    Database(#[from] rusqlite::Error),
    /// Its directory could not be made.
    #[error("history index: {0}")]
    Io(#[from] std::io::Error),
}

/// A metadata file as the index last read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileState {
    /// Modification time in nanoseconds since the epoch.
    modified: i64,
    size: i64,
}

/// The rides' metadata, read once and kept.
pub struct HistoryIndex {
    connection: Connection,
    /// What this session has read, by FIT file: a second look costs a `stat` per ride.
    memory: HashMap<PathBuf, (FileState, RideRecord)>,
}

impl std::fmt::Debug for HistoryIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryIndex")
            .field("known", &self.memory.len())
            .finish_non_exhaustive()
    }
}

impl HistoryIndex {
    /// Opens the index at `file`, making it if it is missing or of another layout.
    ///
    /// # Errors
    /// [`IndexError`] if the file cannot be opened or made.
    pub fn open(file: &Path) -> Result<Self, IndexError> {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(file)?;
        // Readers never wait for a writer, and a crash costs at most the last writes: the
        // index is a cache, the ride files have everything.
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        let schema: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if schema != SCHEMA {
            connection.execute_batch("DROP TABLE IF EXISTS rides")?;
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS rides (
                path TEXT PRIMARY KEY,
                modified INTEGER NOT NULL,
                size INTEGER NOT NULL,
                record TEXT NOT NULL
            )",
        )?;
        connection.pragma_update(None, "user_version", SCHEMA)?;
        Ok(Self {
            connection,
            memory: HashMap::new(),
        })
    }

    /// The rides in `dir`, newest first, each with its FIT file. A ride whose metadata the
    /// index knows as it is now comes from the index; the others `load` reads from their files
    /// (rebuilding or completing the metadata as it sees fit: what is on disk afterwards is
    /// what gets indexed), a ride it cannot read left out and asked for again next time.
    /// Rides whose files are gone leave the index.
    ///
    /// # Errors
    /// [`IndexError`] if the index cannot be read or written; the ride files are unaffected.
    pub fn records<F>(
        &mut self,
        dir: &Path,
        mut load: F,
    ) -> Result<Vec<(PathBuf, RideRecord)>, IndexError>
    where
        F: FnMut(&Path) -> Option<RideRecord>,
    {
        let fits = rides::fit_files(dir);
        let mut records = Vec::with_capacity(fits.len());
        let mut changed = Vec::new();
        for fit in fits {
            let metadata = rides::metadata_path(&fit);
            if let Some(state) = file_state(&metadata) {
                if let Some((known, record)) = self.memory.get(&fit)
                    && *known == state
                {
                    records.push((fit, record.clone()));
                    continue;
                }
                if let Some(record) = self.stored(&fit, state)? {
                    self.memory.insert(fit.clone(), (state, record.clone()));
                    records.push((fit, record));
                    continue;
                }
            }
            let Some(record) = load(&fit) else {
                continue;
            };
            if let Some(state) = file_state(&metadata) {
                self.memory.insert(fit.clone(), (state, record.clone()));
                changed.push((fit.clone(), state, record.clone()));
            }
            records.push((fit, record));
        }
        let present: HashSet<&Path> = records.iter().map(|(fit, _)| fit.as_path()).collect();
        self.store(dir, &changed, &present)?;
        rides::newest_first(&mut records);
        Ok(records)
    }

    /// The record the index holds for `fit`, if its metadata file is still as indexed. A
    /// record this ORQWA cannot read (a newer format) counts as unknown.
    fn stored(&self, fit: &Path, state: FileState) -> Result<Option<RideRecord>, IndexError> {
        let text: Option<String> = self
            .connection
            .query_row(
                "SELECT record FROM rides WHERE path = ?1 AND modified = ?2 AND size = ?3",
                params![key(fit), state.modified, state.size],
                |row| row.get(0),
            )
            .optional()?;
        Ok(text.and_then(|text| rides::from_json(&text).ok()))
    }

    /// Writes what was read anew and forgets the rides of `dir` whose files are gone.
    fn store(
        &mut self,
        dir: &Path,
        changed: &[(PathBuf, FileState, RideRecord)],
        present: &HashSet<&Path>,
    ) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        for (fit, state, record) in changed {
            let Ok(text) = rides::to_json(record) else {
                continue;
            };
            transaction.execute(
                "INSERT INTO rides (path, modified, size, record) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(path) DO UPDATE SET
                    modified = excluded.modified, size = excluded.size, record = excluded.record",
                params![key(fit), state.modified, state.size, text],
            )?;
        }
        let mut prefix = key(dir);
        if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
            prefix.push(std::path::MAIN_SEPARATOR);
        }
        let gone: Vec<String> = transaction
            .prepare("SELECT path FROM rides WHERE substr(path, 1, length(?1)) = ?1")?
            .query_map(params![prefix], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter(|path| !present.contains(Path::new(path)))
            .collect();
        for path in &gone {
            transaction.execute("DELETE FROM rides WHERE path = ?1", params![path])?;
            self.memory.remove(Path::new(path));
        }
        transaction.commit()?;
        Ok(())
    }
}

/// How the index names a file: its path as text.
fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A file's size and modification time, `None` if it is not there.
fn file_state(path: &Path) -> Option<FileState> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_nanos()).ok())?;
    Some(FileState {
        modified,
        size: i64::try_from(metadata.len()).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::time::{Duration, SystemTime};

    use orqwa_domain::recording::RideSummary;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orqwa-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("rides")).unwrap();
        dir
    }

    fn record(route: &str, start: u64) -> RideRecord {
        RideRecord {
            route: route.to_owned(),
            start: SystemTime::UNIX_EPOCH + Duration::from_secs(start),
            summary: RideSummary::default(),
            route_key: None,
            route_time: None,
            climbs: Vec::new(),
            name: None,
            ftp_estimate: None,
            rider_mass: None,
            simulated: false,
            assisted: false,
            analysis: None,
            uploads: BTreeMap::new(),
        }
    }

    /// A ride on disk: a FIT file (its bytes do not matter here) with its metadata.
    fn ride(dir: &Path, name: &str, record: &RideRecord) -> PathBuf {
        let fit = dir.join("rides").join(format!("{name}.fit"));
        std::fs::write(&fit, b"fit").unwrap();
        rides::save(&fit, record).unwrap();
        fit
    }

    /// Reads a ride's metadata, counting the reads.
    fn counting(reads: &Cell<usize>) -> impl FnMut(&Path) -> Option<RideRecord> + '_ {
        move |fit| {
            reads.set(reads.get() + 1);
            rides::load(fit).ok()
        }
    }

    #[test]
    fn rides_are_read_once_and_come_from_the_index_from_then_on() {
        let dir = temp_dir("once");
        let file = dir.join("cache").join(FILE);
        let gurten = ride(
            &dir,
            "orqwa-20260301-100000",
            &record("Gurten", 1_700_000_000),
        );
        let moron = ride(
            &dir,
            "orqwa-20260302-100000",
            &record("Moron", 1_700_100_000),
        );
        let reads = Cell::new(0);

        let mut index = HistoryIndex::open(&file).unwrap();
        let first = index.records(&dir.join("rides"), counting(&reads)).unwrap();
        let again = index.records(&dir.join("rides"), counting(&reads)).unwrap();

        assert_eq!(reads.get(), 2, "each file once");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].0, moron, "newest first");
        assert_eq!(first[0].1.route, "Moron");
        assert_eq!(first[1].0, gurten);
        assert_eq!(again, first);

        // Another session: the index file serves, nothing is read.
        drop(index);
        let mut index = HistoryIndex::open(&file).unwrap();
        let later = index.records(&dir.join("rides"), counting(&reads)).unwrap();
        assert_eq!(reads.get(), 2);
        assert_eq!(later, first);
    }

    #[test]
    fn rides_kept_from_torqa_stay_behind_newer_ones() {
        let dir = temp_dir("renamed");
        let file = dir.join("cache").join(FILE);
        let torqa = ride(
            &dir,
            "torqa-20261012-071500",
            &record("Gurten", 1_791_789_300),
        );
        let orqwa = ride(
            &dir,
            "orqwa-20261024-071500",
            &record("Moron", 1_792_826_100),
        );
        let reads = Cell::new(0);

        let mut index = HistoryIndex::open(&file).unwrap();
        let read = index.records(&dir.join("rides"), counting(&reads)).unwrap();
        let indexed = index.records(&dir.join("rides"), counting(&reads)).unwrap();

        let fits: Vec<&PathBuf> = read.iter().map(|(fit, _)| fit).collect();
        assert_eq!(fits, [&orqwa, &torqa]);
        assert_eq!(indexed, read);
    }

    #[test]
    fn a_changed_file_is_read_again_and_a_gone_one_forgotten() {
        let dir = temp_dir("changes");
        let file = dir.join("cache").join(FILE);
        let gurten = ride(
            &dir,
            "orqwa-20260301-100000",
            &record("Gurten", 1_700_000_000),
        );
        let moron = ride(
            &dir,
            "orqwa-20260302-100000",
            &record("Moron", 1_700_100_000),
        );
        let reads = Cell::new(0);
        let mut index = HistoryIndex::open(&file).unwrap();
        index.records(&dir.join("rides"), counting(&reads)).unwrap();

        // A rename rewrites the metadata (a different size here; a different time in life).
        let mut renamed = record("Gurten", 1_700_000_000);
        renamed.name = Some("Lunch spin".to_owned());
        rides::save(&gurten, &renamed).unwrap();
        std::fs::remove_file(&moron).unwrap();
        std::fs::remove_file(rides::metadata_path(&moron)).unwrap();

        let records = index.records(&dir.join("rides"), counting(&reads)).unwrap();

        assert_eq!(reads.get(), 3, "the changed file once more");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].1.name.as_deref(), Some("Lunch spin"));
        let rows: i64 = index
            .connection
            .query_row("SELECT count(*) FROM rides", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1, "the gone ride left the index");
    }

    #[test]
    fn a_ride_that_cannot_be_read_is_left_out_and_asked_for_again() {
        let dir = temp_dir("unreadable");
        let file = dir.join("cache").join(FILE);
        let fit = dir.join("rides").join("orqwa-20260301-100000.fit");
        std::fs::write(&fit, b"fit").unwrap();
        let reads = Cell::new(0);
        let mut index = HistoryIndex::open(&file).unwrap();

        let none = index.records(&dir.join("rides"), counting(&reads)).unwrap();
        assert!(none.is_empty(), "no metadata, and the reader made none");

        // A reader that rebuilds the metadata: what it wrote is indexed as it is.
        let rebuilt = index
            .records(&dir.join("rides"), |fit| {
                reads.set(reads.get() + 1);
                let record = record("Rebuilt", 1_700_000_000);
                rides::save(fit, &record).unwrap();
                Some(record)
            })
            .unwrap();
        let served = index.records(&dir.join("rides"), counting(&reads)).unwrap();

        assert_eq!(
            reads.get(),
            2,
            "asked again until it could be read, then no more"
        );
        assert_eq!(rebuilt.len(), 1);
        assert_eq!(served, rebuilt);
    }

    #[test]
    fn an_index_of_another_layout_is_built_afresh() {
        let dir = temp_dir("layout");
        let file = dir.join("cache").join(FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        {
            let connection = Connection::open(&file).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE rides (path TEXT PRIMARY KEY); PRAGMA user_version = 99;",
                )
                .unwrap();
        }
        ride(
            &dir,
            "orqwa-20260301-100000",
            &record("Gurten", 1_700_000_000),
        );
        let reads = Cell::new(0);

        let mut index = HistoryIndex::open(&file).unwrap();
        let records = index.records(&dir.join("rides"), counting(&reads)).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(reads.get(), 1);
    }
}
