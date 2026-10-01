#!/bin/sh
# Phase 7b-B (09-27 §4.2): build crates/arb-wasm for the browser into web/src/wasm/pkg (gitignored).
# PROFILE=release (Docker, CI and `npm run build`; the default) or PROFILE=dev (the PC's test loop).
# No wasm-pack, no wasm-opt (09-27 §4.2); the release build is size-optimized through cargo's
# per-invocation profile variables, which touch only this wasm32 build (its own target directory).
# It checks the wasm-bindgen CLI against Cargo.lock and never installs anything.
set -eu
cd "$(dirname "$0")/.."
WANT=$(sh scripts/wasm-bindgen-version.sh) || exit 1
HAVE=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }')
if [ "$HAVE" != "$WANT" ]; then
  echo "build-wasm: the wasm-bindgen CLI is '${HAVE:-missing}', Cargo.lock pins $WANT: cargo install wasm-bindgen-cli --locked --version $WANT" >&2
  exit 1
fi
# Panic messages embed source paths: remap the cargo home and the repo root so the .wasm (published on the
# public page) never carries the build machine's paths. Native (Windows) and POSIX spellings of both are
# remapped; RUSTFLAGS already in the environment are kept. The remap changes cargo's fingerprint, so the first
# build after a change here recompiles. (Paths with spaces would need CARGO_ENCODED_RUSTFLAGS.)
ROOT=$(pwd)
CH="${CARGO_HOME:-$HOME/.cargo}"
remap() { [ -n "$1" ] && RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$1=$2"; return 0; }
remap "$CH" /cargo
remap "$ROOT" /src
if command -v cygpath >/dev/null 2>&1; then
  remap "$(cygpath -m "$CH")" /cargo
  remap "$(cygpath -w "$CH")" /cargo
  remap "$(cygpath -m "$ROOT")" /src
  remap "$(cygpath -w "$ROOT")" /src
fi
export RUSTFLAGS
PROFILE="${PROFILE:-release}"
if [ "$PROFILE" != release ] && [ "$PROFILE" != dev ]; then
  echo "build-wasm: PROFILE must be release or dev, got '$PROFILE'" >&2
  exit 1
fi
if [ "$PROFILE" = release ]; then
  export CARGO_PROFILE_RELEASE_OPT_LEVEL="${CARGO_PROFILE_RELEASE_OPT_LEVEL:-s}"
  export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-true}"
  export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"
  cargo build -p arb-wasm --target wasm32-unknown-unknown --release
  dir=release
else
  cargo build -p arb-wasm --target wasm32-unknown-unknown
  dir=debug
fi
rm -rf web/src/wasm/pkg
wasm-bindgen --target web --out-dir web/src/wasm/pkg --out-name arb_wasm "target/wasm32-unknown-unknown/$dir/arb_wasm.wasm"
ls web/src/wasm/pkg
