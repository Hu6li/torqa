//! Updating an unpacked Linux release in place (R268, ADR 0023): the new release's tarball is
//! unpacked beside ORQWA's folder, the two folders are swapped and the new ORQWA is started.
//! Linux lets a running program's folder be renamed, so ORQWA need not quit first; the new one
//! starts once it has, and removes the old folder.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::UpdateError;

/// The extension every release folder holds beside the program.
const EXTENSION: &str = "liborqwa_gd.so";

/// The folder every release tarball unpacks to.
const RELEASE_FOLDER: &str = "ORQWA";

/// An unpacked release: the program `app` in `folder`, which lies in `parent`.
struct Layout {
    parent: PathBuf,
    folder: PathBuf,
    /// The folder's name, whatever the rider called it.
    name: String,
    /// The program's file name, `ORQWA.x86_64` or `ORQWA.arm64`.
    program: String,
}

impl Layout {
    /// The release `app` runs from; none when started from the editor or moved out of it.
    fn of(app: &Path) -> Option<Self> {
        let folder = app.parent()?;
        let parent = folder.parent()?;
        let name = folder.file_name()?.to_str()?;
        let program = app.file_name()?.to_str()?;
        if !app.is_file() || !folder.join(EXTENSION).is_file() {
            return None;
        }
        Some(Self {
            parent: parent.to_owned(),
            folder: folder.to_owned(),
            name: name.to_owned(),
            program: program.to_owned(),
        })
    }

    /// Where the new release is unpacked, beside the folder so a rename moves it at once.
    fn staging(&self) -> PathBuf {
        self.parent.join(format!(".{}-update", self.name))
    }

    /// Where the replaced release waits to be removed at the next start.
    fn old(&self) -> PathBuf {
        self.parent.join(format!(".{}-old", self.name))
    }
}

/// Whether ORQWA at `app` can replace its own folder: an unpacked release (the program beside
/// its extension) in a folder the rider may change.
pub(crate) fn can_install(app: &Path) -> bool {
    let Some(layout) = Layout::of(app) else {
        return false;
    };
    // Swapping needs both folders' entries in the parent changed. Trying is the only check
    // that also holds for ACLs, read-only mounts and root.
    let probe = layout
        .parent
        .join(format!(".{}-probe-{}", layout.name, std::process::id()));
    if std::fs::create_dir(&probe).is_err() {
        return false;
    }
    let _ = std::fs::remove_dir(&probe);
    true
}

/// Puts the release in `tarball` in place of ORQWA at `app` and starts it there; the caller
/// quits at once. Until the swap, any failure leaves ORQWA as it was.
pub(crate) fn install(tarball: &Path, app: &Path) -> Result<(), UpdateError> {
    install_after(tarball, app, std::process::id())
}

/// [`install`], the new ORQWA starting once the process `running` has exited.
fn install_after(tarball: &Path, app: &Path, running: u32) -> Result<(), UpdateError> {
    let failed =
        |what: &str, e: &dyn std::fmt::Display| UpdateError::Install(format!("{what}: {e}"));
    let layout = Layout::of(app).ok_or_else(|| failed("no release folder", &app.display()))?;
    let staging = layout.staging();
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging).map_err(|e| failed("cannot make room for it", &e))?;
    let staged = unpack(tarball, &staging, &layout.program).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&staging);
    })?;
    let old = layout.old();
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(&layout.folder, &old).map_err(|e| {
        let _ = std::fs::remove_dir_all(&staging);
        failed("cannot move ORQWA aside", &e)
    })?;
    if let Err(e) = std::fs::rename(&staged, &layout.folder) {
        let _ = std::fs::rename(&old, &layout.folder);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(failed("cannot put the new ORQWA in place", &e));
    }
    let _ = std::fs::remove_dir(&staging);
    let _ = std::fs::remove_file(tarball);
    // Started at once, the new ORQWA would share the rider's files and the trainer's connection
    // with the one still quitting. The update is in place either way: a rider whose ORQWA does
    // not start again opens it as usual.
    let start = Command::new("sh")
        .arg("-c")
        .arg("while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$0\"")
        .arg(layout.folder.join(&layout.program))
        .arg(running.to_string())
        .spawn();
    if let Err(error) = start {
        tracing::warn!(%error, "updated, but cannot start ORQWA again");
    }
    Ok(())
}

/// Unpacks `tarball` into the empty `staging` folder with the system's `tar`, and returns the
/// release folder in it once it holds a runnable `program` and the extension.
fn unpack(tarball: &Path, staging: &Path, program: &str) -> Result<PathBuf, UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(tarball)
        .arg("--no-same-owner")
        .arg("-C")
        .arg(staging)
        .output()
        .map_err(|e| UpdateError::Install(format!("cannot run tar: {e}")))?;
    if !output.status.success() {
        return Err(UpdateError::Install(format!(
            "cannot unpack the update: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let staged = staging.join(RELEASE_FOLDER);
    let runnable = std::fs::metadata(staged.join(program))
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if !runnable || !staged.join(EXTENSION).is_file() {
        return Err(UpdateError::Install(format!(
            "the update holds no {program} beside {EXTENSION}"
        )));
    }
    Ok(staged)
}

/// Removes what an update left beside ORQWA at `app`: the replaced release and an unfinished
/// unpacking. Only beside a release folder, and only the names an update gives them.
pub(crate) fn clean_up(app: &Path) {
    let Some(layout) = Layout::of(app) else {
        return;
    };
    for leftover in [layout.old(), layout.staging()] {
        if leftover.exists()
            && let Err(error) = std::fs::remove_dir_all(&leftover)
        {
            tracing::warn!(%error, path = %leftover.display(), "cannot remove an old update");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    use super::*;

    /// A fresh folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orqwa-linux-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A release folder at `folder`: a program that leaves `started` beside itself when run,
    /// the extension, and a file telling which `release` it is.
    fn release(folder: &Path, release: &str, extension: bool) {
        std::fs::create_dir_all(folder).unwrap();
        let program = folder.join("ORQWA.x86_64");
        std::fs::write(&program, "#!/bin/sh\ntouch \"$(dirname \"$0\")/started\"\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        if extension {
            std::fs::write(folder.join(EXTENSION), "so").unwrap();
        }
        std::fs::write(folder.join("release"), release).unwrap();
    }

    /// A release tarball in `dir`, packed as CI packs it (`tar -czf … -C dist ORQWA`).
    fn tarball(dir: &Path, version: &str, extension: bool) -> PathBuf {
        let source = dir.join(format!("build-{version}"));
        release(&source.join(RELEASE_FOLDER), version, extension);
        let path = dir.join(format!("ORQWA-{version}.tar.gz"));
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&path)
            .arg("-C")
            .arg(&source)
            .arg(RELEASE_FOLDER)
            .status()
            .unwrap();
        assert!(status.success());
        path
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn the_new_release_takes_the_folder_and_starts_there() {
        let dir = scratch("swap");
        // Riders may unpack and name the folder as they like; it keeps its name.
        let folder = dir.join("apps/orqwa-bike");
        release(&folder, "0.5.0", true);
        let update = tarball(&dir, "0.6.0", true);
        let app = folder.join("ORQWA.x86_64");

        // Stands in for the ORQWA that is quitting; unreaped, it still counts as running.
        let mut quitting = Command::new("true").spawn().unwrap();

        install_after(&update, &app, quitting.id()).unwrap();

        assert_eq!(read(&folder.join("release")), "0.6.0");
        assert!(!update.exists(), "the tarball is no longer needed");
        assert!(!dir.join("apps/.orqwa-bike-update").exists());
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !folder.join("started").exists(),
            "not started while the old ORQWA still runs"
        );
        quitting.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !folder.join("started").exists() {
            assert!(Instant::now() < deadline, "the new ORQWA was not started");
            std::thread::sleep(Duration::from_millis(20));
        }

        let old = dir.join("apps/.orqwa-bike-old");
        assert_eq!(
            read(&old.join("release")),
            "0.5.0",
            "kept until the next start"
        );
        clean_up(&app);
        assert!(!old.exists());
        assert_eq!(read(&folder.join("release")), "0.6.0");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_broken_update_leaves_orqwa_as_it_was() {
        let dir = scratch("broken");
        let folder = dir.join("ORQWA");
        release(&folder, "0.5.0", true);
        let app = folder.join("ORQWA.x86_64");
        let incomplete = tarball(&dir, "0.6.0", false);
        let garbage = dir.join("garbage.tar.gz");
        std::fs::write(&garbage, "no tarball").unwrap();

        for update in [incomplete, garbage] {
            let result = install(&update, &app);

            assert!(matches!(result, Err(UpdateError::Install(_))), "{result:?}");
            assert_eq!(read(&folder.join("release")), "0.5.0");
            assert!(!folder.join("started").exists(), "nothing started");
            assert!(!dir.join(".ORQWA-update").exists());
            assert!(!dir.join(".ORQWA-old").exists());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_an_unpacked_release_updates_itself() {
        let dir = scratch("layout");
        let folder = dir.join("ORQWA");
        release(&folder, "0.5.0", false);
        let app = folder.join("ORQWA.x86_64");

        assert!(!can_install(&app));
        std::fs::write(folder.join(EXTENSION), "so").unwrap();
        assert!(can_install(&app));
        assert!(
            std::fs::read_dir(&dir).unwrap().count() == 1,
            "the check leaves nothing behind"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_the_rider_cannot_change_is_not_replaced() {
        let dir = scratch("read-only");
        let parent = dir.join("opt");
        release(&parent.join("ORQWA"), "0.5.0", true);
        let app = parent.join("ORQWA/ORQWA.x86_64");
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = can_install(&app);
        let writable_anyway = std::fs::create_dir(parent.join("probe")).is_ok();

        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        if writable_anyway {
            // Root ignores the permissions, so the folder is changeable after all.
            eprintln!("permissions do not hold for this user; checked that the probe agrees");
            assert!(result);
        } else {
            assert!(!result);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clean_up_touches_only_what_an_update_left() {
        let dir = scratch("clean");
        let folder = dir.join("ORQWA");
        release(&folder, "0.6.0", true);
        for kept in ["ORQWA-old", ".other-old", "rides"] {
            std::fs::create_dir(dir.join(kept)).unwrap();
        }
        std::fs::create_dir(dir.join(".ORQWA-old")).unwrap();
        std::fs::create_dir(dir.join(".ORQWA-update")).unwrap();

        clean_up(&folder.join("ORQWA.x86_64"));

        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, [".other-old", "ORQWA", "ORQWA-old", "rides"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
