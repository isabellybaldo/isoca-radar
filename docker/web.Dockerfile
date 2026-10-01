# syntax=docker/dockerfile:1
# Build context: repository root.  docker compose -f compose.yml -f compose.web.yml --profile web build web
# Phase 7b (09-27 §4.2): arb-wasm first, on the same pinned Rust image as rust.Dockerfile, with cargo-chef so
# a web-only change reuses the cooked dependencies; then the node stage builds the dashboard and /math.
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS wasm-chef
RUN rustup target add wasm32-unknown-unknown && cargo install cargo-chef --locked
WORKDIR /src
# the CLI must equal the wasm-bindgen crate Cargo.lock pins: scripts/wasm-bindgen-version.sh is the one source
# (it is also what scripts/build-wasm.sh and the CI web job read); only these two files key this cached layer
COPY Cargo.lock ./
COPY scripts/wasm-bindgen-version.sh scripts/
RUN cargo install --locked --version "$(sh scripts/wasm-bindgen-version.sh)" wasm-bindgen-cli

FROM wasm-chef AS wasm-planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM wasm-chef AS wasm
# The cook must compile the dependencies exactly as scripts/build-wasm.sh will, or cargo rebuilds them: the same
# size-optimized release profile (the script's defaults) and the same path remaps it puts in RUSTFLAGS.
ENV CARGO_PROFILE_RELEASE_OPT_LEVEL=s CARGO_PROFILE_RELEASE_LTO=true CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
COPY --from=wasm-planner /src/recipe.json recipe.json
RUN RUSTFLAGS="--remap-path-prefix=${CARGO_HOME}=/cargo --remap-path-prefix=/src=/src"     cargo chef cook --release --target wasm32-unknown-unknown --recipe-path recipe.json --package arb-wasm
COPY . .
RUN sh scripts/build-wasm.sh

FROM node:22-alpine@sha256:0a7108bf6c7bf5de370ffb1a3ed6be93d405b43ff159f681a8d18c0e2bc2e402 AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
COPY --from=wasm /src/web/src/wasm/pkg ./src/wasm/pkg
RUN npm run build

# Unprivileged nginx (uid 101, port 8080): runs with a read-only root filesystem plus a /tmp tmpfs.
FROM nginxinc/nginx-unprivileged:1.29-alpine@sha256:0c79d56aee561a1d81c63f00eee5fb5fe29279560cdc55e91425133104c7fbe6
COPY docker/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /web/dist /usr/share/nginx/html
# 7b §5.3: mount point of the api's snapshot volume (read-only here); a read-only root filesystem needs it in the image
USER root
RUN mkdir -p /srv/ct-public-root/math-data
USER 101
EXPOSE 8080
