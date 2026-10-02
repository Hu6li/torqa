# ADR 0004 — Configuration and data formats

- Status: accepted
- Date: 2026-10-02

## Context

Torqa needs human-editable configuration and machine-written data/metadata, readable from Rust and,
for some data, from Godot. YAML was considered for configuration.

## Decision

- **TOML** for configuration (`toml` crate).
- **JSON** for data and metadata (`serde_json`; Godot reads JSON natively).
- **FIT** for activities.

## Rationale

- `serde_yaml`, the de-facto YAML crate, was deprecated and archived in 2024. Its successors are
  community forks of varying quality (one had soundness issues). That conflicts with the
  "no unmaintained crates" dependency policy.
- `toml` is the Rust ecosystem standard (used by Cargo) and well maintained.
- Godot has no built-in YAML parser but does parse JSON.
