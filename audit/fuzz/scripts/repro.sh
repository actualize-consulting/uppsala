#!/usr/bin/env bash
# Re-run a single crashing input under the target (with ASan) to reproduce and
# get a stack trace.
#
#   repro.sh <target> <path-to-artifact>
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_tools
TARGET="${1:?usage: repro.sh <target> <artifact>}"
ARTIFACT="${2:?usage: repro.sh <target> <artifact>}"
cd "$REPO_ROOT"
exec cargo +nightly fuzz run --fuzz-dir "$FUZZ_DIR" "$TARGET" "$ARTIFACT"
