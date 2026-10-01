#!/bin/sh
# Prints the wasm-bindgen version locked in Cargo.lock: the one source for build-wasm.sh, the CI web
# job and docker/web.Dockerfile (the CLI must match the crate exactly).
set -eu
cd "$(dirname "$0")/.."
awk '/^name = "wasm-bindgen"/ { getline; gsub(/[^0-9.]/, ""); print; exit }' Cargo.lock
