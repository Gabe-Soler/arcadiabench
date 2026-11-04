#!/usr/bin/env bash
# Reproduce the four demo replays (docs/demos.md): ArcadiaOS runs of a nominal
# build, a dust storm, a conjunction blackout and cascading faults, kept as
# gzipped event logs for the replay viewer.
#
# Usage, from anywhere in the repository:
#
#   scripts/demos.sh [OUT]    # OUT defaults to results/v1.0/replays
#
# Build ArcadiaBench and ArcadiaOS in release first. ARCADIAOS names the
# ArcadiaOS binary; it defaults to ../arcadiaos/target/release/arcadiaos, from
# the repository root. The runs are written to a temporary directory and moved
# into OUT once all four are done. Runs are deterministic and the gzip stream
# carries no time or name, so the logs come out byte for byte the same.
set -euo pipefail

here=$(pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
abs() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s\n' "$here/$1" ;; esac; }

out=$(abs "${1:-$root/results/v1.0/replays}")
if [ -n "${ARCADIAOS:-}" ]; then os=$(abs "$ARCADIAOS"); else os="$root/../arcadiaos/target/release/arcadiaos"; fi
bench="$root/target/release/arcadiabench"

for exe in "$bench" "$os"; do
  if [ ! -x "$exe" ]; then
    echo "demos.sh: $exe not found. Build ArcadiaBench and ArcadiaOS with cargo build --release, or set ARCADIAOS." >&2
    exit 1
  fi
done

runs=$(mktemp -d)
trap 'rm -rf "$runs"' EXIT

"$bench" run --seed 1 --preset large --variant nominal --log "$runs/landing-pad-large-nominal-1.jsonl.gz" -- "$os" </dev/null
"$bench" run --seed 15 --preset small --variant dust_storm --log "$runs/landing-pad-small-dust_storm-15.jsonl.gz" -- "$os" </dev/null
"$bench" run --seed 1 --preset medium --variant conjunction --log "$runs/landing-pad-medium-conjunction-1.jsonl.gz" -- "$os" </dev/null
"$bench" run --seed 13 --preset medium --variant cascading_faults --log "$runs/landing-pad-medium-cascading_faults-13.jsonl.gz" -- "$os" </dev/null

mkdir -p "$out"
mv "$runs"/*.jsonl.gz "$out"/
echo "demos.sh: wrote the four replays to $out"
