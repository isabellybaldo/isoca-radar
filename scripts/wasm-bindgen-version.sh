#!/bin/sh
# Prints the wasm-bindgen version locked in Cargo.lock: the one source for build-wasm.sh, the CI web
# job and docker/web.Dockerfile (the CLI must match the crate exactly). Exits 1 unless exactly one
# wasm-bindgen package is locked.
set -eu
cd "$(dirname "$0")/.."
# Cargo.lock is CRLF in a Windows checkout and the Docker build context copies that working tree: drop the CR first
VERSIONS=$(awk '{ sub(/\r$/, "") } /^name = "wasm-bindgen"$/ { getline; gsub(/[^0-9.]/, ""); print }' Cargo.lock)
if [ -z "$VERSIONS" ] || [ "$(printf '%s
' "$VERSIONS" | wc -l)" -ne 1 ]; then
  echo "wasm-bindgen-version: expected exactly one wasm-bindgen in Cargo.lock, found: ${VERSIONS:-none}" >&2
  exit 1
fi
printf '%s
' "$VERSIONS"
