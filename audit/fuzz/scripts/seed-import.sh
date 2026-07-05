#!/usr/bin/env bash
# seed-import.sh — enrich the fuzz WORKING corpus from test-data/corpus/.
#
# Copies the vendored/generated corpus assembled by scripts/fetch_corpus.sh
# into the matching audit/fuzz/corpus/<target>/ working directories (git-ignored,
# the fuzzer's live set), and folds the libxml2 fuzzer dictionaries into the
# tracked audit/fuzz/dict/. Idempotent.
#
# The large external inputs deliberately land in corpus/ (git-ignored) rather
# than seeds/ (tracked, curated) so git stays lean; run.sh already unions both
# into the working corpus at fuzz time.
#
# Run scripts/fetch_corpus.sh first. If the corpus is absent this is a no-op.
set -euo pipefail

FUZZ_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"   # .../audit/fuzz
REPO_ROOT="$(cd "$FUZZ_DIR/../.." && pwd)"
CORPUS="$REPO_ROOT/test-data/corpus"
WORK="$FUZZ_DIR/corpus"   # git-ignored working set

if [ ! -d "$CORPUS" ]; then
  echo "corpus absent ($CORPUS) — run scripts/fetch_corpus.sh first; nothing to import"
  exit 0
fi

# copy_into <target> <prefix> <files...>
copy_into() {
  local target=$1 prefix=$2; shift 2
  local dest="$WORK/$target"
  mkdir -p "$dest"
  local n=0
  for f in "$@"; do
    [ -f "$f" ] || continue
    cp -f "$f" "$dest/${prefix}$(basename "$f")"
    n=$((n + 1))
  done
  [ "$n" -gt 0 ] && echo "  $target: +$n seeds ($prefix*)"
  return 0
}

echo "== importing parser seeds =="
# go-fuzz-corpus (already hash-named, unique) -> parse + parse_bytes + pull.
if [ -d "$CORPUS/fuzz-seeds/go-fuzz-corpus" ]; then
  copy_into fuzz_parse gofuzz- "$CORPUS"/fuzz-seeds/go-fuzz-corpus/*
  copy_into fuzz_parse_bytes gofuzz- "$CORPUS"/fuzz-seeds/go-fuzz-corpus/*
  copy_into fuzz_pull gofuzz- "$CORPUS"/fuzz-seeds/go-fuzz-corpus/*
fi
# real-world dialects + security payloads -> parse + parse_bytes + pull.
while IFS= read -r f; do
  rel=${f#"$CORPUS/"}
  flat=rw-$(echo "${rel#realworld/}" | tr '/' '-')
  cp -f "$f" "$WORK/fuzz_parse/$flat" 2>/dev/null || true
  cp -f "$f" "$WORK/fuzz_parse_bytes/$flat" 2>/dev/null || true
  cp -f "$f" "$WORK/fuzz_pull/$flat" 2>/dev/null || true
done < <(find "$CORPUS/realworld" -type f \( -name '*.xml' -o -name '*.svg' -o -name '*.kml' -o -name '*.gpx' -o -name '*.xhtml' -o -name '*.plist' \) 2>/dev/null)
echo "  fuzz_parse / fuzz_parse_bytes / fuzz_pull: +realworld dialects"

if [ -d "$CORPUS/security" ]; then
  copy_into fuzz_parse sec- "$CORPUS"/security/recurse/*.xml "$CORPUS"/security/xxe/*.xml "$CORPUS"/security/roundtrip/*.xml
  copy_into fuzz_pull sec- "$CORPUS"/security/recurse/*.xml "$CORPUS"/security/xxe/*.xml "$CORPUS"/security/roundtrip/*.xml
  copy_into fuzz_roundtrip sec- "$CORPUS"/security/roundtrip/*.xml
fi

echo "== importing schema seeds =="
if [ -d "$CORPUS/fuzz-seeds/libxml2/schemas" ]; then
  copy_into fuzz_xsd_builder libxml2- "$CORPUS"/fuzz-seeds/libxml2/schemas/*.xsd
fi
# sitemap schema from the realworld set is a valid XSD too.
[ -f "$CORPUS/realworld/sitemap/schema.xsd" ] && \
  cp -f "$CORPUS/realworld/sitemap/schema.xsd" "$WORK/fuzz_xsd_builder/rw-sitemap.xsd"

echo "== importing xpath seeds =="
# libxml2 test/XPath/expr holds bare XPath expressions.
if [ -d "$CORPUS/fuzz-seeds/libxml2/xpath/expr" ]; then
  copy_into fuzz_xpath libxml2- "$CORPUS"/fuzz-seeds/libxml2/xpath/expr/*
fi

echo "== merging libxml2 fuzzer dictionaries =="
# merge_dict <src> <dst>: append tokens from src not already in dst.
merge_dict() {
  local src=$1 dst=$2
  [ -f "$src" ] || return 0
  mkdir -p "$(dirname "$dst")"
  touch "$dst"
  local added=0
  while IFS= read -r line; do
    case "$line" in
      ''|'#'*) continue ;;                       # skip blanks/comments
    esac
    if ! grep -Fxq "$line" "$dst"; then
      printf '%s\n' "$line" >> "$dst"
      added=$((added + 1))
    fi
  done < "$src"
  [ "$added" -gt 0 ] && echo "  $(basename "$dst"): +$added tokens from $(basename "$src")"
  return 0
}
DICTSRC="$CORPUS/fuzz-seeds/libxml2/dict"
if [ -d "$DICTSRC" ]; then
  merge_dict "$DICTSRC/xml.dict"    "$FUZZ_DIR/dict/xml.dict"
  merge_dict "$DICTSRC/xpath.dict"  "$FUZZ_DIR/dict/xpath.dict"
  merge_dict "$DICTSRC/regexp.dict" "$FUZZ_DIR/dict/xsd_regex.dict"
  merge_dict "$DICTSRC/schema.dict" "$FUZZ_DIR/dict/xml.dict"
fi

echo "done. working corpus enriched under $WORK (dict updates in $FUZZ_DIR/dict)"
