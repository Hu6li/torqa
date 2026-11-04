//! Temporary library migration for #501. Remove this module and its startup call once riders
//! have updated; the course archive itself is unchanged.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;

use orqwa_storage::course::EXTENSION;

pub(crate) fn run(library: &Path) {
    let entries = match fs::read_dir(library) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::warn!(%error, "cannot migrate the course library");
            return;
        }
    };
    for entry in entries {
        let result = entry.and_then(|entry| {
            if entry.file_type()?.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("tqc"))
            {
                migrate(&entry.path())?;
            }
            Ok(())
        });
        if let Err(error) = result {
            tracing::warn!(%error, "cannot migrate a course; leaving its original file for retry");
        }
    }
}

fn migrate(source: &Path) -> io::Result<()> {
    let mut target = source.with_extension(EXTENSION);
    let mut suffix = 2;
    loop {
        // Linking publishes a complete file without overwriting a course a sync client or
        // another application instance just brought in.
        let result = fs::hard_link(source, &target).or_else(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                return Err(error);
            }
            // Android shared storage and some synced drives do not support hard links.
            copy_new(source, &target)
        });
        match result {
            Ok(()) => {
                fs::remove_file(source)?;
                tracing::info!(from = %source.display(), to = %target.display(), "migrated course");
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let mut stem = source.file_stem().unwrap_or_default().to_os_string();
                stem.push(format!("-{suffix}.{EXTENSION}"));
                target = source.with_file_name(stem);
                suffix += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

fn copy_new(source: &Path, target: &Path) -> io::Result<()> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let copied = fs::File::open(source)
        .and_then(|mut input| io::copy(&mut input, &mut output))
        .and_then(|_| output.sync_all());
    drop(output);
    if copied.is_err() {
        let _ = fs::remove_file(target);
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn courses_move_with_their_contents_and_video_references_unchanged() {
        let dir = crate::tests::temp_dir("course-migration");
        let library = dir.join("courses");
        fs::create_dir_all(&library).unwrap();
        fs::write(library.join("Lake.tqc"), b"prepared course").unwrap();
        fs::write(library.join("Hills.TQC"), b"video course").unwrap();
        fs::write(library.join("Ride.mp4"), b"video").unwrap();
        fs::write(library.join("partial.tqc.part"), b"partial").unwrap();
        fs::create_dir(library.join("folder.tqc")).unwrap();

        run(&library);
        run(&library);

        assert_eq!(
            fs::read(library.join("Lake.oqc")).unwrap(),
            b"prepared course"
        );
        assert_eq!(
            fs::read(library.join("Hills.oqc")).unwrap(),
            b"video course"
        );
        assert_eq!(fs::read(library.join("Ride.mp4")).unwrap(), b"video");
        assert!(!library.join("Lake.tqc").exists());
        assert!(!library.join("Hills.TQC").exists());
        assert!(library.join("partial.tqc.part").exists());
        assert!(library.join("folder.tqc").is_dir());
        assert_eq!(fs::read_dir(&library).unwrap().count(), 5);
    }

    #[test]
    fn existing_courses_are_never_overwritten() {
        let dir = crate::tests::temp_dir("course-migration-collision");
        fs::write(dir.join("Lake.tqc"), b"legacy").unwrap();
        fs::write(dir.join("Lake.oqc"), b"current").unwrap();
        fs::write(dir.join("Lake-2.oqc"), b"another").unwrap();

        run(&dir);
        run(&dir);

        assert_eq!(fs::read(dir.join("Lake.oqc")).unwrap(), b"current");
        assert_eq!(fs::read(dir.join("Lake-2.oqc")).unwrap(), b"another");
        assert_eq!(fs::read(dir.join("Lake-3.oqc")).unwrap(), b"legacy");
        assert!(!dir.join("Lake.tqc").exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
    }

    #[test]
    fn a_new_library_is_not_created_by_migration() {
        let dir = crate::tests::temp_dir("course-migration-new");
        let library = dir.join("courses");
        run(&library);
        assert!(
            !library.exists(),
            "first-run sample installation still applies"
        );
    }

    #[test]
    fn the_copy_fallback_preserves_sources_and_never_overwrites_a_destination() {
        let dir = crate::tests::temp_dir("course-migration-copy");
        let source = dir.join("Lake.tqc");
        let target = dir.join("Lake.oqc");
        fs::write(&source, b"original").unwrap();

        copy_new(&source, &target).unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert_eq!(fs::read(&target).unwrap(), b"original");
        fs::write(&source, b"changed").unwrap();
        assert_eq!(
            copy_new(&source, &target).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&target).unwrap(), b"original");

        let failed = dir.join("failed.oqc");
        assert!(copy_new(&dir.join("missing.tqc"), &failed).is_err());
        assert!(!failed.exists());
    }

    #[test]
    fn startup_lists_migrated_courses_and_keeps_the_archive_identical() {
        let dir = crate::tests::temp_dir("course-migration-startup");
        let library = dir.join("data/courses");
        fs::create_dir_all(&library).unwrap();
        let bytes = include_bytes!("../../orqwa-gd/courses/twann-chasseral.oqc");
        fs::write(library.join("Sample.tqc"), bytes).unwrap();

        let app = crate::App::new(dir.join("data"), dir.join("cache")).unwrap();

        let listed = app.courses();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, library.join("Sample.oqc"));
        assert_eq!(fs::read(&listed[0].path).unwrap(), bytes);
        assert_eq!(
            crate::App::suggested_course_name(&listed[0].path),
            listed[0].manifest.name
        );
    }
}
