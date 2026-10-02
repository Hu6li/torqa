# ADR 0001 — Godot 4 + Rust core

- Status: accepted
- Date: 2026-10-02

## Context

Torqa needs semi-realistic 3D at 60 fps on an M1 integrated GPU (R16), Bluetooth LE trainer control
(R5), macOS first with Windows/Linux later (R1), and a GPL-3.0 license (R2).

## Decision

Use **Godot 4** for rendering and UI (statically typed GDScript) and a **Rust** workspace for all
logic, exposed to Godot via GDExtension (`godot-rust/gdext`).

## Rationale

- Godot is MIT-licensed and GPL-compatible; Unity's and Unreal's licenses are not.
- Godot renders natively via Metal (macOS) and Vulkan/D3D12 (Windows/Linux) from one codebase.
- Rust gives fast, memory-safe device, physics and data code. `btleplug` covers BLE on all three
  desktop OSes.
- A headless Rust core is testable in containers and reusable for a future multiplayer server (R27).

## Alternatives considered

- **Tauri/Electron + Three.js/WebGPU**: easier UI, but lower 3D fidelity and unreliable WebGPU on Linux.
- **Pure Rust (Bevy)**: maximum control, but immature UI and editor tooling.
- **Godot + C#**: one language, but weaker cross-platform BLE libraries and an extra .NET runtime.

## Consequences

- Two languages (Rust, GDScript); the boundary is strict: logic in Rust, presentation in Godot.
- macOS GDExtension builds come from GitHub Actions macOS runners (no native toolchain on the host).
