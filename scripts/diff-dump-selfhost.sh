#!/usr/bin/env bash
# M2 differential-test harness (self-hosting-plan.md §১০, M2) — checks the
# self-hosted lexer (selfhost/compiler/*.ক) against the reference Rust
# lexer (kolom-lexer), byte-for-byte, over every golden fixture.
#
# Unlike scripts/diff-dump.sh (which compares two `kolom` *binaries*), one
# side of this comparison isn't a binary at all yet — the self-hosted
# lexer is Kolom source with no native compiler of its own, so it can only
# be *run*: `$KOLOM চালাও selfhost/compiler/main.ক <file>`. Both sides of
# every comparison here therefore invoke the same real kolom.exe; the
# difference is which one is asked to lex the fixture — the built-in `lex
# --stable` subcommand, or the interpreter executing the self-hosted
# lexer's own source on it.
#
# `--stable` (crates/kolom-cli/src/main.rs) is a tab-separated dump format
# distinct from `kolom lex`'s default human-readable one. The default
# format goes through Rust's `{:?}`, which escapes by `char::is_printable`
# and treats Bengali combining marks as unprintable (`অ্যাপ` becomes
# `"অ\u{9cd}য\u{9be}প"`) — every golden fixture hits this. Matching that
# byte-for-byte from Kolom source would mean carrying Rust's Unicode
# printability table into the self-hosted compiler forever, and would
# test formatting fidelity rather than lexing. `--stable`'s escaping is
# instead fully specified in Kolom terms (backslash, three whitespace
# characters, C0/DEL) and reproduced field-for-field by
# selfhost/compiler/main.ক's স্টেবল_এস্কেপ/স্টেবল_টোকেন_লাইন.
#
# Usage: scripts/diff-dump-selfhost.sh <kolom-exe> [golden-dir]
#   kolom-exe    required — must be a build with `kolom lex --stable`
#                (this repo's reference toolchain; the self-hosted side
#                always runs through this same binary's ইন্টারপ্রেটার)
#   golden-dir   optional — defaults to crates/kolom-cli/tests/golden

set -u

KOLOM="${1:?usage: $0 <kolom-exe> [golden-dir]}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GOLDEN="${2:-$ROOT/crates/kolom-cli/tests/golden}"
SELFHOST_MAIN="$ROOT/selfhost/compiler/main.ক"

if [ ! -x "$KOLOM" ] && ! command -v "$KOLOM" >/dev/null 2>&1; then
    echo "ত্রুটি: '$KOLOM' চালানো যায় না" >&2
    exit 1
fi
if [ ! -f "$SELFHOST_MAIN" ]; then
    echo "ত্রুটি: সেলফ-হোস্টেড লেক্সার পাওয়া যায়নি — '$SELFHOST_MAIN'" >&2
    exit 1
fi

checked=0
failed=0

for dir in "$GOLDEN"/*/; do
    src="$dir"main.ক
    [ -f "$src" ] || continue
    name="$(basename "$dir")"
    checked=$((checked + 1))

    out_ref="$("$KOLOM" lex --stable "$src" 2>&1)"
    out_self="$("$KOLOM" চালাও "$SELFHOST_MAIN" "$src" 2>&1)"
    if [ "$out_ref" != "$out_self" ]; then
        failed=$((failed + 1))
        echo "=== $name — differs ==="
        diff <(echo "$out_ref") <(echo "$out_self") | head -20
        echo
    fi
done

echo "$checked ফিক্সচার চেক করা হয়েছে (সেলফ-হোস্টেড বনাম রেফারেন্স লেক্সার), $failed-টা মিলেনি"
[ "$failed" -eq 0 ]
