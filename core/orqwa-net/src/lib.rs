//! Bounded group ride networking (R159–R161, ADR 0035).
//!
//! Pasted codes are untrusted input. Destination policy is checked when decoding and again
//! before any network operation. LAN and loopback access require separate explicit choices.

pub mod activity;
pub mod code;
pub mod content;
pub mod crypto;
pub mod discovery;
#[cfg(any(test, feature = "fuzzing"))]
#[doc(hidden)]
pub mod fuzz;
pub mod handover;
pub mod limits;
pub mod link;
pub mod lobby;
pub mod policy;
mod runtime;
pub mod state;
pub mod stun;
mod text;
mod tls;
pub mod voice;
mod wire;
pub mod workout;
