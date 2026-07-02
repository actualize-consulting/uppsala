#!/usr/bin/env bash
# Minimize a target's corpus (drop inputs that add no coverage). Run periodically
# during long campaigns to keep the corpus small and fast.
#
#   minimize.sh <target>
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_tools
TARGET="${1:?usage: minimize.sh <target>}"
cd "$REPO_ROOT"
exec cargo +nightly fuzz cmin --fuzz-dir "$FUZZ_DIR" "$TARGET"
