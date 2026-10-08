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
- **FFmpeg 9.0.2 is built by `scripts/build-ffmpeg.sh`** from the release tarball, its SHA-256
  pinned in the script, statically linked, and found by `ffmpeg-sys-next` through `FFMPEG_DIR`
  — the same bytes in the dev container (built into the image), in CI (built once per
  platform and cached) and in app builds, no system FFmpeg needed. Only avcodec, avformat,
  avutil and swscale, nothing autodetected, no network; the configuration is **LGPL**,
  compatible with our GPL-3.0; no GPL-only or non-free parts are enabled. *(Until 2026-10-08
  `ffmpeg-sys-next`'s `build` feature cloned FFmpeg's release/9.0 branch at build time:
  unpinned, unverified, and compiled again on every cold build.)*
- Frames are decoded in software on a background thread; hardware decoding (VideoToolbox on
  macOS) is a follow-up, see PLAN Phase 5, and would add Apple's frameworks to the link.

## Consequences

- Building FFmpeg takes several minutes: once per dev image, once per platform in CI (cached
  by the script's hash). The container needs `libclang-dev` (bindgen) and `nasm`; the CI
  runners need `nasm` (never on developer machines). On Windows `torqa-video`'s build script
  links the system libraries FFmpeg's static libraries need.
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
- The crate is the official **`rav1d` 1.1.0**, **without its assembly**: the package leaves out
  the headers its ARM assembly needs, and the pure Rust decoder is fast enough — ~145 fps at
  1080p (decoding and scaling) in the dev container, ~250 fps with assembly. rerun's
  `re_rav1d`, which packages the headers, was tried and dropped: its repository is archived.
  Assembly can be enabled once a rav1d release ships the headers (tracked in an issue).
- rav1d depends on `paste`, a finished compile-time macro flagged unmaintained
  (RUSTSEC-2024-0436); `deny.toml` ignores that advisory, and allows `CC0-1.0` (`to_method`).

## Amendment (2026-10-08): rav1d's x86 assembly and frame threading

The ~145 fps above does not hold everywhere. On an Intel i7-9750H laptop (6 cores) a
720p, 30 fps AV1 route video decoded at **21 fps**: slower than it plays, so rides on it
stuttered while ordinary players, using dav1d with its assembly, played it smoothly.

- **x86_64 builds turn on rav1d's `asm` feature** (in `torqa-video`, for that architecture
  only): the 1.1.0 package does ship its x86 assembly, it is only the ARM headers that are
  missing. It is assembled with **nasm**, which every x86_64 build already has for FFmpeg (dev
  container, Linux and Windows CI). rav1d's build script runs it through `nasm-rs`, a build
  dependency it always had: no crate is added. ARM builds stay as they are.
- **The decoder works on several frames at once**: dav1d's default frame delay instead of one
  frame. A picture then comes out a few packets after its data went in; decoding forward and
  seeking already take pictures out as they come and drain them at the end.
- On the laptop above, for that video: 21 fps → **161 fps** decoding, and a jump to a new
  moment (decoding forward from the keyframe before it, 5 s apart in that video) from ~7 s to
  ~1 s. Assembly alone gave 102 fps, frame threading alone 40 fps.
