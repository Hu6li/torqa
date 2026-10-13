//! Group rides without a server (R67–R69, [ADR 0015]): riders pass each other [`code`]s, each
//! side [`gather`]s the addresses it may be reached at — its own, the public one a [`stun`]
//! server sees and a port its [`router`] maps — and the two then punch through their routers
//! and talk QUIC ([`link`]).
//!
//! [ADR 0015]: https://github.com/bossm8/torqa/blob/main/docs/adr/0015-group-rides.md

mod base64url;
pub mod candidates;
pub mod code;
pub mod link;
pub mod router;
pub mod stun;

pub use candidates::{Candidate, CandidateKind, GatherOptions, Gathered, NatBehavior, gather};
pub use code::{CodeError, Course, Invite, Reply};
pub use link::{
    ConnectOptions, Connection, Hint, Host, HostEvent, JoinError, Joiner, OpenError, diagnose,
};
