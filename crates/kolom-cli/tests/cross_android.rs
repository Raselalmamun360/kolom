//! Cross-compilation check: `কলম বিল্ড <file> android` must produce a
//! statically-linked aarch64 Android ELF whose output matches the
//! interpreter's reference `expected.txt` exactly.
//!
//! Needs an installed NDK to build at all (`ANDROID_NDK_HOME`, or
//! `ANDROID_HOME`/`ANDROID_SDK_ROOT` with an `ndk/` subdirectory) and a
//! connected device or emulator (`adb devices`) to run the result. Either
//! missing piece is reported and skipped rather than failed — this is the
//! one target that can never be fully self-contained the way Windows and
//! Linux are (see `link.rs`'s module docs), so CI and most dev machines
//! will only ever exercise the "ELF was produced correctly" half.
//!
//! Verified once against real hardware while this was written: two genuine
//! memory-corruption bugs in `kl_arr_decref`/`kl_shared_decref` (passing a
//! storage slot's *address* to the element/payload drop callback instead of
//! the pointer *stored* there) had been silently tolerated by Windows'
//! allocator and were caught immediately by Android's Scudo — a heap
//! corruption that had nothing to do with the Android target itself.

use std::path::{Path, PathBuf};
use std::process::Command;

fn kolom_exe() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden").join(name)
}

fn android_runtime_available() -> bool {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    ["release", "debug"]
        .iter()
        .any(|p| repo.join("target").join("aarch64-linux-android").join(p).join("libkolom_runtime.a").exists())
}

fn ndk_available() -> bool {
    ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"].iter().any(|v| std::env::var_os(v).is_some_and(|p| !p.is_empty()))
        || ["ANDROID_HOME", "ANDROID_SDK_ROOT"].iter().any(|v| std::env::var_os(v).is_some_and(|p| !p.is_empty()))
}

/// Builds `fixture` for Android; returns the produced binary's path.
fn build_for_android(fixture: &str) -> PathBuf {
    let src = fixture_dir(fixture).join("main.ক");
    let out = Command::new(kolom_exe()).arg("বিল্ড").arg(&src).arg("android").output().expect("failed to run kolom");
    assert!(
        out.status.success(),
        "{fixture}: কলম বিল্ড ... android failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    assert!(p.exists(), "{fixture}: reported binary does not exist: {}", p.display());
    p
}

/// Verifies the file really is a static aarch64 ELF, by reading the header
/// rather than trusting the linker's exit code.
fn assert_static_aarch64_elf(fixture: &str, bin: &Path) {
    let bytes = std::fs::read(bin).expect("cannot read produced binary");
    assert!(bytes.len() > 20, "{fixture}: binary suspiciously small");
    assert_eq!(&bytes[0..4], b"\x7fELF", "{fixture}: not an ELF file");
    assert_eq!(bytes[4], 2, "{fixture}: not 64-bit");
    assert_eq!(bytes[5], 1, "{fixture}: not little-endian");
    let e_type = u16::from_le_bytes([bytes[16], bytes[17]]);
    assert_eq!(e_type, 2, "{fixture}: expected ET_EXEC (static), got e_type={e_type}");
    // e_machine == EM_AARCH64 (183)
    let e_machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    assert_eq!(e_machine, 183, "{fixture}: expected aarch64, got e_machine={e_machine}");
}

fn adb() -> Option<PathBuf> {
    for var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"] {
        // adb ships with the SDK's platform-tools, not the NDK, but a
        // sibling `platform-tools/adb` next to a `Sdk/ndk/<version>` layout
        // covers the common install shape without needing a third env var.
        if let Ok(ndk) = std::env::var(var) {
            let mut p = PathBuf::from(&ndk);
            for _ in 0..3 {
                let candidate = p.join("platform-tools").join(format!("adb{}", std::env::consts::EXE_SUFFIX));
                if candidate.exists() {
                    return Some(candidate);
                }
                if !p.pop() {
                    break;
                }
            }
        }
    }
    which_on_path("adb")
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let p = dir.join(&exe);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn device_connected(adb: &Path) -> bool {
    let Ok(out) = Command::new(adb).arg("devices").output() else { return false };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .any(|l| l.split_whitespace().nth(1) == Some("device"))
}

/// Pushes `bin` to `/data/local/tmp`, runs it, and returns its stdout.
fn run_on_device(adb: &Path, fixture: &str, bin: &Path) -> Option<String> {
    let remote = format!("/data/local/tmp/kolom-test-{fixture}");
    let push = Command::new(adb).arg("push").arg(bin).arg(&remote).output().ok()?;
    if !push.status.success() {
        return None;
    }
    let _ = Command::new(adb).arg("shell").arg("chmod").arg("755").arg(&remote).output();
    let run = Command::new(adb).arg("shell").arg(&remote).output().ok()?;
    let _ = Command::new(adb).arg("shell").arg("rm").arg("-f").arg(&remote).output();
    Some(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"))
}

fn check(fixture: &str) {
    if !ndk_available() {
        eprintln!("skip {fixture}: no Android NDK found (set ANDROID_NDK_HOME)");
        return;
    }
    if !android_runtime_available() {
        eprintln!(
            "skip {fixture}: Android runtime not built. Run:\n    \
             cargo build --release -p kolom-runtime --target aarch64-linux-android"
        );
        return;
    }
    let bin = build_for_android(fixture);
    assert_static_aarch64_elf(fixture, &bin);

    let Some(adb) = adb() else {
        eprintln!("skip {fixture} (execution only): adb not found; ELF was verified");
        return;
    };
    if !device_connected(&adb) {
        eprintln!("skip {fixture} (execution only): no device/emulator connected; ELF was verified");
        return;
    }
    let Some(got) = run_on_device(&adb, fixture, &bin) else {
        eprintln!("skip {fixture} (execution only): could not run on device; ELF was verified");
        return;
    };
    let expected = std::fs::read_to_string(fixture_dir(fixture).join("expected.txt")).expect("cannot read expected.txt");
    let norm = |s: &str| s.replace("\r\n", "\n");
    assert_eq!(norm(&got), norm(&expected), "{fixture}: Android output differs from the interpreter's reference");
}

#[test]
fn android_hello() {
    check("01_hello");
}

#[test]
fn android_variables() {
    check("02_variables");
}

#[test]
fn android_control_flow() {
    check("04_control_flow");
}

#[test]
fn android_arrays() {
    check("07_arrays");
}

#[test]
fn android_struct_type() {
    check("30_struct_type");
}

/// The exact fixture that first exposed the `kl_arr_decref` bug (an array of
/// লেখা that is never even printed still corrupts the heap when it goes out
/// of scope) — see this file's module docs.
#[test]
fn android_print_containers() {
    check("40_print_containers");
}

/// A struct-typed `শেয়ার` cell — exercises `kl_shared_decref`'s half of the
/// same bug class.
#[test]
fn android_struct_containers() {
    check("39_struct_containers");
}
