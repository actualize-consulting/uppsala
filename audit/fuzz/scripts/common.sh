#!/usr/bin/env bash
# Shared helpers for the uppsala fuzz scripts. Sourced, not executed.
set -euo pipefail

FUZZ_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"   # .../audit/fuzz
REPO_ROOT="$(cd "$FUZZ_DIR/../.." && pwd)"                    # repo root

ALL_TARGETS=(
  fuzz_simd_differential
  fuzz_escape_differential
  fuzz_parse
  fuzz_parse_bytes
  fuzz_roundtrip
  fuzz_serialize
  fuzz_dom_mutate
  fuzz_xpath
  fuzz_transform
  fuzz_xsd_regex
  fuzz_xsd_builder
)

# Map a target to its libFuzzer dictionary (empty if none).
dict_for() {
  case "$1" in
    # Byte-oriented differential scanners: no XML dictionary, boundary-length
    # seeds matter instead (see seeds/).
    fuzz_simd_differential|fuzz_escape_differential) echo "" ;;
    fuzz_xpath)      echo "$FUZZ_DIR/dict/xpath.dict" ;;
    fuzz_xsd_regex)  echo "$FUZZ_DIR/dict/xsd_regex.dict" ;;
    *)               echo "$FUZZ_DIR/dict/xml.dict" ;;   # all XML-shaped inputs
  esac
}

require_tools() {
  command -v cargo >/dev/null || { echo "cargo not found (install rustup)"; exit 1; }
  cargo +nightly --version >/dev/null 2>&1 || {
    echo "nightly toolchain missing: rustup toolchain install nightly"; exit 1; }
  cargo +nightly fuzz --version >/dev/null 2>&1 || {
    echo "cargo-fuzz missing: sfw cargo install cargo-fuzz"; exit 1; }
}
