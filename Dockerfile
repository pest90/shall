# syntax=docker/dockerfile:1
#
# Targets:
#   dev      toolchain for interactive work, used by compose.yaml
#   check    runs scripts/check.sh (fmt, clippy, tests, rustdoc, MSRV);
#            the build fails if any check fails
#   runtime  (default) minimal image with the `shall` binary
#
# The Rust toolchain comes from rust-toolchain.toml and the MSRV from
# `rust-version` in Cargo.toml, so the image cannot drift from them.

ARG DEBIAN_RELEASE=trixie

FROM debian:${DEBIAN_RELEASE}-slim AS toolchain
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev git \
 && rm -rf /var/lib/apt/lists/* \
 && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none
WORKDIR /work
COPY rust-toolchain.toml Cargo.toml ./
RUN rustup toolchain install \
 && msrv=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml) \
 && rustup toolchain install "$msrv" --profile minimal \
 && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME" \
 && rm rust-toolchain.toml Cargo.toml

FROM toolchain AS dev
ARG UID=1000
ARG GID=1000
# For scripts/prepare-release.sh; the same version as in release.yml.
ARG GIT_CLIFF_VERSION=2.14.2
COPY rust-toolchain.toml ./
RUN cargo install --locked --no-default-features --version "${GIT_CLIFF_VERSION}" git-cliff \
 && rm -rf rust-toolchain.toml "$CARGO_HOME/registry"
# --non-unique: the host IDs may already exist in the image, e.g. GID 20
# (staff on macOS, dialout on Debian) or GID 100 (users).
RUN groupadd --non-unique --gid "${GID}" dev \
 && useradd --non-unique --uid "${UID}" --gid "${GID}" --create-home dev \
 && mkdir -p /home/dev/.cargo/registry /home/dev/target \
 && chown -R dev:dev /home/dev /work
# Build output stays outside the bind-mounted repository, so Docker never
# creates a root-owned target/ in the working tree.
ENV CARGO_HOME=/home/dev/.cargo \
    CARGO_TARGET_DIR=/home/dev/target \
    PATH=/home/dev/.cargo/bin:/usr/local/cargo/bin:$PATH
USER dev
CMD ["bash"]

FROM toolchain AS check
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/work/target \
    scripts/check.sh

FROM toolchain AS build
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/work/target \
    cargo build --release --locked -p shall-cli \
 && cp target/release/shall /usr/local/bin/shall

FROM debian:${DEBIAN_RELEASE}-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/shall /usr/local/bin/shall
WORKDIR /project
ENTRYPOINT ["shall"]
