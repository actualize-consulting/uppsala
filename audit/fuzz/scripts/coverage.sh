#!/usr/bin/env bash
# Generate an HTML line-coverage report for a target's accumulated corpus, so you
# can see which parts of the parser/serializer/xpath the fuzzer actually reaches.
#
#   coverage.sh <target>
#
# Prereqs (once):
#   rustup component add llvm-tools-preview --toolchain nightly
#   (optional, for demangled names) sfw cargo install rustfilt
#
# This calls `llvm-cov` from the llvm-tools-preview component directly rather
# than the `cargo cov` wrapper: some cargo-binutils / clap version combinations
# make `cargo cov` panic on arg parsing ("arg `no-default-features`'s ArgAction
# ..."). The raw tool sidesteps that entirely.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
require_tools
TARGET="${1:?usage: coverage.sh <target>}"
cd "$REPO_ROOT"

# 1. Instrument + run the corpus + merge into coverage.profdata (this step
#    already worked for you; it's cheap to repeat and keeps the script one-shot).
cargo +nightly fuzz coverage --fuzz-dir "$FUZZ_DIR" "$TARGET"

TRIPLE="$(rustc -vV | sed -n 's/host: //p')"
PROF="$FUZZ_DIR/coverage/$TARGET/coverage.profdata"
OUT="$FUZZ_DIR/coverage/$TARGET/html"
mkdir -p "$OUT"

# 2. Locate the instrumented binary. `cargo fuzz coverage --fuzz-dir` builds it
#    into the REPO-ROOT target tree (not the fuzz crate's), under
#    coverage/<triple>/release/. Search both roots and prefer the coverage path;
#    glob because cargo-fuzz's exact layout has varied across versions.
BIN="$(find "$REPO_ROOT/target" "$FUZZ_DIR/target" -type f -name "$TARGET" -path "*coverage*release*" 2>/dev/null | head -1 || true)"
[ -z "$BIN" ] && BIN="$REPO_ROOT/target/$TRIPLE/coverage/$TRIPLE/release/$TARGET"
if [ ! -f "$BIN" ]; then
  echo "error: coverage binary not found for $TARGET"
  echo "       looked under $REPO_ROOT/target and $FUZZ_DIR/target for a coverage/*/release/$TARGET"; exit 1
fi
if [ ! -f "$PROF" ]; then
  echo "error: profdata not found at $PROF"; exit 1
fi

# 3. Find llvm-cov inside the nightly sysroot (installed by llvm-tools-preview).
SYSROOT="$(rustc +nightly --print sysroot)"
LLVM_COV="$SYSROOT/lib/rustlib/$TRIPLE/bin/llvm-cov"
if [ ! -x "$LLVM_COV" ]; then
  echo "error: llvm-cov not found at $LLVM_COV"
  echo "       run: rustup component add llvm-tools-preview --toolchain nightly"; exit 1
fi

# 4. Optional Rust name demangling if rustfilt is installed.
DEMANGLER=()
command -v rustfilt >/dev/null 2>&1 && DEMANGLER=(-Xdemangler=rustfilt)

"$LLVM_COV" show \
  "${DEMANGLER[@]}" \
  "$BIN" \
  -instr-profile="$PROF" \
  -show-line-counts-or-regions \
  -show-instantiations \
  -format=html \
  -output-dir="$OUT" \
  "$REPO_ROOT/src"

echo "Coverage HTML: $OUT/index.html"
