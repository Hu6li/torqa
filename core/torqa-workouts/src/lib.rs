//! Structured workouts for Torqa (R21): reading ZWO, ERG/MRC and FIT workout files through
//! the [`WorkoutParser`] interface, the built-in workouts, and the rider's workout library — a
//! `workouts/` folder in the data directory, shared by all riders like the courses.

mod erg;
mod fit;
mod zwo;

use std::path::{Path, PathBuf};

pub use erg::ErgParser;
pub use fit::FitParser;
use torqa_domain::units::Watts;
use torqa_domain::workout::{Plan, WorkoutFileError, WorkoutParser};
use tracing::warn;
pub use zwo::{ZwoParser, write as write_zwo};

/// Built-in workouts start their ids with this; files are known by their path.
pub const BUILTIN: &str = "builtin:";

/// The built-in workouts, as ZWO files: (id after [`BUILTIN`], file).
const BUILTINS: [(&str, &str); 5] = [
    ("recovery-30", include_str!("../builtin/recovery-30.zwo")),
    ("endurance-60", include_str!("../builtin/endurance-60.zwo")),
    (
        "sweet-spot-3x10",
        include_str!("../builtin/sweet-spot-3x10.zwo"),
    ),
    (
        "threshold-2x15",
        include_str!("../builtin/threshold-2x15.zwo"),
    ),
    ("vo2max-5x3", include_str!("../builtin/vo2max-5x3.zwo")),
];

/// Every workout file parser.
#[must_use]
pub fn parsers() -> [&'static dyn WorkoutParser; 3] {
    [&ZwoParser, &ErgParser, &FitParser]
}

/// The file extensions of workout files, lowercase without the dot.
#[must_use]
pub fn extensions() -> Vec<&'static str> {
    parsers()
        .iter()
        .flat_map(|parser| parser.extensions().iter().copied())
        .collect()
}

/// Reads a workout file, by its extension.
///
/// # Errors
/// [`WorkoutFileError`] if it cannot be read or is no workout Torqa understands.
pub fn parse_file(path: &Path) -> Result<Plan, WorkoutFileError> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let parser = parsers()
        .into_iter()
        .find(|parser| parser.extensions().contains(&extension.as_str()))
        .ok_or_else(|| {
            WorkoutFileError(format!(
                "{} is not a workout file ({})",
                path.display(),
                extensions().join(", ")
            ))
        })?;
    let bytes = std::fs::read(path)
        .map_err(|e| WorkoutFileError(format!("cannot read {}: {e}", path.display())))?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    parser
        .parse(&bytes, &name)
        .map_err(|e| WorkoutFileError(format!("{}: {e}", path.display())))
}

/// A workout to choose from.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// How to load it again: `builtin:…` or the file's path.
    pub id: String,
    /// The workout.
    pub plan: Plan,
}

/// The workout library folder in the data directory.
#[must_use]
pub fn library_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("workouts")
}

/// The built-in workouts, then those in `dir` by name; files that cannot be read are left out
/// (and logged).
#[must_use]
pub fn library(dir: &Path) -> Vec<Entry> {
    let mut files: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| match parse_file(&path) {
            Ok(plan) => Some(Entry {
                id: path.display().to_string(),
                plan,
            }),
            Err(error) => {
                warn!(%error, "workout left out of the library");
                None
            }
        })
        .collect();
    files.sort_by_key(|entry| entry.plan.name.to_lowercase());
    builtins().into_iter().chain(files).collect()
}

/// The built-in workouts.
#[must_use]
pub fn builtins() -> Vec<Entry> {
    BUILTINS
        .iter()
        .filter_map(|(id, xml)| {
            let plan = ZwoParser.parse(xml.as_bytes(), id).ok()?;
            Some(Entry {
                id: format!("{BUILTIN}{id}"),
                plan,
            })
        })
        .collect()
}

/// Loads a workout by its [`Entry::id`].
///
/// # Errors
/// [`WorkoutFileError`] for unknown built-ins and unreadable files.
pub fn load(id: &str) -> Result<Plan, WorkoutFileError> {
    if let Some(builtin) = id.strip_prefix(BUILTIN) {
        return builtins()
            .into_iter()
            .find(|entry| entry.id.strip_prefix(BUILTIN) == Some(builtin))
            .map(|entry| entry.plan)
            .ok_or_else(|| WorkoutFileError(format!("no built-in workout {builtin}")));
    }
    parse_file(Path::new(id))
}

/// Adds a workout file to the library in `dir` (a copy, under a name of its own); returns the
/// copy's path.
///
/// # Errors
/// [`WorkoutFileError`] if it is no workout Torqa understands, or cannot be copied.
pub fn import(dir: &Path, file: &Path) -> Result<PathBuf, WorkoutFileError> {
    parse_file(file)?;
    std::fs::create_dir_all(dir)
        .map_err(|e| WorkoutFileError(format!("cannot create {}: {e}", dir.display())))?;
    let stem = file.file_stem().map_or_else(
        || "workout".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mut target = dir.join(format!("{stem}.{extension}"));
    let mut copy = 2;
    while target.exists() {
        target = dir.join(format!("{stem} {copy}.{extension}"));
        copy += 1;
    }
    std::fs::copy(file, &target)
        .map_err(|e| WorkoutFileError(format!("cannot copy {}: {e}", file.display())))?;
    Ok(target)
}

/// Saves `plan` (from the editor) into the library `dir` as a ZWO file: over `replace` if that
/// is a ZWO file of the library, else as a new file (an edited built-in or imported ERG/FIT
/// file is kept as it was); steps in watts become shares of `ftp`. Returns the file's id.
///
/// # Errors
/// [`WorkoutFileError`] for a plan without steps or if the file cannot be written.
pub fn save(
    dir: &Path,
    plan: &Plan,
    ftp: Watts,
    replace: Option<&str>,
) -> Result<String, WorkoutFileError> {
    if plan.steps.is_empty() {
        return Err(WorkoutFileError(
            "a workout needs at least one step".to_owned(),
        ));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| WorkoutFileError(format!("cannot create {}: {e}", dir.display())))?;
    let path = match replace.map(Path::new) {
        Some(path) if is_library_zwo(dir, path) => path.to_path_buf(),
        _ => new_file(dir, &plan.name),
    };
    std::fs::write(&path, write_zwo(plan, ftp))
        .map_err(|e| WorkoutFileError(format!("cannot write {}: {e}", path.display())))?;
    Ok(path.display().to_string())
}

/// Deletes a workout file of the library `dir`; built-ins and files elsewhere stay.
///
/// # Errors
/// [`WorkoutFileError`] for built-ins, files outside the library, or if deleting fails.
pub fn delete(dir: &Path, id: &str) -> Result<(), WorkoutFileError> {
    let path = Path::new(id);
    if id.starts_with(BUILTIN) || path.parent() != Some(dir) {
        return Err(WorkoutFileError(format!(
            "{id} is not in your workout library"
        )));
    }
    std::fs::remove_file(path).map_err(|e| WorkoutFileError(format!("cannot delete {id}: {e}")))
}

fn is_library_zwo(dir: &Path, path: &Path) -> bool {
    path.parent() == Some(dir)
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zwo"))
}

/// A new file in `dir` named after the workout.
fn new_file(dir: &Path, name: &str) -> PathBuf {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_owned();
    let stem = if stem.is_empty() {
        "Workout".to_owned()
    } else {
        stem
    };
    let mut path = dir.join(format!("{stem}.zwo"));
    let mut copy = 2;
    while path.exists() {
        path = dir.join(format!("{stem} {copy}.zwo"));
        copy += 1;
    }
    path
}

/// The middle of a Coggan power zone (1–7) as a share of FTP, for files giving zones only.
pub(crate) fn zone_share(zone: f64) -> f64 {
    const MIDDLES: [f64; 7] = [0.45, 0.65, 0.83, 0.98, 1.13, 1.35, 1.6];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 1–7 first
    let index = zone.round().clamp(1.0, 7.0) as usize - 1;
    MIDDLES[index]
}

#[cfg(test)]
mod tests;
