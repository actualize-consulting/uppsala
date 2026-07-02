#!/usr/bin/env bash
# Launch every fuzz target in parallel inside one tmux session, one window per
# target, cores split evenly across targets. Designed for a big multicore box:
# leave it running, detach, come back later and check artifacts/.
#
#   fuzz-all.sh [max_total_time_seconds]     (0 = forever, the default)
#
# Env:
#   SESSION  tmux session name (default: uppsala-fuzz)
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_tools
command -v tmux >/dev/null || { echo "tmux not found"; exit 1; }

SESSION="${SESSION:-uppsala-fuzz}"
TIME="${1:-0}"
CORES="$(nproc)"
N="${#ALL_TARGETS[@]}"
PER=$(( CORES / N ))
[ "$PER" -lt 1 ] && PER=1

if tmux has-session -t "$SESSION" 2>/dev/null; then
  echo "tmux session '$SESSION' already exists."
  echo "Attach: tmux attach -t $SESSION    Kill: tmux kill-session -t $SESSION"
  exit 1
fi

# Compile once up front so the windows start fuzzing immediately instead of
# fighting over the build lock.
"$(dirname "${BASH_SOURCE[0]}")/build.sh"

echo "Cores=$CORES  targets=$N  cores/target=$PER  time=${TIME}s"
tmux new-session -d -s "$SESSION" -n "${ALL_TARGETS[0]}"
first=1
for t in "${ALL_TARGETS[@]}"; do
  if [ "$first" -eq 0 ]; then
    tmux new-window -t "$SESSION" -n "$t"
  fi
  first=0
  tmux send-keys -t "$SESSION:$t" \
    "JOBS=$PER '$FUZZ_DIR/scripts/run.sh' $t $TIME" C-m
done

cat <<EOF
Launched $N targets x $PER cores in tmux session '$SESSION'.
  Attach:      tmux attach -t $SESSION
  Next window: Ctrl-b n     Previous: Ctrl-b p     Detach: Ctrl-b d
  Stop all:    tmux kill-session -t $SESSION
  Crashes:     $FUZZ_DIR/artifacts/<target>/
EOF
