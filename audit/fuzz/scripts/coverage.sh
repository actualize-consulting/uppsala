#!/usr/bin/env bash
# Generate an HTML line-coverage report for a target's accumulated corpus, so you
# can see which parts of the parser/serializer/xpath the fuzzer actually reaches.
#
#   coverage.sh <target>
#
# Prereqs (once):
#   rustup component add llvm-tools-preview --toolchain nightly
#   sfw cargo install cargo-binutils rustfilt
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_tools
TARGET="${1:?usage: coverage.sh <target>}"
cd "$REPO_ROOT"

cargo +nightly fuzz coverage --fuzz-dir "$FUZZ_DIR" "$TARGET"

TRIPLE="$(rustc -vV | sed -n 's/host: //p')"
BIN="$FUZZ_DIR/target/$TRIPLE/coverage/$TRIPLE/release/$TARGET"
PROF="$FUZZ_DIR/coverage/$TARGET/coverage.profdata"
OUT="$FUZZ_DIR/coverage/$TARGET/html"
mkdir -p "$OUT"

cargo +nightly cov -- show \
  -Xdemangler=rustfilt \
  "$BIN" \
  -instr-profile="$PROF" \
  -show-line-counts-or-regions \
  -show-instantiations \
  -format=html \
  -output-dir="$OUT" \
  "$REPO_ROOT/src"

echo "Coverage HTML: $OUT/index.html"
