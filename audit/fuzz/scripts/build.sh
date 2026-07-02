#!/usr/bin/env bash
# Build every fuzz target once (ASan on -- keep it, the SIMD scanners are
# `unsafe`). Run this before fuzz-all.sh so the tmux windows don't serialize on
# the first compile.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_tools
cd "$REPO_ROOT"
echo "Building all fuzz targets (this compiles uppsala + libFuzzer once)..."
cargo +nightly fuzz build --fuzz-dir "$FUZZ_DIR"
echo "Build OK. Binaries under $FUZZ_DIR/target/*/release/"
