//! Apple's microphone authorization UI, independent of audio streams and group networking.

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn orqwa_microphone_permission(request: bool) -> std::ffi::c_int;
}

/// Queries access, requesting it only for an explicit microphone action on the UI thread.
/// Other platforms keep their existing permission flow.
pub fn access(request: bool) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a synchronous C function taking/returning scalars. Its asynchronous
        // Apple block owns no Rust data and never calls Godot from a background thread.
        match unsafe { orqwa_microphone_permission(request) } {
            0 => "undetermined",
            2 => "denied",
            3 => "allowed",
            4 => "pending",
            5 => "unavailable",
            _ => "restricted",
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = request;
        "allowed"
    }
}
