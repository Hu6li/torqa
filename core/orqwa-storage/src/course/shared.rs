//! Validation boundary for courses received from group peers, before cache extraction/import.

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::{CourseError, File, MANIFEST, Manifest, ROUTE, ZipArchive};

/// Maximum encoded size of a peer-supplied course, matching the transfer limit.
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum aggregate uncompressed course data.
pub const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Maximum members, checked in the ZIP trailer before allocating a central directory.
pub const MAX_ENTRIES: usize = 16_384;
const MAX_DIRECTORY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
const MAX_ROUTE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

fn invalid() -> CourseError {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "The shared course is invalid, unsupported or exceeds its limits.",
    )
    .into()
}

// A normal course under our size/count caps needs no multi-disk or ZIP64 trailer.
// Reject both before ZipArchive can allocate from attacker-controlled directory counts.
fn directory(file: &mut File) -> Result<usize, CourseError> {
    let size = file.metadata()?.len();
    if !(22..=MAX_ARCHIVE_BYTES).contains(&size) {
        return Err(invalid());
    }
    let length = usize::try_from(size.min(22 + 65_535)).map_err(|_| invalid())?;
    file.seek(SeekFrom::Start(size - length as u64))?;
    let mut tail = vec![0; length];
    file.read_exact(&mut tail)?;
    let offset = (0..=length - 22)
        .rev()
        .find(|&i| {
            tail[i..].starts_with(b"PK\x05\x06")
                && i + 22 + usize::from(u16::from_le_bytes([tail[i + 20], tail[i + 21]])) == length
        })
        .ok_or_else(invalid)?;
    let header = &tail[offset..];
    let short = |i: usize| u16::from_le_bytes([header[i], header[i + 1]]);
    let word =
        |i: usize| u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
    let count = usize::from(short(10));
    let central_bytes = u64::from(word(12));
    let central_at = u64::from(word(16));
    if short(4) != 0
        || short(6) != 0
        || usize::from(short(8)) != count
        || !(2..=MAX_ENTRIES).contains(&count)
        || central_bytes > MAX_DIRECTORY_BYTES
        || central_at + central_bytes != size - length as u64 + offset as u64
    {
        return Err(invalid());
    }
    let end_at = size - length as u64 + offset as u64;
    if end_at >= 20 {
        file.seek(SeekFrom::Start(end_at - 20))?;
        let mut locator = [0; 4];
        file.read_exact(&mut locator)?;
        if &locator == b"PK\x06\x07" {
            return Err(invalid());
        }
    }
    file.rewind()?;
    Ok(count)
}

fn safe(name: &str, directory: bool) -> bool {
    let name = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    !name.is_empty()
        && name.len() <= 1024
        && !name.contains(['\\', ':'])
        && !name.chars().any(char::is_control)
        && name.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !part.is_empty()
                && !part.ends_with(['.', ' '])
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.starts_with("COM") || stem.starts_with("LPT"))
                    .then(|| &stem[3..])
                    .is_some_and(|n| {
                        matches!(
                            n,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
        })
}

/// Validates ZIP metadata and every decompressed byte without writing to the shared cache.
/// Only 3D course inputs are accepted. Parsing/route preparation must still run before Ready.
/// Run this synchronous bounded scan on a worker, never in the ride/trainer loop.
///
/// # Errors
/// Returns [`CourseError`] for malformed framing, unsafe paths, duplicates, symlinks, expansion
/// limits, CRC failures, invalid metadata, unsupported formats or referenced video.
pub fn validate(path: &Path) -> Result<Manifest, CourseError> {
    let mut file = File::open(path)?;
    let count = directory(&mut file)?;
    let mut zip = ZipArchive::new(file)?;
    if zip.len() != count {
        return Err(invalid());
    }
    let mut expanded = 0u64;
    let mut names = BTreeSet::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        let name = entry.name()?.into_owned();
        let is_dir = entry.is_dir();
        let mode = entry.unix_mode().unwrap_or(0) & 0o170_000;
        if !safe(&name, is_dir)
            || (mode != 0 && mode != 0o100_000 && mode != 0o040_000)
            || !names.insert(name.clone())
        {
            return Err(invalid());
        }
        let cap = match name.as_str() {
            MANIFEST => MAX_MANIFEST_BYTES,
            ROUTE => MAX_ROUTE_BYTES,
            "map.png" | "preview.png" => MAX_IMAGE_BYTES,
            _ if name.starts_with("data/") || (is_dir && name == "data/") => MAX_ENTRY_BYTES,
            _ => return Err(invalid()),
        };
        let declared = entry.size();
        expanded = expanded.checked_add(declared).ok_or_else(invalid)?;
        if declared > cap || expanded > MAX_EXPANDED_BYTES || (is_dir && declared != 0) {
            return Err(invalid());
        }
        // A lying local header or decompressor must not bypass the declared-size budget.
        let read = std::io::copy(&mut entry.take(declared + 1), &mut std::io::sink())?;
        if read != declared {
            return Err(invalid());
        }
    }
    if !names.contains(MANIFEST) || !names.contains(ROUTE) {
        return Err(invalid());
    }
    let manifest = super::manifest_of(&mut zip)?;
    if manifest.format != super::FORMAT_VERSION
        || manifest.video.is_some()
        || manifest.name.is_empty()
        || manifest.name.len() > 1024
        || manifest.name.chars().any(char::is_control)
        || !manifest.length_m.is_finite()
        || !(0.0..=10_000_000.0).contains(&manifest.length_m)
        || !manifest.elevation_gain_m.is_finite()
        || manifest.elevation_gain_m < 0.0
        || !manifest.max_grade_percent.is_finite()
        || manifest.track.iter().flatten().any(|n| !n.is_finite())
        || manifest.profile.iter().flatten().any(|n| !n.is_finite())
    {
        return Err(invalid());
    }
    if zip.by_name(ROUTE)?.size() == 0 {
        return Err(invalid());
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};
    fn archive(entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "orqwa-shared-course-{}-{}.tqc",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(
                *name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }
    fn manifest() -> Vec<u8> {
        br#"{"format":1,"generator":"ORQWA","name":"Shared","length_m":1000,"elevation_gain_m":20,"max_grade_percent":3,"created_unix_s":0,"attribution":[]}"#.to_vec()
    }
    // scripts/fuzz.sh runs this before fuzzing the validator from a small accepted course.
    #[test]
    #[ignore = "writes the fuzz corpus named by ORQWA_FUZZ_CORPUS"]
    fn write_fuzz_seed() {
        let directory = std::path::PathBuf::from(
            std::env::var_os("ORQWA_FUZZ_CORPUS").expect("ORQWA_FUZZ_CORPUS"),
        )
        .join("course");
        std::fs::create_dir_all(&directory).unwrap();
        let path = archive(&[
            (MANIFEST, &manifest()),
            (ROUTE, b"<gpx/>"),
            ("data/map/tile", b"tile"),
        ]);
        validate(&path).unwrap();
        std::fs::rename(&path, directory.join("seed.tqc"))
            .or_else(|_| std::fs::copy(&path, directory.join("seed.tqc")).map(|_| ()))
            .unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn shared_courses_are_checked_before_any_extraction() {
        let manifest = manifest();
        let path = archive(&[
            (MANIFEST, &manifest),
            (ROUTE, b"<gpx/>"),
            ("data/map/tile", b"tile"),
        ]);
        assert_eq!(validate(&path).unwrap().name, "Shared");
        std::fs::remove_file(path).unwrap();
        for name in [
            "data/../escape",
            "data/C:/escape",
            "data\\escape",
            "/data/escape",
            "data//escape",
            "data/.. /escape",
            "data/a./b",
            "data/NUL",
            "data/COM1.txt",
            "unknown",
        ] {
            let path = archive(&[(MANIFEST, &manifest), (ROUTE, b"<gpx/>"), (name, b"bad")]);
            assert!(validate(&path).is_err(), "{name}");
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn unsupported_video_invalid_metadata_and_directory_counts_are_rejected() {
        let mut metadata: serde_json::Value = serde_json::from_slice(&manifest()).unwrap();
        for value in [serde_json::json!(0), serde_json::json!(2)] {
            metadata["format"] = value;
            let path = archive(&[
                (MANIFEST, &serde_json::to_vec(&metadata).unwrap()),
                (ROUTE, b"<gpx/>"),
            ]);
            assert!(validate(&path).is_err());
            std::fs::remove_file(path).unwrap();
        }
        let path = archive(&[(MANIFEST, &manifest()), (ROUTE, b"<gpx/>")]);
        let mut bytes = std::fs::read(&path).unwrap();
        let offset = bytes.len() - 22;
        bytes[offset + 8..offset + 12].copy_from_slice(&[255; 4]);
        std::fs::write(&path, bytes).unwrap();
        assert!(validate(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_bundled_course_passes_the_peer_import_boundary() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../orqwa-gd/courses/twann-chasseral.tqc");
        assert!(validate(&path).is_ok());
    }

    #[test]
    fn oversized_entries_are_rejected_before_decompression() {
        let path = archive(&[(MANIFEST, &manifest()), (ROUTE, b"<gpx/>")]);
        let mut bytes = std::fs::read(&path).unwrap();
        let index = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        bytes[index + 24..index + 28]
            .copy_from_slice(&u32::try_from(MAX_MANIFEST_BYTES + 1).unwrap().to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(validate(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
