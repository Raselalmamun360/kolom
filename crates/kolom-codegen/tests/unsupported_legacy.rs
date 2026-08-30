//! `kolom-sema` accepts these calls (they're real, valid stdlib functions on
//! the default Cranelift backend), but the legacy `--সি` C backend
//! deliberately doesn't implement the ones that need a platform-specific
//! recursive directory walk or Windows `FILETIME` conversion — see
//! `docs/language.md` §১৭.১. What matters here is that `kolom_codegen::emit`
//! fails loudly (a panic with a clear message) rather than silently
//! dispatching to the wrong runtime function through the C backend's
//! wildcard item-name match, which is exactly the footgun this test guards
//! against regressing.

fn emit_would_panic(src: &str) -> String {
    let (tokens, lex_errs) = kolom_lexer::lex(src);
    assert!(lex_errs.is_empty(), "unexpected lex errors: {lex_errs:?}");
    let (prog, parse_errs) = kolom_syntax::parse(tokens);
    assert!(parse_errs.is_empty(), "unexpected parse errors: {parse_errs:?}");
    let sema_errs = kolom_sema::analyze(&prog);
    assert!(sema_errs.is_empty(), "unexpected sema errors (function should be a valid stdlib call): {sema_errs:?}");

    let result = std::panic::catch_unwind(|| kolom_codegen::emit(&prog, "t", "windows"));
    match result {
        Ok(_) => panic!("expected emit() to panic (unsupported-in---সি item), but it produced C code silently"),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "<non-string panic payload>".to_string()),
    }
}

#[test]
fn dir_delete_unsupported_under_legacy_backend() {
    let src = "ইম্পোর্ট ফাইলসিস্টেম\n\nঅ্যাপ {\n    ফাইলসিস্টেম.ডিরেক্টরি_মুছো(\"এক্স\")\n}\n";
    let msg = emit_would_panic(src);
    assert!(msg.contains("ডিরেক্টরি_মুছো"), "panic message should name the unsupported function: {msg:?}");
}

#[test]
fn dir_copy_unsupported_under_legacy_backend() {
    let src = "ইম্পোর্ট ফাইলসিস্টেম\n\nঅ্যাপ {\n    ফাইলসিস্টেম.ডিরেক্টরি_কপি(\"এক্স\", \"ওয়াই\")\n}\n";
    let msg = emit_would_panic(src);
    assert!(msg.contains("ডিরেক্টরি_কপি"), "panic message should name the unsupported function: {msg:?}");
}

#[test]
fn mtime_unsupported_under_legacy_backend() {
    let src = "ইম্পোর্ট ফাইলসিস্টেম\n\nঅ্যাপ {\n    ধরি ক = ফাইলসিস্টেম.পরিবর্তনের_সময়(\"এক্স\")\n}\n";
    let msg = emit_would_panic(src);
    assert!(msg.contains("পরিবর্তনের_সময়"), "panic message should name the unsupported function: {msg:?}");
}

/// `ম্যাট্রিক্স`'s runtime lives in kolom-runtime (Cranelift backend only —
/// nested-array marshalling and Gaussian elimination were never ported to
/// this backend's hand-written C codegen). Its whole module is unsupported
/// here, so any one function stands in for all of them.
#[test]
fn matrix_unsupported_under_legacy_backend() {
    let src = "ইম্পোর্ট ম্যাট্রিক্স\n\nঅ্যাপ {\n    ধরি ক = ম্যাট্রিক্স.নির্ণায়ক([[1.0, 2.0], [3.0, 4.0]])\n}\n";
    let msg = emit_would_panic(src);
    assert!(msg.contains("নির্ণায়ক"), "panic message should name the unsupported function: {msg:?}");
}

/// Like `ম্যাট্রিক্স`, `জ্যামিতি` gets no legacy-backend investment at all —
/// the native compiler and interpreter are Kolom's two real execution paths;
/// `--সি` is legacy/non-default and not worth partial (scalar-only) support
/// for the sake of parity with the point/polygon functions that would still
/// need the same নেস্টেড-অ্যারে marshalling `ম্যাট্রিক্স` never got here.
#[test]
fn geometry_unsupported_under_legacy_backend() {
    let src = "ইম্পোর্ট জ্যামিতি\n\nঅ্যাপ {\n    ধরি ক = জ্যামিতি.দূরত্ব(0.0, 0.0, 3.0, 4.0)\n}\n";
    let msg = emit_would_panic(src);
    assert!(msg.contains("দূরত্ব"), "panic message should name the unsupported function: {msg:?}");
}
