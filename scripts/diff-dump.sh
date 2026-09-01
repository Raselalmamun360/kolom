#!/usr/bin/env bash
# Differential-test harness for v2.0 self-hosting (docs/v2-prerequisites.md
# §৭). Runs `kolom lex`/`kolom ast` for every golden fixture through two
# `kolom` binaries and diffs the output byte-for-byte — the day a
# self-hosted lexer/parser exists, pointing B at it turns this into the
# actual "does the new frontend agree with the reference one" check the
# prereq doc calls for. Until then, running it with A and B set to the same
# binary is a self-consistency smoke test (and catches accidental
# token/AST-shape drift from ordinary Rust-side changes too).
#
# Usage: scripts/diff-dump.sh <kolom-exe-A> [kolom-exe-B] [golden-dir]
#   kolom-exe-A   required — the reference toolchain
#   kolom-exe-B   optional — defaults to kolom-exe-A (self-consistency mode)
#   golden-dir    optional — defaults to crates/kolom-cli/tests/golden

set -u

A="${1:?usage: $0 <kolom-exe-A> [kolom-exe-B] [golden-dir]}"
B="${2:-$A}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GOLDEN="${3:-$ROOT/crates/kolom-cli/tests/golden}"

if [ ! -x "$A" ] && ! command -v "$A" >/dev/null 2>&1; then
    echo "ত্রুটি: '$A' চালানো যায় না" >&2
    exit 1
fi
if [ ! -x "$B" ] && ! command -v "$B" >/dev/null 2>&1; then
    echo "ত্রুটি: '$B' চালানো যায় না" >&2
    exit 1
fi

checked=0
failed=0

for dir in "$GOLDEN"/*/; do
    src="$dir"main.ক
    [ -f "$src" ] || continue
    name="$(basename "$dir")"
    checked=$((checked + 1))

    for mode in lex ast; do
        out_a="$("$A" "$mode" "$src" 2>&1)"
        out_b="$("$B" "$mode" "$src" 2>&1)"
        if [ "$out_a" != "$out_b" ]; then
            failed=$((failed + 1))
            echo "=== $name ($mode) — differs ==="
            diff <(echo "$out_a") <(echo "$out_b") | head -20
            echo
        fi
    done
done

echo "$checked ফিক্সচার চেক করা হয়েছে, $failed-টা ডাম্প মিলেনি (lex+ast দুটোই ধরে)"
[ "$failed" -eq 0 ]
