//! Link settings for the GDExtension library, and the branch and commit it is built from.

use std::path::Path;
use std::process::Command;

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
    if os == "macos" {
        microphone(&arch);
    }
    commit();
    branch();
    println!("cargo:rerun-if-changed=build.rs");
}

/// Clang supplies Apple's block ABI; no third-party Objective-C runtime wrapper is needed.
fn microphone(arch: &str) {
    let out =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    let object = out.join("microphone.o");
    let archive = out.join("liborqwa_microphone.a");
    let apple_arch = if arch == "aarch64" { "arm64" } else { arch };
    let status = Command::new("xcrun")
        .args(["--sdk", "macosx", "clang", "-arch", apple_arch])
        .args([
            "-mmacosx-version-min=11.0",
            "-fobjc-arc",
            "-fblocks",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
        ])
        .arg("src/microphone_permission.m")
        .arg("-o")
        .arg(&object)
        .status()
        .expect("Apple Clang for microphone authorization");
    assert!(
        status.success(),
        "compiling microphone authorization failed"
    );
    let status = Command::new("xcrun")
        .args(["ar", "crs"])
        .arg(archive)
        .arg(object)
        .status()
        .expect("Apple archive tool");
    assert!(
        status.success(),
        "archiving microphone authorization failed"
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=orqwa_microphone");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rerun-if-changed=src/microphone_permission.m");
}

/// The commit the About tab shows (R83, R130, ADR 0018), as `ORQWA_COMMIT`, cut to seven
/// characters: that variable's value when set, else Git's; empty without either, e.g. built
/// from a source archive. `scripts/dev.sh` sets the variable for worktrees, whose Git directory
/// the container cannot see, and CI to a pull request's own last commit, as it checks out a
/// merge of it that no branch has.
fn commit() {
    println!("cargo:rerun-if-env-changed=ORQWA_COMMIT");
    let commit: String = from_env("ORQWA_COMMIT")
        .or_else(|| from_git(&["rev-parse", "--short=7", "HEAD"]))
        .unwrap_or_default()
        .chars()
        .take(7)
        .collect();
    println!("cargo:rustc-env=ORQWA_COMMIT={commit}");
}

/// The branch the About tab names with the commit (R130, ADR 0018), as `ORQWA_BRANCH`: that
/// variable's value when set, as `scripts/dev.sh` and CI set it, else Git's; empty on a detached
/// checkout, such as a release built from its tag.
fn branch() {
    println!("cargo:rerun-if-env-changed=ORQWA_BRANCH");
    let branch = from_env("ORQWA_BRANCH")
        .or_else(|| from_git(&["rev-parse", "--abbrev-ref", "HEAD"]))
        .filter(|branch| branch != "HEAD")
        .unwrap_or_default();
    println!("cargo:rustc-env=ORQWA_BRANCH={branch}");
}

/// The variable's value, `None` when unset or empty.
fn from_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Git's answer about this checkout, building again when it can change.
fn from_git(args: &[&str]) -> Option<String> {
    // A commit or a checkout rewrites HEAD or adds to its log.
    for name in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", name])
            && Path::new(&path).exists()
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    git(args)
}

/// Git's answer, trimmed; `None` if Git is missing or fails.
fn git(args: &[&str]) -> Option<String> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let output = Command::new("git")
        // The checkout can belong to another user than the build, as in CI's container, and
        // Git refuses such a repository unless told it is safe. This only reads it.
        .args(["-c", "safe.directory=*", "-C", &manifest])
        .args(args)
        .output()
        .ok()?;
    let answer = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !answer.is_empty()).then_some(answer)
}
