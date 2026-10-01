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
