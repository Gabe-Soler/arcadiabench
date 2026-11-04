#!/usr/bin/env bash
# Reproduce the v1.0 benchmark results (results/v1.0/RESULTS.md): ArcadiaOS
# and the three baseline coordinators on every preset and variant over seeds
# 1-50, kept as one scorecard per coordinator and scenario plus results.json.
#
# Usage, from anywhere in the repository:
#
#   scripts/results.sh [OUT]    # OUT defaults to results/v1.0
#
# Build ArcadiaBench and ArcadiaOS in release first. ARCADIAOS names the
# ArcadiaOS binary; it defaults to ../arcadiaos/target/release/arcadiaos, from
# the repository root. The runs go to a temporary directory that is removed
# afterwards. Every run is deterministic and a batch's output does not depend
# on --jobs, so the files come out byte for byte the same on any machine.
set -euo pipefail

here=$(pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
abs() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s\n' "$here/$1" ;; esac; }

out=$(abs "${1:-$root/results/v1.0}")
if [ -n "${ARCADIAOS:-}" ]; then os=$(abs "$ARCADIAOS"); else os="$root/../arcadiaos/target/release/arcadiaos"; fi
bin="$root/target/release"

for exe in "$bin/arcadiabench" "$bin/baseline-greedy" "$bin/baseline-random" "$bin/baseline-fcfs" "$os"; do
  if [ ! -x "$exe" ]; then
    echo "results.sh: $exe not found. Build ArcadiaBench and ArcadiaOS with cargo build --release, or set ARCADIAOS." >&2
    exit 1
  fi
done

runs=$(mktemp -d)
trap 'rm -rf "$runs"' EXIT

for c in arcadiaos baseline-greedy baseline-random baseline-fcfs; do
  if [ "$c" = arcadiaos ]; then coord=$os; else coord=$bin/$c; fi
  "$bin/arcadiabench" suite --seeds 1-50 --out "$runs/$c" -- "$coord" </dev/null
  mkdir -p "$out/scorecards/$c"
  for d in "$runs/$c"/*/; do
    cp "$d/scorecard.json" "$out/scorecards/$c/$(basename "$d").json"
  done
done
"$bin/arcadiabench" score --json "$runs"/*/*/metrics.jsonl >"$out/results.json" </dev/null

echo "results.sh: wrote $out/scorecards/ and $out/results.json"
