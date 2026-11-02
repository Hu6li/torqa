# ORQWA development container: everything needed to build, lint and test lives here.
FROM rust:1.99.0-trixie

ARG GODOT_VERSION=4.7.2
ARG CARGO_DENY_VERSION=0.20.2
ARG GDTOOLKIT_VERSION=4.5.0
# Fuzzing only (scripts/fuzz.sh): libFuzzer instrumentation needs nightly. Keep the date equal to
# core/fuzz/rust-toolchain.toml.
ARG RUST_FUZZ_TOOLCHAIN=nightly-2026-10-31
ARG CARGO_FUZZ_VERSION=0.13.2
# Checksums of the downloads below, from the releases' SHA512-SUMS.txt (Godot) and .sha256
# files (cargo-deny): a swapped download fails the build rather than ending up in the image.
ARG GODOT_SHA512_X86_64=9aa00f7a605200940bce3027a567b782f49bd8e940dd06ae9e987bd65aee1b1467edd56ed84fcdcbdd44354bf613bdbb4e5d2913e925850368e150c59ed54c65
ARG GODOT_SHA512_ARM64=dd59918da086bd49bde2f5450b5e567ff8650cbde9abbd7b8f4ca1197ff8c609baa38834666d032deafb47099078d7822279e2a0e06e5665745468f26533e7e2
ARG CARGO_DENY_SHA256_X86_64=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
ARG CARGO_DENY_SHA256_AARCH64=995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d

# libdbus: btleplug's Linux backend talks to BlueZ over D-Bus.
# libfontconfig: needed by the Godot binary even in headless mode.
# mesa-vulkan-drivers (lavapipe) + xvfb: software rendering for 3D screenshots without a GPU.
# libclang-dev: ffmpeg-sys-next's bindgen. nasm: FFmpeg's assembly (built below).
# gcc-mingw-w64-x86-64: the Windows extension for native GPU renders (ADR 0021).
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libdbus-1-dev pkg-config libfontconfig1 pipx unzip ca-certificates \
        mesa-vulkan-drivers xvfb xauth libxcursor1 libxinerama1 libxrandr2 libxi6 libgl1 \
        libclang-dev nasm gcc-mingw-w64-x86-64 \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy && rustup target add x86_64-pc-windows-gnu

RUN rustup toolchain install "$RUST_FUZZ_TOOLCHAIN" --profile minimal \
    && cargo install cargo-fuzz --version "$CARGO_FUZZ_VERSION" --locked \
    && rm -rf /usr/local/cargo/registry

RUN arch="$(uname -m)" \
    && case "$arch" in \
        x86_64) sha256="$CARGO_DENY_SHA256_X86_64" ;; \
        aarch64) sha256="$CARGO_DENY_SHA256_AARCH64" ;; \
        *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o /tmp/cargo-deny.tar.gz "https://github.com/EmbarkStudios/cargo-deny/releases/download/${CARGO_DENY_VERSION}/cargo-deny-${CARGO_DENY_VERSION}-${arch}-unknown-linux-musl.tar.gz" \
    && echo "$sha256  /tmp/cargo-deny.tar.gz" | sha256sum -c - \
    && tar -xz -C /tmp -f /tmp/cargo-deny.tar.gz \
    && mv "/tmp/cargo-deny-${CARGO_DENY_VERSION}-${arch}-unknown-linux-musl/cargo-deny" /usr/local/cargo/bin/ \
    && rm -rf /tmp/cargo-deny*

RUN PIPX_HOME=/opt/pipx PIPX_BIN_DIR=/usr/local/bin pipx install "gdtoolkit==${GDTOOLKIT_VERSION}"

RUN case "$(uname -m)" in \
        x86_64) godot_arch=x86_64; sha512="$GODOT_SHA512_X86_64" ;; \
        aarch64) godot_arch=arm64; sha512="$GODOT_SHA512_ARM64" ;; \
        *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o /tmp/godot.zip \
        "https://github.com/godotengine/godot/releases/download/${GODOT_VERSION}-stable/Godot_v${GODOT_VERSION}-stable_linux.${godot_arch}.zip" \
    && echo "$sha512  /tmp/godot.zip" | sha512sum -c - \
    && unzip -q /tmp/godot.zip -d /tmp/godot \
    && mv "/tmp/godot/Godot_v${GODOT_VERSION}-stable_linux.${godot_arch}" /usr/local/bin/godot \
    && rm -rf /tmp/godot /tmp/godot.zip

# FFmpeg's libraries for orqwa-video (ADR 0010), from the pinned release the script verifies;
# ffmpeg-sys-next finds them through FFMPEG_DIR instead of building its own. The Windows build
# is for scripts/build-gdext.sh's Windows extension (ADR 0021).
COPY scripts/build-ffmpeg.sh /tmp/build-ffmpeg.sh
RUN sh /tmp/build-ffmpeg.sh /opt/ffmpeg \
    && sh /tmp/build-ffmpeg.sh /opt/ffmpeg-windows windows \
    && rm /tmp/build-ffmpeg.sh
ENV FFMPEG_DIR=/opt/ffmpeg

# cargo-about writes the crates' licence notices the app shows (scripts/notices.sh, ADR 0018).
# Declared after FFmpeg so that adding or bumping it keeps the slow FFmpeg layer cached;
# checksums from the release's .sha256 files.
ARG CARGO_ABOUT_VERSION=0.9.2
ARG CARGO_ABOUT_SHA256_X86_64=9099a59e820c38a68b9d65f300662a567d56562f9a10f6aa4c7e86c17c2566af
ARG CARGO_ABOUT_SHA256_AARCH64=af5169282fb6f84e13471493f405437e43ac517744c9ae12fbe2cdf0a6f0e5a8
RUN arch="$(uname -m)" \
    && case "$arch" in \
        x86_64) sha256="$CARGO_ABOUT_SHA256_X86_64" ;; \
        aarch64) sha256="$CARGO_ABOUT_SHA256_AARCH64" ;; \
        *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o /tmp/cargo-about.tar.gz "https://github.com/EmbarkStudios/cargo-about/releases/download/${CARGO_ABOUT_VERSION}/cargo-about-${CARGO_ABOUT_VERSION}-${arch}-unknown-linux-musl.tar.gz" \
    && echo "$sha256  /tmp/cargo-about.tar.gz" | sha256sum -c - \
    && tar -xz -C /tmp -f /tmp/cargo-about.tar.gz \
    && mv "/tmp/cargo-about-${CARGO_ABOUT_VERSION}-${arch}-unknown-linux-musl/cargo-about" /usr/local/cargo/bin/ \
    && rm -rf /tmp/cargo-about*

# Voice-chat development: ALSA headers for CPAL and CMake for bundled, hardened libopus.
# Kept after FFmpeg so these additions preserve the expensive video-build cache.
RUN apt-get update \
    && apt-get install -y --no-install-recommends libasound2-dev cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspaces/orqwa
