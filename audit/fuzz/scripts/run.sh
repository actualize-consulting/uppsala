#!/usr/bin/env bash
# Run ONE fuzz target, using multiple cores via libFuzzer fork mode.
#
#   run.sh <target> [max_total_time_seconds] [extra libfuzzer args...]
#
# Env:
#   JOBS     number of parallel fork workers (default: all cores)
#   MAX_LEN  max input length in bytes       (default: 16384)
#
# Fork mode (-fork=N) keeps fuzzing across crashes: each crash is saved under
# artifacts/<target>/ and the campaign continues, so this is safe to leave
# running unattended in tmux. 0 seconds = run forever (Ctrl-C to stop).
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_tools

TARGET="${1:?usage: run.sh <target> [seconds] [extra libfuzzer args...]}"
TIME="${2:-0}"
shift || true; shift || true

JOBS="${JOBS:-$(nproc)}"
MAX_LEN="${MAX_LEN:-16384}"
DICT="$(dict_for "$TARGET")"
DICT_ARG=()
[ -f "$DICT" ] && DICT_ARG=(-dict="$DICT")

# Seed the fuzzer's working corpus from the curated (tracked) seeds. `-n` never
# overwrites inputs the fuzzer has already discovered, so this is safe to repeat.
mkdir -p "$FUZZ_DIR/corpus/$TARGET"
if [ -d "$FUZZ_DIR/seeds/$TARGET" ]; then
  cp -n "$FUZZ_DIR/seeds/$TARGET/"* "$FUZZ_DIR/corpus/$TARGET/" 2>/dev/null || true
fi

cd "$REPO_ROOT"
echo ">> $TARGET  jobs=$JOBS  max_len=$MAX_LEN  time=${TIME}s  dict=$(basename "${DICT:-none}")"
exec cargo +nightly fuzz run --fuzz-dir "$FUZZ_DIR" "$TARGET" -- \
  -fork="$JOBS" \
  -ignore_crashes=1 \
  -rss_limit_mb=4096 \
  -max_len="$MAX_LEN" \
  -max_total_time="$TIME" \
  "${DICT_ARG[@]}" \
  "$@"
