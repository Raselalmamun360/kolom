#!/usr/bin/env bash
# Compare the Rust stable AST dump with the self-hosted AST dumper.
# This script is intentionally not part of cargo tests: it requires a Kolom
# interpreter supplied by the caller.
#
# Usage: scripts/diff-ast-selfhost.sh <kolom-exe> [golden-dir]

set -u

KOLOM="${1:?usage: $0 <kolom-exe> [golden-dir]}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GOLDEN="${2:-$ROOT/crates/kolom-cli/tests/golden}"
SELFHOST_MAIN="$ROOT/selfhost/compiler/main_এস্ট.ক"

if [ ! -x "$KOLOM" ] && ! command -v "$KOLOM" >/dev/null 2>&1; then
    echo "ত্রুটি: '$KOLOM' চালানো যায় না" >&2
    exit 1
fi
if [ ! -f "$SELFHOST_MAIN" ]; then
    echo "ত্রুটি: self-hosted AST driver পাওয়া যায়নি" >&2
    exit 1
fi

run_limited() {
    if command -v timeout >/dev/null 2>&1; then
        timeout --signal=KILL 30 "$@"
    else
        echo "ত্রুটি: নিরাপদ M4 যাচাইয়ের জন্য 'timeout' কমান্ড আবশ্যক" >&2
        return 124
    fi
}

checked=0
failed=0
for dir in "$GOLDEN"/*/; do
    src="$dir"main.ক
    [ -f "$src" ] || continue
    name="$(basename "$dir")"
    checked=$((checked + 1))

    out_ref="$(run_limited "$KOLOM" ast --stable "$src" 2>&1)"
    out_self="$(run_limited "$KOLOM" চালাও "$SELFHOST_MAIN" "$src" 2>&1)"
    if [ "$out_ref" != "$out_self" ]; then
        failed=$((failed + 1))
        echo "=== $name — differs ==="
        diff <(printf '%s\n' "$out_ref") <(printf '%s\n' "$out_self") | head -40
        echo
    fi
done

echo "$checked ফিক্সচার চেক করা হয়েছে, $failed-টা AST ডাম্প মিলেনি"
[ "$failed" -eq 0 ]
