//! Shared test harness: compiles Kolom source to a native exe through the
//! real pipeline (parse -> sema -> Cranelift -> link) and runs it.
//!
//! Linking goes through `kolom_codegen_cranelift::link_executable`, the same
//! code path `কলম বিল্ড` uses — so these tests exercise the shipping linker
//! logic rather than a parallel copy of it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Built {
    pub exe: PathBuf,
    pub dir: PathBuf,
}

/// Compiles `src` to a native executable. Panics with a labelled message on
/// any failure, so callers read as straight-line assertions.
pub fn build(name: &str, src: &str) -> Built {
    let (tokens, lex_errs) = kolom_lexer::lex(src);
    assert!(lex_errs.is_empty(), "{name}: lex errors: {lex_errs:?}");
    let (prog, parse_errs) = kolom_syntax::parse(tokens);
    assert!(parse_errs.is_empty(), "{name}: parse errors: {parse_errs:?}");

    let obj_bytes = kolom_codegen_cranelift::emit(&prog).unwrap_or_else(|e| panic!("{name}: codegen failed: {e}"));

    let dir = std::env::temp_dir().join("kolom-cranelift-tests").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let obj = dir.join("out.obj");
    let exe = dir.join("out.exe");
    std::fs::write(&obj, &obj_bytes).unwrap();

    kolom_codegen_cranelift::link_executable(&obj, &exe).unwrap_or_else(|e| panic!("{name}: link failed: {e}"));
    Built { exe, dir }
}

/// Builds, runs, and returns stdout. `envs` sets environment variables (used
/// by the UI tests' headless hooks); `cwd` overrides the working directory
/// (used by the filesystem tests so they don't touch the repo).
pub fn run_with(name: &str, src: &str, envs: &[(&str, &str)], cwd: Option<&Path>) -> String {
    let built = build(name, src);
    let mut cmd = Command::new(&built.exe);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    if let Some(c) = cwd {
        std::fs::create_dir_all(c).unwrap();
        cmd.current_dir(c);
    }
    let res = cmd.output().unwrap_or_else(|e| panic!("{name}: failed to run produced exe: {e}"));
    assert!(
        res.status.success(),
        "{name}: produced exe exited {:?}:\n{}",
        res.status.code(),
        String::from_utf8_lossy(&res.stderr)
    );
    String::from_utf8_lossy(&res.stdout).into_owned()
}

/// Builds, runs, returns stdout.
pub fn run(name: &str, src: &str) -> String {
    run_with(name, src, &[], None)
}

/// Root of kolom-cli's golden fixture directory — the language's reference
/// behaviour, produced by the interpreter and shared with the C backend.
fn fixture_dir(fixture: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("kolom-cli")
        .join("tests")
        .join("golden")
        .join(fixture)
}

/// Compiles `fixture`'s `main.ক` natively and asserts its output matches the
/// SAME `expected.txt` the interpreter is checked against.
///
/// Asserting against the shared reference rather than a hand-written string
/// is deliberate: an earlier version of these tests hardcoded the Cranelift
/// backend's own output, so when that output silently diverged from the
/// interpreter (Bengali vs ASCII digits, byte vs codepoint `দৈর্ঘ্য`) the
/// tests happily confirmed the bug. Sharing the reference makes that class
/// of drift impossible.
pub fn assert_matches_reference(fixture: &str) {
    assert_matches_reference_with(fixture, &[], false);
}

/// As `assert_matches_reference`, with env vars and an option to run in a
/// freshly-cleared working directory (for fixtures that create files).
pub fn assert_matches_reference_with(fixture: &str, envs: &[(&str, &str)], clean_cwd: bool) {
    let dir = fixture_dir(fixture);
    let src = std::fs::read_to_string(dir.join("main.ক"))
        .unwrap_or_else(|e| panic!("{fixture}: cannot read main.ক: {e}"));
    let expected = std::fs::read_to_string(dir.join("expected.txt"))
        .unwrap_or_else(|e| panic!("{fixture}: cannot read expected.txt: {e}"));

    let cwd = clean_cwd.then(|| {
        let p = std::env::temp_dir().join("kolom-cranelift-tests").join(fixture).join("cwd");
        let _ = std::fs::remove_dir_all(&p);
        p
    });
    let got = run_with(fixture, &src, envs, cwd.as_deref());

    // expected.txt is stored with the platform's line endings; compare on
    // normalized newlines so the check is about content, not checkout style.
    let norm = |s: &str| s.replace("\r\n", "\n");
    assert_eq!(
        norm(&got),
        norm(&expected),
        "{fixture}: native output differs from the interpreter's reference expected.txt"
    );
}

/// Builds and runs in a freshly-cleared working directory — for tests that
/// create files and therefore aren't idempotent across runs.
pub fn run_in_clean_dir(name: &str, src: &str) -> String {
    let cwd = std::env::temp_dir().join("kolom-cranelift-tests").join(name).join("cwd");
    let _ = std::fs::remove_dir_all(&cwd);
    run_with(name, src, &[], Some(&cwd))
}
