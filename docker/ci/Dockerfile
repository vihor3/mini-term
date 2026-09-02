# syntax=docker/dockerfile:1.7

FROM rust:1.95-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive

# Keep this list aligned with the Linux dependency step in .github/workflows/ci.yml.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        clang \
        cmake \
        git \
        libasound2-dev \
        libfontconfig1-dev \
        libfreetype6-dev \
        libwayland-dev \
        libx11-dev \
        libxcb1-dev \
        libxkbcommon-dev \
        libxkbcommon-x11-dev \
        nodejs \
        perl \
        pkg-config \
        python3 \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy rustfmt \
    && git config --system --add safe.directory /workspace

ENV CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/target \
    CARGO_INCREMENTAL=0 \
    CARGO_TERM_COLOR=always \
    HOME=/tmp \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

WORKDIR /workspace
CMD ["bash"]
