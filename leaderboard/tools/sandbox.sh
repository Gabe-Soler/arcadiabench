#!/usr/bin/env bash
# One run's coordinator, in its own container. This is the coordinator
# command of every leaderboard run:
#
#   arcadiabench suite ... -- leaderboard/tools/sandbox.sh IMAGE
#
# The container gets the protocol on standard input and output, and nothing
# else: no network, a read-only file system with a small /tmp, no
# capabilities, an unprivileged user, and limits on memory, CPU and
# processes. Its standard error is dropped.
#
# A run has a wall-clock budget, ARCADIA_RUN_BUDGET_S seconds (900 by
# default). A watchdog in its own session kills the container when the
# budget is spent, even if this script has been killed first: the simulator
# kills its coordinator on a decision timeout, and killing the `docker run`
# client alone would leave the container running.
set -euo pipefail

image=${1:?usage: sandbox.sh IMAGE}
budget=${ARCADIA_RUN_BUDGET_S:-900}
case "$budget" in '' | *[!0-9]*) echo "sandbox.sh: bad ARCADIA_RUN_BUDGET_S" >&2; exit 2 ;; esac
name="arcadia-run-$$-${RANDOM}${RANDOM}"

setsid sh -c "sleep $budget; docker kill $name" >/dev/null 2>&1 </dev/null &
watchdog=$!
trap 'kill -- "-$watchdog" 2>/dev/null || true' EXIT

docker run --rm -i \
  --name "$name" --label arcadia-run=1 \
  --network none \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=64m \
  --cap-drop ALL --security-opt no-new-privileges \
  --user 65534:65534 \
  --memory 2g --memory-swap 2g --cpus 1 --pids-limit 256 \
  --log-driver none \
  "$image" 2>/dev/null
