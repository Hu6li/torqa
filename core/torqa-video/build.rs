//! Links the Windows system libraries FFmpeg's static libraries need (bcrypt for its random
//! numbers, the rest for its Windows backends). ffmpeg-sys-next lists them only when it builds
//! FFmpeg itself; ours comes prebuilt through `FFMPEG_DIR` (ADR 0010).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for lib in [
            "ole32", "oleaut32", "gdi32", "user32", "vfw32", "strmiids", "bcrypt", "shlwapi",
            "shell32",
        ] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    }
}
