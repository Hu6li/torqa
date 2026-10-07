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

## Amendment 2026-10-06: how the gears feel

Implemented in `torqa_physics::VirtualGears`:

- **Gears:** 24, from 0.75 to 5.5 (chainring over cog), each about 9 % above the last. A ride
  starts in the gear nearest to the real one (chainring / cog of the rider's drivetrain,
  50 / 14 by default).
- **Resistance:** in slope simulation the trainer brakes its flywheel, turning at the speed of
  the real gear, as the road would at that speed. In a virtual gear `r` times the real one, the
  same cadence is `r` times the speed, and the pedals feel the road's force at that speed times
  the leverage `r`. With the trainer's force `m·g·(grade + Crr) + Cw·v²`, sending `r·grade`,
  `r·Crr` and `r³·Cw` gives exactly `r·F_road(r·v)` — so gears change how hard the road feels
  at a cadence, while Torqa's virtual speed still comes from the measured power. Crr and Cw
  are capped at what FTMS can carry (0.0255, 2.55 kg/m); the trainer limits the grade.
- **Inputs:** behind `ShiftInput` (`torqa_domain::shifting`): the keyboard (↑ / ↓ in the
  app, `u` / `d` in the CLI) and **Shimano Di2** (R59): the 12-speed Di2 or the EW-WU111
  wireless unit indicates presses of the buttons assigned to D-Fly channels (service
  `0x18EF`, characteristic `0x2AC2` in Shimano's own UUID base; `[counter, ch1…ch4]`, flags
  for short, long and double presses). Each channel's byte keeps its last press until the
  next one (only a long press is released), so a press is a channel whose byte changed, not
  every flagged channel of a new indication — on a real RD-R8150 that miscounted presses on
  the other channel. Each channel's press, hold and double press do what the rider gives
  them (#139): shift one or two gears, or a ride control the front end carries out as its key
  (next camera, overlay, music). `ShiftInput::controls` passes the controls on beside the
  shifts. Until assigned, the channels chosen to shift up and down do so, one gear a press or
  hold and two a double press.
  The Zwift Click is not supported: its protocol is encrypted, the v2 must be unlocked in
  Zwift each day, and the bridge app that offers it (BikeControl) needs a paid subscription.
- In ERG (workouts) the trainer holds the power in any gear.
