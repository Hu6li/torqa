//! Bookkeeping of downloaded files, so a prepared course can take exactly its own data along.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// A shared, thread-safe set of file paths that data providers record when they read or write a
/// cached file. Clones share the same set.
#[derive(Debug, Clone, Default)]
pub struct UsedFiles(Arc<Mutex<BTreeSet<PathBuf>>>);

impl UsedFiles {
    /// Records that `path` was used.
    pub fn record(&self, path: &Path) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_owned());
    }

    /// All recorded paths, sorted.
    #[must_use]
    pub fn paths(&self) -> Vec<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_one_sorted_set_without_duplicates() {
        let used = UsedFiles::default();
        let provider = used.clone();

        provider.record(Path::new("b"));
        provider.record(Path::new("a"));
        used.record(Path::new("b"));

        assert_eq!(used.paths(), [PathBuf::from("a"), PathBuf::from("b")]);
    }
}
