#![no_main]

use std::io::Write as _;
use std::sync::OnceLock;

// The validator reads a file, as it does when a rider imports one; one file per process.
fn path() -> &'static std::path::Path {
    static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::temp_dir().join(format!("orqwa-fuzz-world-{}.oqx", std::process::id()))
    })
}

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let mut file = std::fs::File::create(path()).expect("temporary world");
    file.write_all(data).expect("temporary world");
    drop(file);
    let _ = orqwa_storage::world::validate(path());
});
