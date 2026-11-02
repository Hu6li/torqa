#![no_main]

use std::io::Write as _;
use std::sync::OnceLock;

// The validator reads a file, as it does for a download; one file per process, rewritten.
fn path() -> &'static std::path::Path {
    static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::temp_dir().join(format!("orqwa-fuzz-course-{}.tqc", std::process::id()))
    })
}

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let mut file = std::fs::File::create(path()).expect("temporary course");
    file.write_all(data).expect("temporary course");
    drop(file);
    let _ = orqwa_storage::course::shared::validate(path());
});
