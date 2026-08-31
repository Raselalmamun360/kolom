//! End-to-end KPM tests: real `git init` local repos as packages (no mock —
//! `git` is a real local tool, same reasoning as `kolom-pkg`'s own unit
//! tests), driven through the actual `kolom` binary (`যোগ`/`ইনস্টল`, then
//! `চালাও`/`বিল্ড`), asserting real process output. Covers both execution
//! backends and the specific guarantee namespaced package dispatch exists
//! for: two packages exporting the same function name must not collide.

use std::path::{Path, PathBuf};
use std::process::Command;

fn kolom_exe() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("kolom-kpm-test").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn git(args: &[&str], cwd: &Path) {
    let out = Command::new("git").args(args).current_dir(cwd).output().expect("git must be on PATH for this test");
    assert!(out.status.success(), "git {:?} failed:\n{}", args, String::from_utf8_lossy(&out.stderr));
}

/// A minimal package repo: one function `নাম_ফাংশন(x) -> x * multiplier`,
/// exported as `<নাম>.ক`.
fn init_package_repo(dir: &Path, name: &str, func: &str, multiplier: i64) {
    std::fs::create_dir_all(dir).unwrap();
    git(&["init", "-q"], dir);
    git(&["config", "user.email", "t@example.com"], dir);
    git(&["config", "user.name", "t"], dir);
    std::fs::write(
        dir.join(format!("{name}.ক")),
        format!("ফাংশন {func}(সংখ্যা x) -> সংখ্যা {{\n    রিটার্ন x * {multiplier}\n}}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("কলম.toml"),
        format!("[\"প্যাকেজ\"]\n\"নাম\" = \"{name}\"\n\"সংস্করণ\" = \"0.1.0\"\n"),
    )
    .unwrap();
    git(&["add", "."], dir);
    git(&["commit", "-q", "-m", "init"], dir);
    git(&["tag", "v1.0.0"], dir);
}

fn kolom(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(kolom_exe()).args(args).current_dir(cwd).output().expect("failed to run kolom")
}

#[test]
fn qualified_package_call_works_both_backends() {
    let root = workdir("qualified-call");
    let dep = root.join("dep");
    init_package_repo(&dep, "সহায়ক", "দ্বিগুণ", 2);

    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let out = kolom(&["যোগ", "সহায়ক", &dep.to_string_lossy(), "--রেফ", "v1.0.0"], &project);
    assert!(out.status.success(), "যোগ failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let out = kolom(&["ইনস্টল"], &project);
    assert!(out.status.success(), "ইনস্টল failed:\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(project.join("কলম_প্যাকেজ").join("সহায়ক").join("সহায়ক.ক").exists());

    std::fs::write(project.join("main.ক"), "ইম্পোর্ট সহায়ক\n\nঅ্যাপ {\n    লেখো(সহায়ক.দ্বিগুণ(21))\n}\n").unwrap();

    let interp = kolom(&["চালাও", "main.ক"], &project);
    assert!(interp.status.success(), "চালাও failed:\n{}", String::from_utf8_lossy(&interp.stderr));
    assert_eq!(String::from_utf8_lossy(&interp.stdout).trim(), "42");

    let build = kolom(&["বিল্ড", "main.ক"], &project);
    assert!(build.status.success(), "বিল্ড failed:\n{}", String::from_utf8_lossy(&build.stderr));
    let exe = String::from_utf8_lossy(&build.stdout).trim().to_string();
    let run = Command::new(&exe).output().expect("failed to run produced exe");
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "42");
}

/// The whole point of qualified dispatch: two independently-authored
/// packages exporting a function with the *same name* must not collide —
/// each call resolves through its own package's namespace.
#[test]
fn two_packages_same_function_name_do_not_collide() {
    let root = workdir("collision");
    let dep_a = root.join("dep_a");
    init_package_repo(&dep_a, "সহায়ক", "দ্বিগুণ", 2);
    let dep_b = root.join("dep_b");
    init_package_repo(&dep_b, "অন্য", "দ্বিগুণ", 100);

    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    assert!(kolom(&["যোগ", "সহায়ক", &dep_a.to_string_lossy(), "--রেফ", "v1.0.0"], &project).status.success());
    assert!(kolom(&["যোগ", "অন্য", &dep_b.to_string_lossy(), "--রেফ", "v1.0.0"], &project).status.success());
    let install = kolom(&["ইনস্টল"], &project);
    assert!(install.status.success(), "ইনস্টল failed:\n{}", String::from_utf8_lossy(&install.stderr));

    std::fs::write(
        project.join("main.ক"),
        "ইম্পোর্ট সহায়ক\nইম্পোর্ট অন্য\n\nঅ্যাপ {\n    লেখো(সহায়ক.দ্বিগুণ(21))\n    লেখো(অন্য.দ্বিগুণ(21))\n}\n",
    )
    .unwrap();

    let interp = kolom(&["চালাও", "main.ক"], &project);
    assert!(interp.status.success(), "চালাও failed:\n{}", String::from_utf8_lossy(&interp.stderr));
    assert_eq!(String::from_utf8_lossy(&interp.stdout).replace("\r\n", "\n"), "42\n2100\n");

    let build = kolom(&["বিল্ড", "main.ক"], &project);
    assert!(build.status.success(), "বিল্ড failed:\n{}", String::from_utf8_lossy(&build.stderr));
    let exe = String::from_utf8_lossy(&build.stdout).trim().to_string();
    let run = Command::new(&exe).output().expect("failed to run produced exe");
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"), "42\n2100\n");
}

/// A same-directory sibling `.ক` file (no package involved) must keep
/// working exactly as before packages existed — unqualified, flat-merged.
/// Guards the `resolve_user_modules` directory-relative refactor against
/// regressing the one path that predates KPM entirely.
#[test]
fn sibling_file_modules_still_resolve_unqualified() {
    let project = workdir("sibling-unaffected");
    std::fs::write(
        project.join("helper.ক"),
        "ফাংশন তিনগুণ(সংখ্যা x) -> সংখ্যা {\n    রিটার্ন x * ৩\n}\n",
    )
    .unwrap();
    std::fs::write(project.join("main.ক"), "ইম্পোর্ট helper\n\nঅ্যাপ {\n    লেখো(তিনগুণ(7))\n}\n").unwrap();

    let interp = kolom(&["চালাও", "main.ক"], &project);
    assert!(interp.status.success(), "চালাও failed:\n{}", String::from_utf8_lossy(&interp.stderr));
    assert_eq!(String::from_utf8_lossy(&interp.stdout).trim(), "21");
}
