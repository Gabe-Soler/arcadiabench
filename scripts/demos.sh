#!/usr/bin/env bash
# Reproduce the demo replays (docs/demos.md): ArcadiaOS runs kept as gzipped
# event logs for the replay viewer.
#
# Usage, from anywhere in the repository:
#
#   scripts/demos.sh [VERSION] [OUT]
#
# VERSION is v1.1 (the default) or v1.0, and OUT defaults to
# results/VERSION/replays.
#
# - v1.1: five runs on catalog 1.1 with ArcadiaOS 1.1: a nominal build, a
#   dust storm, a conjunction blackout, cascading faults, and a build with
#   the crew ArcadiaOS designs for the fleet design track
#   (results/v1.1/fleet/crew.json).
# - v1.0: the four runs on catalog 1.0. The published files were written by
#   ArcadiaBench 1.0.0 (tag v1.0.0) with ArcadiaOS 1.0: check that tag out to
#   get them byte for byte.
#
# Build ArcadiaBench and ArcadiaOS in release first. ARCADIAOS names the
# ArcadiaOS binary; it defaults to ../arcadiaos/target/release/arcadiaos, from
# the repository root. Every run is deterministic, so the logs come out byte
# for byte the same on any machine.
set -euo pipefail

here=$(pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
abs() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s\n' "$here/$1" ;; esac; }

version=${1:-v1.1}
case "$version" in
  v1.1 | v1.0) ;;
  *) echo "demos.sh: unknown results version '$version' (v1.1 or v1.0)" >&2; exit 2 ;;
esac
out=$(abs "${2:-$root/results/$version/replays}")
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

if [ "$version" = v1.1 ]; then
  crew="$root/results/v1.1/fleet/crew.json"
  "$bench" run --catalog 1.1 --seed 1 --preset medium --variant nominal --log "$runs/landing-pad-1.1-medium-nominal-1.jsonl.gz" -- "$os" </dev/null
  "$bench" run --catalog 1.1 --seed 1 --preset large --variant dust_storm --log "$runs/landing-pad-1.1-large-dust_storm-1.jsonl.gz" -- "$os" </dev/null
  "$bench" run --catalog 1.1 --seed 1 --preset medium --variant conjunction --log "$runs/landing-pad-1.1-medium-conjunction-1.jsonl.gz" -- "$os" </dev/null
  "$bench" run --catalog 1.1 --seed 13 --preset medium --variant cascading_faults --log "$runs/landing-pad-1.1-medium-cascading_faults-13.jsonl.gz" -- "$os" </dev/null
  "$bench" run --catalog 1.1 --seed 1 --crew "$crew" --variant nominal --log "$runs/landing-pad-1.1-crew.2.1.1.10-nominal-1.jsonl.gz" -- "$os" </dev/null
else
  "$bench" run --catalog 1.0 --seed 1 --preset large --variant nominal --log "$runs/landing-pad-large-nominal-1.jsonl.gz" -- "$os" </dev/null
  "$bench" run --catalog 1.0 --seed 15 --preset small --variant dust_storm --log "$runs/landing-pad-small-dust_storm-15.jsonl.gz" -- "$os" </dev/null
  "$bench" run --catalog 1.0 --seed 1 --preset medium --variant conjunction --log "$runs/landing-pad-medium-conjunction-1.jsonl.gz" -- "$os" </dev/null
  "$bench" run --catalog 1.0 --seed 13 --preset medium --variant cascading_faults --log "$runs/landing-pad-medium-cascading_faults-13.jsonl.gz" -- "$os" </dev/null
fi

mkdir -p "$out"
mv "$runs"/*.jsonl.gz "$out"/
echo "demos.sh: wrote the replays to $out"
