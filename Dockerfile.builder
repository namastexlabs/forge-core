# ============================================
# Forge Builder Image
# High-performance build environment for forge-core and automagik-forge
# ============================================
# Base: Debian bookworm-slim
# Includes: Rust nightly-2025-05-18, Node 22, pnpm 10.8.1, cargo-zigbuild, Zig 0.13.0
# Optimized for: CT 200 self-hosted runners (12 cores, 12GB RAM)
# Registry: ghcr.io/namastexlabs/forge-builder:nightly-2025-05-18
# ============================================

# ============================================
# Stage 1: Base System (forge-base)
# ============================================
FROM debian:bookworm-slim AS forge-base

# Install system build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    clang \
    libclang-dev \
    ca-certificates \
    curl \
    git \
    perl \
    pkg-config \
    libssl-dev \
    gnupg \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 22 from NodeSource
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install pnpm 10.8.1 globally
RUN npm install -g pnpm@10.8.1

# Verify installations
RUN node --version && npm --version && pnpm --version

# ============================================
# Stage 2: Rust Toolchain (forge-rust)
# ============================================
FROM forge-base AS forge-rust

# Install rustup + nightly-2025-05-18
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    --default-toolchain nightly-2025-05-18 \
    --component rustfmt,clippy,rust-analyzer,rust-src

ENV PATH="/root/.cargo/bin:${PATH}"

# Verify Rust installation
RUN rustc --version && cargo --version

# Add common Linux targets (cache-bust: force fresh install)
RUN echo "Installing targets: $(date -u +%Y-%m-%dT%H:%M:%SZ)" && rustup target add \
    x86_64-unknown-linux-gnu \
    x86_64-unknown-linux-musl \
    aarch64-unknown-linux-gnu \
    aarch64-unknown-linux-musl

# Install Zig 0.13.0 (required by cargo-zigbuild)
RUN curl -L https://ziglang.org/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz \
    | tar -xJ -C /usr/local \
    && ln -s /usr/local/zig-linux-x86_64-0.13.0/zig /usr/local/bin/zig

# Verify Zig installation
RUN zig version

# Install cargo-zigbuild (expensive, cache this layer)
RUN cargo install cargo-zigbuild --locked

# Verify cargo-zigbuild installation
RUN cargo zigbuild --help | head -1

# ============================================
# Stage 3: Build Optimizations (forge-builder)
# ============================================
FROM forge-rust AS forge-builder

# Rust build config optimized for CT 200 (12 cores)
ENV CARGO_BUILD_JOBS=12
ENV CARGO_INCREMENTAL=1
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

# pnpm store configuration
RUN pnpm config set store-dir /root/.pnpm-store

# Pre-create directories for caching
RUN mkdir -p /workspace /root/.cargo/registry /root/.pnpm-store

WORKDIR /workspace

# ============================================
# Stage 4: CI Tools (ci-builder)
# ============================================
FROM forge-builder AS ci-builder

# Install gh CLI for GitHub operations
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
    | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \
    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
    | tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
    && apt-get update \
    && apt-get install -y gh jq \
    && rm -rf /var/lib/apt/lists/*

# Verify gh CLI installation
RUN gh --version && jq --version

# Set shell and entrypoint
SHELL ["/bin/bash", "-c"]
ENTRYPOINT ["/bin/bash"]

# ============================================
# Metadata
# ============================================
LABEL org.opencontainers.image.source="https://github.com/namastexlabs/forge-core"
LABEL org.opencontainers.image.description="High-performance build environment for Forge ecosystem"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.version="nightly-2025-05-18"
LABEL maintainer="Namaste Labs <hello@namastex.io>"

# ============================================
# Image size estimate: ~1.2GB
# Build command: DOCKER_BUILDKIT=1 docker build -f Dockerfile.builder -t ghcr.io/namastexlabs/forge-builder:nightly-2025-05-18 .
# ============================================
