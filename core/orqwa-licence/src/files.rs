//! Files only their owner may read, written whole: the maintainers' keys and certificates.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use crate::{hex, random};

/// Writes a file only its owner may read, in full or not at all, replacing one there.
///
/// # Errors
/// If writing or renaming fails.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = write_partial(path, bytes)?;
    fs::rename(&partial, path).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })
}

/// Writes a new file only its owner may read, in full or not at all, never replacing one:
/// [`std::io::ErrorKind::AlreadyExists`] if `path` exists, also when another writer got there
/// first.
///
/// # Errors
/// As above, or if writing fails.
pub fn write_new_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = write_partial(path, bytes)?;
    // A hard link never replaces: of two writers, exactly one succeeds.
    let linked = fs::hard_link(&partial, path);
    let _ = fs::remove_file(&partial);
    linked
}

fn write_partial(path: &Path, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let unique = random::<8>().map_err(std::io::Error::other)?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.part", hex(&unique)));
    let partial = path.with_file_name(name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&partial).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    match written {
        Ok(()) => Ok(partial),
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("orqwa-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_new_file_is_never_written_over() {
        let dir = dir("new");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        write_new_private(&path, b"first").unwrap();
        let again = write_new_private(&path, b"second").unwrap_err();
        assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no partial files left"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
