# ADR 0010 — Video decoding with FFmpeg in the Rust core

- Status: accepted
- Date: 2026-10-03

## Context

The video ride mode (R17) shows the rider's own footage (GoPro, Insta360 — H.264/HEVC in
MP4), downloaded videos and plain videos without GPS. Unlike a player, a ride decides the
moment of the video from the rider's position: speed varies all the time, the rider may stop,
and sync corrections jump around; frames are blended for smoothness. Godot only plays Ogg
Theora (`VideoStreamPlayer`), at its own pace.

Options considered:

- **Convert every video to Theora on import** and use Godot's player: no new dependency, but
  Theora is dated (large, blurry at 1080p), conversion needs an encoder anyway, and the
  player cannot be driven frame by frame.
- **A Godot FFmpeg plugin** (GDE GoZen, EIRTeam.FFmpeg) or **native decoders** (Native Video:
  AVFoundation / Media Foundation): good playback, but built for playing at the video's pace;
  sync and speed logic would sit in GDScript, against the architecture (logic in Rust).
- **FFmpeg in the Rust core**.

## Decision

- New crate **`torqa-video`** on **`ffmpeg-next` 9.0.0** (WTFPL). It hands out the frame for
  any moment (`Video::frame_at`), decoding forward when the next request is close and seeking
  to a keyframe otherwise, scaled to at most 1920 px wide (1080p); Godot only displays the
  frames.
- **FFmpeg 9.0 is built from source** by `ffmpeg-sys-next`'s `build` feature (the release/9.0
  branch, i.e. the latest 9.0.x), statically linked — the same version in the dev container,
  in CI and in app builds, no system FFmpeg needed. The default configuration is **LGPL**,
  compatible with our GPL-3.0; no GPL-only or non-free parts are enabled.
- On macOS FFmpeg's configure builds in VideoToolbox. The decoder does not use it yet: frames
  are decoded in software on a background thread (hardware decoding is a follow-up, see
  PLAN Phase 5).

## Consequences

- The first build compiles FFmpeg (several minutes); later builds reuse it from the target
  directory/cache. The container needs `libclang-dev` (bindgen) and `nasm`; macOS runners
  need `nasm` (CI only, never on developer machines).
- H.264 and HEVC are patent-encumbered in some countries; FFmpeg's decoders are used as is,
  as by most open-source players.
- Transcoding down on import (R17) needs an encoder: VideoToolbox on macOS; until then frames
  are scaled while decoding. Whether software decoding keeps up with 4K on an M1 is to be
  measured; hardware decoding is the next step if not.

## Amendment (2026-10-04): AV1 with rav1d (#39)

Route-video libraries such as Van Gestel's are AV1. FFmpeg's own AV1 decoder only drives
hardware decoders, which M1/M2 Macs lack, so those videos showed nothing.

- AV1 is decoded with **rav1d**, the Rust port of dav1d (BSD-2-Clause); FFmpeg still reads the
  file and converts the pictures. rav1d publishes only dav1d's C interface, so `torqa-video`'s
  `av1` module is its one place with `unsafe` code: small FFI wrappers with documented safety.
- The crate is **`re_rav1d` 0.1.3** (rerun's publish of rav1d): rav1d 1.1.0's package leaves out
  the headers its ARM assembly needs, which would make it slow on Apple Silicon. To switch back
  to `rav1d` once a release includes them (tracked in an issue).
- Its assembly is enabled except on Linux/ARM, where it cannot be linked into the GDExtension
  (a shared library); there the pure Rust decoder is used — still ~145 fps at 1080p in the dev
  container, ~250 fps with assembly.
- rav1d depends on `paste`, a finished compile-time macro flagged unmaintained
  (RUSTSEC-2024-0436); `deny.toml` ignores that advisory, and allows `CC0-1.0` (`to_method`).
