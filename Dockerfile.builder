FROM rust:1.89

RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    zip \
    curl \
    xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Zig for cross-compilation (cargo-zigbuild uses it for linking)
ARG ZIG_VERSION=0.13.0
RUN curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-linux-x86_64-${ZIG_VERSION}.tar.xz" \
    | tar -xJ -C /opt \
    && ln -s /opt/zig-linux-x86_64-${ZIG_VERSION}/zig /usr/local/bin/zig

# cargo-zigbuild: Linux + FreeBSD cross-compilation via Zig
# cargo-xwin: Windows cross-compilation via MSVC SDK download
RUN cargo install cargo-zigbuild cargo-xwin

# Rust standard library for all non-macOS targets
RUN rustup target add \
    x86_64-unknown-linux-musl \
    aarch64-unknown-linux-musl \
    arm-unknown-linux-musleabihf \
    armv7-unknown-linux-musleabihf \
    x86_64-unknown-freebsd \
    x86_64-pc-windows-msvc \
    i686-pc-windows-msvc \
    aarch64-pc-windows-msvc
