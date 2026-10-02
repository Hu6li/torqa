//! Godot GDExtension bindings exposing the Torqa core to the presentation layer.

// This crate is the FFI boundary: gdext's entry point is an `unsafe impl`, and the
// `#[gdextension]` macro drops item-level attributes, so the allow must be crate-wide.
#![allow(unsafe_code)]

use godot::prelude::*;

struct TorqaExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TorqaExtension {}

/// Entry point for GDScript into the Rust core.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TorqaCore;

#[godot_api]
impl TorqaCore {
    /// Version of the Torqa core.
    #[func]
    fn version() -> GString {
        GString::from(torqa_domain::version())
    }
}
