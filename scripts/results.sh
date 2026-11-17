#!/usr/bin/env bash
# Reproduce the published benchmark results: ArcadiaOS and the three baseline
# coordinators on every preset and variant of a catalog over seeds 1-50, kept
# as one scorecard per coordinator and scenario plus results.json.
#
# Usage, from anywhere in the repository:
#
#   scripts/results.sh [VERSION] [OUT]
#
# VERSION is v1.1 (the default) or v1.0, and OUT defaults to results/VERSION.
#
# - v1.1 (results/v1.1/RESULTS.md) is catalog 1.1, with ArcadiaOS 1.1. It
#   also runs the fleet design track: ArcadiaOS designs a crew for the
#   catalog's mass budget (OUT/fleet/crew.json), and that crew goes through
#   every variant with ArcadiaOS and, for comparison, with the greedy
#   baseline.
# - v1.0 (results/v1.0/RESULTS.md) is catalog 1.0. The published files were
#   written by ArcadiaBench 1.0.0 (tag v1.0.0) with ArcadiaOS 1.0: check that
#   tag out to get them byte for byte. A later ArcadiaBench runs the same
#   catalog 1.0 scenarios and adds what its version adds to a scorecard, and
#   it needs a coordinator that speaks its protocol version.
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

version=${1:-v1.1}
case "$version" in
  v1.1) catalog=1.1 ;;
  v1.0) catalog=1.0 ;;
  *) echo "results.sh: unknown results version '$version' (v1.1 or v1.0)" >&2; exit 2 ;;
esac
out=$(abs "${2:-$root/results/$version}")
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

# Copy the scorecards of one suite to OUT/scorecards/<coordinator>/.
keep() {
  mkdir -p "$out/scorecards/$1"
  for d in "$2"/*/; do
    cp "$d/scorecard.json" "$out/scorecards/$1/$(basename "$d").json"
  done
}

for c in arcadiaos baseline-greedy baseline-random baseline-fcfs; do
  if [ "$c" = arcadiaos ]; then coord=$os; else coord=$bin/$c; fi
  "$bin/arcadiabench" suite --catalog "$catalog" --seeds 1-50 --out "$runs/$c" -- "$coord" </dev/null
  keep "$c" "$runs/$c"
done

if [ "$version" = v1.1 ]; then
  # The fleet design track. Every crew works from the Medium outpost, so
  # that is the scenario ArcadiaOS designs for; 10 t is catalog 1.1's budget.
  mkdir -p "$out/fleet"
  "$bin/arcadiabench" scenario --catalog 1.1 --preset medium --seed 1 --goal >"$runs/scenario.jsonl"
  "$os" design-crew --scenario "$runs/scenario.jsonl" --budget-kg 10000 >"$out/fleet/crew.json"
  "$bin/arcadiabench" crew --catalog 1.1 "$out/fleet/crew.json"
  for c in arcadiaos baseline-greedy; do
    if [ "$c" = arcadiaos ]; then coord=$os; else coord=$bin/$c; fi
    "$bin/arcadiabench" suite --catalog 1.1 --crew "$out/fleet/crew.json" --seeds 1-50 --out "$runs/fleet-$c" -- "$coord" </dev/null
    keep "$c" "$runs/fleet-$c"
  done
fi
"$bin/arcadiabench" score --json "$runs"/*/*/metrics.jsonl >"$out/results.json" </dev/null

echo "results.sh: wrote $out/scorecards/ and $out/results.json"
