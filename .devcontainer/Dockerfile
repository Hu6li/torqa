# Torqa development container: everything needed to build, lint and test lives here.
FROM rust:1.99.0-trixie

ARG GODOT_VERSION=4.7.2
ARG CARGO_DENY_VERSION=0.20.2
ARG GDTOOLKIT_VERSION=4.5.0

# libdbus: btleplug's Linux backend talks to BlueZ over D-Bus.
# libfontconfig: needed by the Godot binary even in headless mode.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libdbus-1-dev pkg-config libfontconfig1 pipx unzip ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

RUN arch="$(uname -m)" \
    && curl -fsSL "https://github.com/EmbarkStudios/cargo-deny/releases/download/${CARGO_DENY_VERSION}/cargo-deny-${CARGO_DENY_VERSION}-${arch}-unknown-linux-musl.tar.gz" \
        | tar -xz -C /tmp \
    && mv "/tmp/cargo-deny-${CARGO_DENY_VERSION}-${arch}-unknown-linux-musl/cargo-deny" /usr/local/cargo/bin/ \
    && rm -rf /tmp/cargo-deny-*

RUN PIPX_HOME=/opt/pipx PIPX_BIN_DIR=/usr/local/bin pipx install "gdtoolkit==${GDTOOLKIT_VERSION}"

RUN case "$(uname -m)" in \
        x86_64) godot_arch=x86_64 ;; \
        aarch64) godot_arch=arm64 ;; \
        *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o /tmp/godot.zip \
        "https://github.com/godotengine/godot/releases/download/${GODOT_VERSION}-stable/Godot_v${GODOT_VERSION}-stable_linux.${godot_arch}.zip" \
    && unzip -q /tmp/godot.zip -d /tmp/godot \
    && mv "/tmp/godot/Godot_v${GODOT_VERSION}-stable_linux.${godot_arch}" /usr/local/bin/godot \
    && rm -rf /tmp/godot /tmp/godot.zip

WORKDIR /workspaces/torqa
