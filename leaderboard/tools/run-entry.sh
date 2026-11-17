#!/usr/bin/env bash
# Run an entry's image through the benchmark and write what the leaderboard
# keeps of it.
#
#   leaderboard/tools/run-entry.sh ENTRY_JSON IMAGE OUT_DIR SEEDS [twice]
#
# ENTRY_JSON is the entry's file, IMAGE its image (a local tag or a digest),
# SEEDS a seed list such as 1-50. With `twice` every run is made twice and
# the two have to be the same byte for byte: an entry is deterministic or it
# is not ranked. A run that ends in a coordinator error (a crash, a protocol
# error or a timeout) fails the entry too.
#
# ARCADIABENCH names the arcadiabench binary (default
# target/release/arcadiabench). The catalog, the decision timeout and the
# number of runs at once come from leaderboard/config.json.
set -euo pipefail

entry=${1:?usage: run-entry.sh ENTRY_JSON IMAGE OUT_DIR SEEDS [twice]}
image=${2:?}
out=${3:?}
seeds=${4:?}
twice=${5:-}
root=$(cd "$(dirname "$0")/../.." && pwd)
bench=${ARCADIABENCH:-$root/target/release/arcadiabench}
cfg() { node -e 'const c = require(process.argv[1]); console.log(c[process.argv[2]])' "$root/leaderboard/config.json" "$1"; }
catalog=$(cfg catalog)
jobs=$(cfg jobs)
timeout=$(cfg decision_timeout_s)
export ARCADIA_RUN_BUDGET_S=${ARCADIA_RUN_BUDGET_S:-$(cfg run_budget_s)}

work=$(mktemp -d)
trap 'rm -rf "$work"; docker ps -q --filter label=arcadia-run | xargs -r docker kill >/dev/null 2>&1 || true' EXIT

tracks=$(node -e 'console.log(require(process.argv[1]).tracks.join(" "))' "$(cd "$(dirname "$entry")" && pwd)/$(basename "$entry")")
node -e 'const e = require(process.argv[1]); if (e.fleet) console.log(JSON.stringify(e.fleet.crew))' "$(cd "$(dirname "$entry")" && pwd)/$(basename "$entry")" >"$work/crew.json"

suite() { # suite PASS TRACK
  local dir="$work/$1/$2"
  if [ "$2" = fleet ]; then set -- --crew "$work/crew.json"; else set --; fi
  "$bench" suite --catalog "$catalog" "$@" --seeds "$seeds" --jobs "$jobs" --decision-timeout "$timeout" \
    --out "$dir" -- "$root/leaderboard/tools/sandbox.sh" "$image" >"$dir.txt" 2>"$dir.err" </dev/null || {
    echo "run-entry.sh: the suite failed; its last lines:" >&2; tail -5 "$dir.err" >&2; exit 1; }
}

passes="a"; [ "$twice" = twice ] && passes="a b"
dirs=()
for pass in $passes; do
  mkdir -p "$work/$pass"
  for track in $tracks; do
    case "$track" in standard | fleet) suite "$pass" "$track" ;; *) echo "run-entry.sh: unknown track" >&2; exit 2 ;; esac
    [ "$pass" = a ] && dirs+=("$work/a/$track")
  done
done

if [ "$twice" = twice ]; then
  for track in $tracks; do
    diff -r -q "$work/a/$track" "$work/b/$track" >/dev/null || {
      echo "run-entry.sh: not deterministic: two runs of the $track track on the same seeds differ" >&2; exit 1; }
  done
fi

errors=$(node -e '
  let n = 0;
  for (const dir of process.argv.slice(1)) {
    for (const card of require(dir + "/suite.json").scorecards) n += card.completion.end_reasons.coordinator_error + card.completion.end_reasons.simulator_error;
  }
  console.log(n);' "${dirs[@]}")
if [ "$errors" != 0 ]; then
  echo "run-entry.sh: $errors runs ended in an error (a crash, a protocol error or a timeout)" >&2
  exit 1
fi

node "$root/site/tools/entry-results.mjs" "$entry" "$out" "${dirs[@]}"
