//! The legacy `--সি` backend must refuse a cross target rather than quietly
//! building for the host.
//!
//! `kolom-codegen` uses the target name only to pick console vs Win32 output;
//! it never selects a toolchain. So before this guard, `কলম বিল্ড --সি
//! main.ক android` on Windows compiled the generated C with whatever host
//! `gcc` was on PATH and handed back a **PE32+ Windows executable** — one
//! that ran fine on Windows, with nothing anywhere to suggest it was not an
//! Android build. `linux` did the same.
//!
//! This is the C-backend counterpart of the check that already existed on the
//! default path, where an unsupported target is rejected outright.

use std::path::PathBuf;
use std::process::Command;

fn kolom_exe() -> PathBuf {
    // .../target/<profile>/deps/cross_guard-<hash>.exe
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/01_hello/main.ক")
}

/// Builds `01_hello` for `target` through the C backend, with KLOM_CC cleared
/// so no cross compiler is nominated.
fn build_for(target: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join("kolom-cross-guard").join(target);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("main.ক");
    std::fs::copy(fixture(), &src).unwrap();

    Command::new(kolom_exe())
        .arg("বিল্ড")
        .arg("--সি")
        .arg(&src)
        .arg(target)
        .env_remove("KLOM_CC")
        .current_dir(&dir)
        .output()
        .expect("failed to run kolom")
}

fn assert_refused(target: &str) {
    let out = build_for(target);
    assert!(
        !out.status.success(),
        "{target}: expected a refusal, but the build reported success:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("KLOM_CC"),
        "{target}: the refusal should say how to supply a cross compiler, got: {stderr:?}"
    );
}

#[test]
fn c_backend_refuses_android_without_a_cross_compiler() {
    assert_refused("android");
}

#[test]
fn c_backend_refuses_linux_cross_without_a_cross_compiler() {
    // Only meaningful when Linux is not the host; there it is an ordinary
    // native build and should be allowed.
    if cfg!(target_os = "linux") {
        return;
    }
    assert_refused("linux");
}

/// The guard keys off the target differing from the host, so a plain
/// host-target build must still work with nothing but a host compiler —
/// otherwise this would have made `--সি` useless rather than honest.
#[test]
fn c_backend_still_builds_for_the_host() {
    let host = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let out = build_for(host);
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // A machine with no C compiler at all cannot run this one; that is a
        // missing prerequisite, not the guard rejecting the target.
        if stderr.contains("কোনো C কম্পাইলার পাওয়া যায়নি") {
            return;
        }
        panic!("host build failed for a reason other than a missing compiler:\n{stderr}");
    }
}
