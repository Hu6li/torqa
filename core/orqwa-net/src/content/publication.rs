//! Shared storage can reject hard links while still supporting atomic, no-replace renames.
#![allow(unsafe_code)] // small OS boundary; only owned, NUL-terminated paths cross it

use std::io;
use std::path::Path;

pub(super) fn retain(from: &Path, to: &Path) -> io::Result<()> {
    with_link(from, to, |from, to| std::fs::hard_link(from, to))
}

fn with_link(
    from: &Path,
    to: &Path,
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    match link(from, to) {
        Ok(()) => Ok(()),
        #[cfg(any(target_os = "android", target_os = "linux"))]
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
            ) =>
        {
            rename_no_replace(from, to)
        }
        Err(error) => Err(error),
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::{CString, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt as _;
    const AT_FDCWD: c_int = -100;
    const NO_REPLACE: c_uint = 1;
    let from =
        CString::new(from.as_os_str().as_bytes()).map_err(|_| io::ErrorKind::InvalidInput)?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(|_| io::ErrorKind::InvalidInput)?;
    #[cfg(target_os = "linux")]
    let result = {
        unsafe extern "C" {
            fn renameat2(
                old_dir: c_int,
                old: *const std::ffi::c_char,
                new_dir: c_int,
                new: *const std::ffi::c_char,
                flags: c_uint,
            ) -> c_int;
        }
        // SAFETY: both C strings remain alive for this synchronous syscall; flags disallow replacement.
        unsafe { renameat2(AT_FDCWD, from.as_ptr(), AT_FDCWD, to.as_ptr(), NO_REPLACE) }
    };
    #[cfg(target_os = "android")]
    let result = android_rename(AT_FDCWD, &from, &to, NO_REPLACE);
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "android")]
fn android_rename(
    dir: std::ffi::c_int,
    from: &std::ffi::CStr,
    to: &std::ffi::CStr,
    flags: std::ffi::c_uint,
) -> std::ffi::c_long {
    use std::ffi::c_long;
    // Stable Linux syscall numbers from the NDK's UAPI headers. Bionic's renameat2 symbol
    // requires API 30, while the kernel syscall also works on our Android 10 / API 29 floor.
    #[cfg(target_arch = "aarch64")]
    const RENAMEAT2: c_long = 276;
    #[cfg(target_arch = "x86_64")]
    const RENAMEAT2: c_long = 316;
    #[cfg(target_arch = "arm")]
    const RENAMEAT2: c_long = 382;
    #[cfg(target_arch = "x86")]
    const RENAMEAT2: c_long = 353;
    unsafe extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
    }
    // SAFETY: the Linux ABI takes two directory descriptors, two valid C strings and flags.
    unsafe { syscall(RENAMEAT2, dir, from.as_ptr(), dir, to.as_ptr(), flags) }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let token = crate::crypto::random::<16>().unwrap();
            let path = std::env::temp_dir().join(format!("orqwa-publication-{token:x?}"));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn no_links(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }

    #[test]
    fn shared_storage_without_hard_links_publishes_complete_files_atomically() {
        let dir = Directory::new();
        let from = dir.0.join("validated.part");
        let to = dir.0.join("course.tqc");
        std::fs::write(&from, b"verified course").unwrap();
        with_link(&from, &to, no_links).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"verified course");
        assert!(!from.exists());
        std::fs::write(&from, b"different course").unwrap();
        assert_eq!(
            with_link(&from, &to, no_links).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(&to).unwrap(), b"verified course");
        assert_eq!(std::fs::read(&from).unwrap(), b"different course");
    }

    #[test]
    fn fallback_cannot_replace_a_racing_publisher_or_a_symlink() {
        let dir = Directory::new();
        let from = dir.0.join("validated.part");
        let to = dir.0.join("course.tqc");
        std::fs::write(&from, b"verified course").unwrap();
        let race = |_: &Path, target: &Path| {
            std::fs::write(target, b"already here").unwrap();
            no_links(&from, target)
        };
        assert_eq!(
            with_link(&from, &to, race).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(&to).unwrap(), b"already here");
        std::fs::remove_file(&to).unwrap();
        std::os::unix::fs::symlink(&from, &to).unwrap();
        assert_eq!(
            with_link(&from, &to, no_links).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(to.is_symlink());
    }
}
