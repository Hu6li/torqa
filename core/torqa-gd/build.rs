//! Link settings for the GDExtension library.

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // rav1d's x86 assembly addresses rav1d's tables directly, as if they could not be replaced,
    // but a shared library's exported symbols can be on Linux, and the linker refuses that.
    // Binding the library's references to its own definitions makes the assembly's assumption
    // true (ADR 0010).
    if os == "linux" && arch == "x86_64" {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-Bsymbolic");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
