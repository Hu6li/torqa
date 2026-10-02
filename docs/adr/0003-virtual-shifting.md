# ADR 0003 — Virtual shifting

- Status: accepted
- Date: 2026-10-02

## Context

The KICKR Core 2 can be used with a normal cassette or with a single Zwift Cog (R9). With a cassette,
real shifting works without app involvement: in SIM mode the app sends the gradient and the trainer
sets resistance based on flywheel speed. With a Zwift Cog, gears must be simulated.

Zwift's own virtual shifting runs over the proprietary "Zwift protocol" that requires trainer
firmware support and has no published specification. Zwift initially promised an open protocol
for the Click, then walked this back. Rouvy and the open-source BikeControl project use
reverse-engineered implementations; BikeControl also defines the open OpenBikeControl protocol.

## Decision

- Implement **app-side virtual gears** over standard FTMS: the selected virtual gear ratio adjusts
  the resistance/grade sent to the trainer. Works with any FTMS trainer, not just Zwift-protocol ones.
- Shift input is a separate `ShiftInput` trait with implementations for keyboard/gamepad,
  OpenBikeControl, and Zwift Click via reverse-engineered BLE (isolated, optional, may break with
  firmware updates).
- Virtual shifting is optional and post-MVP; with a cassette it stays off.

## Consequences

- No dependency on Zwift's proprietary protocol for resistance control.
- Click support carries maintenance risk; check BikeControl's license before reusing any code.
