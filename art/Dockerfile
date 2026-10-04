# Torqa art container: headless Blender for the scripted models (ADR 0009). Blender publishes
# Linux builds for x86-64 only, so this image is x86-64; on Apple Silicon Docker runs it
# emulated, which scripted modelling tolerates well.
FROM --platform=linux/amd64 debian:13.7-slim

ARG BLENDER_VERSION=5.2.2
ARG BLENDER_SHA256=84098912789dc450e95697c4184fb8a90acbe5111c2ba4aede3fecb57806a168

# Blender links against these X11/GL/Wayland client libraries even when run in the background.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl xz-utils \
        libx11-6 libxi6 libxxf86vm1 libxfixes3 libxrender1 libxkbcommon0 libsm6 libice6 \
        libgl1 libegl1 libwayland-client0 libwayland-cursor0 libwayland-egl1 libdecor-0-0 \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /tmp/blender.tar.xz \
        "https://download.blender.org/release/Blender${BLENDER_VERSION%.*}/blender-${BLENDER_VERSION}-linux-x64.tar.xz" \
    && echo "${BLENDER_SHA256}  /tmp/blender.tar.xz" | sha256sum -c - \
    && tar -xJf /tmp/blender.tar.xz -C /opt \
    && mv "/opt/blender-${BLENDER_VERSION}-linux-x64" /opt/blender \
    && ln -s /opt/blender/blender /usr/local/bin/blender \
    && rm /tmp/blender.tar.xz

WORKDIR /workspaces/torqa
