#!/usr/bin/env bash
# 7b §3.3: the sim extraction changes no paper row. Replays one capture directory at BASE (default
# main) and at the working tree with the same harness (bins/scanner/examples/replay_dump.rs, copied
# into a temporary worktree of BASE) and the same scanner.toml and policies.toml, then compares the
# dumps: in emission order and sorted. Prints IDENTICAL and exits 0, or DIFFERENT and exits 1.
#   scripts/replay-identity.sh <capture dir> [base ref]
set -euo pipefail
CAP="$(cd "$1" && pwd)"
BASE="${2:-main}"
ROOT="$(git rev-parse --show-toplevel)"
WORK="$(mktemp -d)"
cleanup() {
  git -C "$ROOT" worktree remove --force "$WORK/base" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT
git -C "$ROOT" worktree add --detach "$WORK/base" "$BASE" >/dev/null
mkdir -p "$WORK/base/bins/scanner/examples"
cp "$ROOT/bins/scanner/examples/replay_dump.rs" "$WORK/base/bins/scanner/examples/replay_dump.rs"
# One target dir for both builds: the dependencies compile once.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
for side in base head; do
  if [ "$side" = base ]; then SRC="$WORK/base"; else SRC="$ROOT"; fi
  echo "== $side: $(git -C "$SRC" log -1 --format='%h %s')"
  (cd "$SRC" && cargo run -q -p scanner --example replay_dump -- \
    "$CAP" "$ROOT/config/scanner.toml" "$ROOT/config/policies.toml" "$WORK/$side.txt")
done
ROWS="$(wc -l < "$WORK/head.txt" | tr -d ' ')"
if [ "$ROWS" -eq 0 ]; then
  echo "DIFFERENT: no paper rows at all, so this hour proves nothing; pick a busier hour"
  exit 1
fi
for side in base head; do
  sort "$WORK/$side.txt" > "$WORK/$side.sorted"
  echo "$side rows $(wc -l < "$WORK/$side.txt" | tr -d ' ') order $(sha256sum < "$WORK/$side.txt" | cut -c1-16) sorted $(sha256sum < "$WORK/$side.sorted" | cut -c1-16)"
done
if cmp -s "$WORK/base.txt" "$WORK/head.txt" && cmp -s "$WORK/base.sorted" "$WORK/head.sorted"; then
  echo IDENTICAL
else
  diff "$WORK/base.txt" "$WORK/head.txt" | head -20 || true
  echo DIFFERENT
  exit 1
fi
