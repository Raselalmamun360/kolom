//! Every program in `examples/` must keep working — same rationale as
//! `docs_examples.rs`, but for the standalone example programs rather than
//! the code fences inside the docs.
//!
//! Console examples are run to completion, interpreted AND natively, and
//! must exit successfully. The two UI examples (`07_গণক_অ্যাপ`,
//! `08_ক্যানভাস_আর্ট`) open a real window, so they are only parsed and
//! type-checked here — running them is covered natively by
//! `kolom-codegen-cranelift`'s headless UI tests instead, which already
//! script clicks and assert on handler output for the state pattern these
//! examples use (`golden_m4.rs::ui_shared_cell_get_set`).

use std::path::{Path, PathBuf};
use std::process::Command;

const UI_EXAMPLES: &[&str] = &["07_গণক_অ্যাপ", "08_ক্যানভাস_আর্ট"];

/// Examples whose output legitimately differs run to run — excluded from
/// the strict byte-for-byte backend-parity check, though `run_interpreted`/
/// `run_natively` still require them to run successfully.
///
/// `03_কেনাকাটার_তালিকা` iterates a `ম্যাপ`'s keys, and both backends'
/// `ম্যাপ` are hash-ordered with a randomized seed — the interpreter uses
/// `std::collections::HashMap`, whose default hasher reseeds every process.
/// The two backends happening to agree on one run is luck, not a
/// guarantee, so asserting exact equality here would be a flaky test.
const ORDER_NONDETERMINISTIC: &[&str] = &["03_কেনাকাটার_তালিকা"];

fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("examples")
}

fn kolom_exe() -> PathBuf {
    // .../target/<profile>/deps/examples-<hash>.exe
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

fn example_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(examples_root())
        .expect("examples/ should exist")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn is_ui_example(dir: &Path) -> bool {
    let name = dir.file_name().unwrap().to_string_lossy();
    UI_EXAMPLES.contains(&name.as_ref())
}

#[test]
fn console_examples_run_interpreted() {
    let mut failures = Vec::new();
    for dir in example_dirs() {
        if is_ui_example(&dir) {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let out = Command::new(kolom_exe())
            .arg("চালাও") // চালাও
            .arg(dir.join("main.ক")) // main.ক
            .current_dir(&dir) // 05_নোট_রাখা writes/removes a file relative to cwd
            .output()
            .unwrap_or_else(|e| panic!("{name}: failed to run kolom: {e}"));
        if !out.status.success() {
            failures.push(format!("{name}: {}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    assert!(failures.is_empty(), "interpreted example(s) failed:\n{}", failures.join("\n"));
}

#[test]
fn console_examples_run_natively() {
    let mut failures = Vec::new();
    for dir in example_dirs() {
        if is_ui_example(&dir) {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let build_dir = std::env::temp_dir().join("kolom-examples-native").join(&name);
        let _ = std::fs::remove_dir_all(&build_dir);
        std::fs::create_dir_all(&build_dir).unwrap();
        let src = build_dir.join("main.ক");
        std::fs::copy(dir.join("main.ক"), &src).unwrap();

        let build = Command::new(kolom_exe())
            .arg("বিল্ড") // বিল্ড
            .arg(&src)
            .current_dir(&build_dir)
            .output()
            .unwrap_or_else(|e| panic!("{name}: failed to run kolom: {e}"));
        if !build.status.success() {
            failures.push(format!("{name}: build failed: {}", String::from_utf8_lossy(&build.stderr)));
            continue;
        }
        let exe = String::from_utf8_lossy(&build.stdout).trim().to_string();
        let run = Command::new(&exe)
            .current_dir(&build_dir)
            .output()
            .unwrap_or_else(|e| panic!("{name}: failed to run produced exe {exe}: {e}"));
        if !run.status.success() {
            failures.push(format!("{name}: exe exited {:?}: {}", run.status.code(), String::from_utf8_lossy(&run.stderr)));
        }
    }
    assert!(failures.is_empty(), "native example(s) failed:\n{}", failures.join("\n"));
}

/// Console examples must give byte-identical output on both backends — the
/// whole point of testing both is to catch the day one of them drifts.
#[test]
fn console_examples_agree_between_backends() {
    let mut mismatches = Vec::new();
    for dir in example_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if is_ui_example(&dir) || ORDER_NONDETERMINISTIC.contains(&name.as_str()) {
            continue;
        }

        let interp_dir = std::env::temp_dir().join("kolom-examples-parity").join(&name).join("interp");
        let _ = std::fs::remove_dir_all(&interp_dir);
        std::fs::create_dir_all(&interp_dir).unwrap();
        let interp_src = interp_dir.join("main.ক");
        std::fs::copy(dir.join("main.ক"), &interp_src).unwrap();
        let interp_out = Command::new(kolom_exe())
            .arg("চালাও")
            .arg(&interp_src)
            .current_dir(&interp_dir)
            .output()
            .unwrap_or_else(|e| panic!("{name}: interpreted run failed: {e}"));

        let native_dir = std::env::temp_dir().join("kolom-examples-parity").join(&name).join("native");
        let _ = std::fs::remove_dir_all(&native_dir);
        std::fs::create_dir_all(&native_dir).unwrap();
        let native_src = native_dir.join("main.ক");
        std::fs::copy(dir.join("main.ক"), &native_src).unwrap();
        let build = Command::new(kolom_exe())
            .arg("বিল্ড")
            .arg(&native_src)
            .current_dir(&native_dir)
            .output()
            .unwrap_or_else(|e| panic!("{name}: build failed: {e}"));
        if !build.status.success() {
            continue; // already reported by console_examples_run_natively
        }
        let exe = String::from_utf8_lossy(&build.stdout).trim().to_string();
        let native_out = Command::new(&exe).current_dir(&native_dir).output().unwrap();

        let norm = |b: &[u8]| String::from_utf8_lossy(b).replace("\r\n", "\n");
        let (a, c) = (norm(&interp_out.stdout), norm(&native_out.stdout));
        if a != c {
            mismatches.push(format!("{name}:\n  interpreted: {a:?}\n  native:      {c:?}"));
        }
    }
    assert!(mismatches.is_empty(), "backend output mismatch:\n{}", mismatches.join("\n\n"));
}

/// The two UI examples aren't run here (they open a real window), but they
/// must still parse and type-check — same bar `docs_examples.rs` holds every
/// documented program to.
#[test]
fn ui_examples_compile() {
    let mut failures = Vec::new();
    for dir in example_dirs() {
        if !is_ui_example(&dir) {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let src = std::fs::read_to_string(dir.join("main.ক")).unwrap();
        let (tokens, lex_errs) = kolom_lexer::lex(&src);
        if !lex_errs.is_empty() {
            failures.push(format!("{name}: lex errors: {lex_errs:?}"));
            continue;
        }
        let (prog, parse_errs) = kolom_syntax::parse(tokens);
        if !parse_errs.is_empty() {
            failures.push(format!("{name}: parse errors: {parse_errs:?}"));
            continue;
        }
        let diags = kolom_sema::analyze(&prog);
        if !diags.is_empty() {
            failures.push(format!("{name}: {diags:?}"));
        }
    }
    assert!(failures.is_empty(), "UI example(s) failed to compile:\n{}", failures.join("\n"));
}
