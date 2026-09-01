//! Regression test for a stack-overflow crash (docs/v2-prerequisites.md §৬
//! stabilization pass): `kolom_interp`'s evaluator is a plain recursive-
//! descent walk with no depth limit, and used to overflow the default OS
//! thread stack at a shallow depth (as few as ~৫০ frames in a debug build).
//! `কলম চালাও`/`কলম পাতা` now run the interpreter on a dedicated thread with
//! a much larger stack (see `run_on_deep_stack` in `main.rs`) — this test
//! spawns the real `kolom` binary (so it actually exercises that thread
//! wrapping, unlike golden.rs's in-process `kolom_interp::run` calls) with
//! recursion depths that reliably crashed before the fix.

use std::path::PathBuf;
use std::process::Command;

fn kolom_exe() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

// Each test gets its own subdirectory — tests in this file run in parallel
// (the default), and a shared path would race two `main.ক` writes against
// each other.
fn run_src(name: &str, src: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join("kolom-deep-recursion-test").join(name);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("main.ক");
    std::fs::write(&path, src).unwrap();
    Command::new(kolom_exe())
        .arg("চালাও")
        .arg(&path)
        .output()
        .expect("failed to run kolom")
}

#[test]
fn plain_recursive_function_does_not_overflow() {
    let src = "ফাংশন গণনা(সংখ্যা n) -> সংখ্যা {\n\
        যদি (n <= ০) {\n            রিটার্ন ০\n        }\n\
        রিটার্ন ১ + গণনা(n - ১)\n\
    }\n\
    অ্যাপ {\n        লেখো(লেখায়(গণনা(3000)))\n    }\n";
    let out = run_src("plain_fn", src);
    assert!(
        out.status.success(),
        "recursive call at depth 3000 crashed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3000");
}

#[test]
fn recursive_enum_eval_does_not_overflow() {
    let src = "এনাম Expr {\n    Num(সংখ্যা),\n    Add(শেয়ার Expr, শেয়ার Expr)\n}\n\
    ফাংশন মূল্যায়ন(Expr e) -> সংখ্যা {\n        রিটার্ন মিলাও e {\n            Num(n) => n,\n            Add(l, r) => মূল্যায়ন(মান(l)) + মূল্যায়ন(মান(r)),\n        }\n    }\n\
    অ্যাপ {\n\
        ধরি e = Num(১)\n\
        ধরি i = ০\n\
        যতক্ষণ (i < 3000) {\n            e = Add(শেয়ার_করো(e), শেয়ার_করো(Num(১)))\n            i = i + ১\n        }\n\
        লেখো(লেখায়(মূল্যায়ন(e)))\n    }\n";
    let out = run_src("recursive_enum", src);
    assert!(
        out.status.success(),
        "recursive এনাম evaluation at depth 3000 crashed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3001");
}
