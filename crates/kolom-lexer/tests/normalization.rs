//! Bengali has two encodings for several letters, and input methods disagree
//! about which to produce. `য়` is either U+09DF or `য` U+09AF followed by a
//! nukta U+09BC; `ড়` and `ঢ়` are the same story. Unicode lists all three
//! precomposed forms as composition exclusions, so NFC settles on the
//! two-codepoint spelling.
//!
//! Two source files can therefore be identical on screen and differ in bytes.
//! Before the lexer normalized, `শেয়ার` and `ডায়ালগ` typed the precomposed way
//! were not keywords at all, and the parse error that followed pointed at text
//! that looked exactly right.

use kolom_lexer::{lex, TokenKind, KEYWORDS};

/// The three Bengali letters with two spellings, precomposed -> decomposed.
const PAIRS: &[(&str, &str)] = &[
    ("\u{09DF}", "\u{09AF}\u{09BC}"), // য়
    ("\u{09DC}", "\u{09A1}\u{09BC}"), // ড়
    ("\u{09DD}", "\u{09A2}\u{09BC}"), // ঢ়
];

/// Rewrites `s` into the precomposed spelling — what several common Bengali
/// keyboards emit, and what the repository's own sources do *not* use.
fn precompose(s: &str) -> String {
    let mut out = s.to_string();
    for (pre, decomp) in PAIRS {
        out = out.replace(decomp, pre);
    }
    out
}

/// Every keyword must already be NFC, or normalizing the input would move
/// identifiers *away* from the table and reintroduce the bug in reverse.
/// This is the invariant the lexer's normalization relies on.
#[test]
fn keyword_table_is_nfc() {
    use unicode_normalization::UnicodeNormalization;
    for kw in KEYWORDS {
        let nfc: String = kw.nfc().collect();
        assert_eq!(
            nfc, **kw,
            "keyword {kw:?} is not NFC. The lexer normalizes identifiers to NFC, \
             so a table entry in any other form could never be matched at all."
        );
    }
}

/// The two keywords the bug actually reached.
#[test]
fn affected_keywords_are_recognized_either_way() {
    for kw in ["শেয়ার", "ডায়ালগ"] {
        let precomposed = precompose(kw);
        assert_ne!(precomposed, kw, "{kw} should differ once precomposed");

        let (toks, errs) = lex(&precomposed);
        assert!(errs.is_empty(), "{kw}: lex errors: {errs:?}");
        assert!(
            matches!(toks.first().map(|t| &t.kind), Some(TokenKind::Kw(k)) if *k == kw),
            "{kw}: precomposed spelling did not lex as the keyword, got {:?}",
            toks.first().map(|t| &t.kind)
        );
    }
}

/// A declaration using the precomposed spelling must produce exactly the same
/// tokens as the decomposed one — the real-world symptom was a parse failure a
/// few tokens later, not a bad identifier.
#[test]
fn precomposed_source_lexes_identically() {
    let decomposed = "ধরি ক : শেয়ার সংখ্যা = শেয়ার_করো(১০)";
    let precomposed = precompose(decomposed);
    assert_ne!(precomposed, decomposed, "the two spellings should differ in bytes");

    let (a, ea) = lex(decomposed);
    let (b, eb) = lex(&precomposed);
    assert!(ea.is_empty() && eb.is_empty(), "lex errors: {ea:?} / {eb:?}");

    let kinds = |ts: &[kolom_lexer::Token]| format!("{:?}", ts.iter().map(|t| t.kind.clone()).collect::<Vec<_>>());
    assert_eq!(kinds(&a), kinds(&b), "token streams differ between spellings");
}

/// Identifiers are names to be matched, so they normalize. String literals are
/// program data and must survive byte-for-byte — normalizing them would
/// silently rewrite what a program prints or writes to a file.
#[test]
fn string_literals_are_left_alone() {
    let precomposed = format!("\"{}\"", precompose("যায়নি"));
    let (toks, errs) = lex(&precomposed);
    assert!(errs.is_empty(), "lex errors: {errs:?}");
    match toks.first().map(|t| &t.kind) {
        Some(TokenKind::Str(s)) => assert!(
            s.contains('\u{09DF}'),
            "the literal's precomposed spelling was rewritten: {s:?}"
        ),
        other => panic!("expected a string literal, got {other:?}"),
    }
}
