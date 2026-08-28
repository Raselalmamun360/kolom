//! Milestone-5: end-to-end check that `কলম বিল্ড` produces a working native
//! executable with **no C compiler reachable at all**.
//!
//! This is the acceptance test for the whole migration: it runs the real
//! `kolom` binary with a PATH scrubbed of every gcc/clang/cc/cl, so if the
//! build path had any lingering dependency on an external C compiler, it
//! would fail here rather than silently succeeding on a dev machine that
//! happens to have MinGW installed.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the `kolom` binary built alongside this test.
fn kolom_exe() -> PathBuf {
    // .../target/<profile>/deps/build_standalone-<hash>.exe
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

/// A PATH with every directory containing a C compiler removed.
fn path_without_c_compilers() -> String {
    let names = ["gcc", "clang", "cc", "cl"];
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|dir| {
            !names.iter().any(|n| {
                dir.join(format!("{n}{}", std::env::consts::EXE_SUFFIX)).exists()
            })
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(if cfg!(windows) { ";" } else { ":" })
}

/// Assembles a kolom sysroot (runtime + linker + MinGW CRT/import libs) by
/// copying already-built artifacts, so this test exercises the actual
/// shipping configuration rather than a dev checkout's fallbacks.
///
/// This matters because those fallbacks locate MinGW's *libraries* by
/// finding `gcc.exe` on PATH — and this test deliberately removes gcc.
/// Those .a/.o files are data, not a compiler, but a packaged install must
/// carry its own copies rather than assume a compiler is present.
///
/// Deliberately does NOT shell out to cargo: this runs *inside* a cargo
/// test, and a nested `cargo build` blocks on the same target-directory
/// lock. Everything needed must already be built — see the skip message.
fn sysroot() -> Result<PathBuf, String> {
    use std::sync::OnceLock;
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    DIR.get_or_init(build_sysroot).clone()
}

fn build_sysroot() -> Result<PathBuf, String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };

    let runtime = repo.join("target").join("x86_64-pc-windows-gnu").join(profile).join("libkolom_runtime.a");
    if !runtime.exists() {
        return Err(format!(
            "GNU-target runtime not built. Run:\n    \
             cargo build{} -p kolom-runtime --target x86_64-pc-windows-gnu",
            if profile == "release" { " --release" } else { "" }
        ));
    }
    let lld = kolom_codegen_cranelift::link::find_linker().ok_or("rust-lld.exe not found")?;
    let mingw = find_mingw_root().ok_or("MinGW-w64 not found (set MINGW64_ROOT)")?;

    let out = std::env::temp_dir().join("kolom-standalone-sysroot");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(out.join("bin")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out.join("lib")).map_err(|e| e.to_string())?;
    std::fs::copy(&runtime, out.join("libkolom_runtime.a")).map_err(|e| e.to_string())?;
    std::fs::copy(&lld, out.join("bin").join("rust-lld.exe")).map_err(|e| e.to_string())?;

    let ml = mingw.join("x86_64-w64-mingw32").join("lib");
    for f in [
        "crt2.o", "crtbegin.o", "crtend.o",
        "libmingw32.a", "libmingwex.a", "libmoldname.a", "libucrt.a", "libucrtbase.a", "libmsvcrt.a",
        "libkernel32.a", "libuser32.a", "libgdi32.a", "libadvapi32.a", "libshell32.a",
        "libuserenv.a", "libws2_32.a", "libbcrypt.a", "libntdll.a", "libsynchronization.a",
        "libole32.a", "liboleaut32.a",
    ] {
        let src = ml.join(f);
        if src.exists() {
            let _ = std::fs::copy(&src, out.join("lib").join(f));
        }
    }
    // libgcc lives under a version-stamped directory.
    if let Ok(entries) = std::fs::read_dir(mingw.join("lib").join("gcc").join("x86_64-w64-mingw32")) {
        for e in entries.flatten() {
            for f in ["libgcc.a", "libgcc_eh.a"] {
                let src = e.path().join(f);
                if src.exists() {
                    let _ = std::fs::copy(&src, out.join("lib").join(f));
                }
            }
        }
    }
    Ok(out)
}

fn find_mingw_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MINGW64_ROOT") {
        let p = PathBuf::from(p);
        if p.join("x86_64-w64-mingw32").join("lib").is_dir() {
            return Some(p);
        }
    }
    // A MinGW on PATH: <root>/bin/gcc.exe. Read PATH before it is scrubbed.
    for d in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if d.join("gcc.exe").exists() {
            if let Some(root) = d.parent() {
                if root.join("x86_64-w64-mingw32").join("lib").is_dir() {
                    return Some(root.to_path_buf());
                }
            }
        }
    }
    None
}

fn fixture_dir(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden").join(rel)
}

/// Builds `fixture` with a C-compiler-free PATH and asserts its output
/// matches the same `expected.txt` the interpreter is checked against,
/// rather than a hand-written string — a hardcoded expectation here once
/// masked a real digit-formatting divergence from the interpreter.
fn assert_fixture_matches_reference(fixture: &str) {
    let dir = fixture_dir(fixture);
    let expected = std::fs::read_to_string(dir.join("expected.txt"))
        .unwrap_or_else(|e| panic!("{fixture}: cannot read expected.txt: {e}"));
    let got = build_and_run(&dir.join("main.ক"));
    let norm = |s: &str| s.replace("\r\n", "\n");
    assert_eq!(norm(&got), norm(&expected), "{fixture}: output differs from the reference expected.txt");
}

/// Builds `src_path` via the real CLI (with a C-compiler-free PATH) and
/// returns the produced executable's stdout.
fn build_and_run(src_path: &Path) -> String {
    let kolom = kolom_exe();
    assert!(kolom.exists(), "kolom binary not found at {}", kolom.display());
    let sysroot = sysroot().unwrap_or_else(|e| panic!("cannot assemble sysroot: {e}"));

    let out = Command::new(&kolom)
        .arg("বিল্ড")
        .arg(src_path)
        .env("PATH", path_without_c_compilers())
        .env("KOLOM_SYSROOT", &sysroot)
        .output()
        .expect("failed to run kolom");
    assert!(
        out.status.success(),
        "কলম বিল্ড failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // The CLI prints the path of the executable it produced.
    let exe = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(!exe.is_empty(), "kolom printed no output path");
    assert!(Path::new(&exe).exists(), "reported exe does not exist: {exe}");

    let run = Command::new(&exe).output().expect("failed to run produced exe");
    assert!(
        run.status.success(),
        "produced exe exited {:?}:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn build_hello_without_any_c_compiler() {
    assert_fixture_matches_reference("01_hello");
}

#[test]
fn build_functions_without_any_c_compiler() {
    assert_fixture_matches_reference("03_functions");
}

#[test]
fn build_arrays_without_any_c_compiler() {
    assert_fixture_matches_reference("07_arrays");
}

/// Sanity check on the test's own premise: if a C compiler were still
/// reachable through the scrubbed PATH, the tests above would prove nothing.
#[test]
fn scrubbed_path_really_has_no_c_compiler() {
    let scrubbed = path_without_c_compilers();
    for name in ["gcc", "clang", "cc", "cl"] {
        let found = std::env::split_paths(&scrubbed)
            .any(|d| d.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)).exists());
        assert!(!found, "{name} still reachable on the scrubbed PATH");
    }
}
