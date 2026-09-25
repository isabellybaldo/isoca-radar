#!/bin/sh
# Linux/VPS twin of disk-budget.ps1: everything cute-trading puts on disk, fails above 30 GB.
# Run as root from the deploy folder:  sh scripts/disk-budget.sh
# Exit code: 0 within budget, 1 over budget.
set -eu
LIMIT=${LIMIT_GB:-30}
DIR=$(cd "$(dirname "$0")/.." && pwd)

total=0
row() { printf '%-60s %6s GB\n' "$1" "$(awk "BEGIN{printf \"%.2f\", $2/1e9}")"; total=$((total + $2)); }

row "deploy folder $DIR" "$(du -sb "$DIR" | cut -f1)"
for v in /var/lib/docker/volumes/cute-trading_*; do
  [ -d "$v" ] && row "volume $(basename "$v")" "$(du -sb "$v" | cut -f1)"
done
for img in cute-trading/rust:dev "$(sed -n 's/^ *image: *\(postgres:[^ ]*\).*/\1/p' "$DIR/compose.yml" | head -n1)"; do
  size=$(docker image inspect -f '{{.Size}}' "$img" 2>/dev/null) && row "image ${img%%@*}" "$size"
done
row "container log caps (3 x 50 MB)" 150000000

awk "BEGIN{printf \"total %.2f GB of $LIMIT GB\n\", $total/1e9}"
if [ "$total" -gt $((LIMIT * 1000000000)) ]; then echo "OVER BUDGET: stop the scanner and prune"; exit 1; fi
