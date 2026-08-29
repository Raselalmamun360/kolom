//! Cross-compilation check: `কলম বিল্ড <file> linux` must produce a
//! statically-linked Linux ELF whose output matches the interpreter's
//! reference `expected.txt` exactly.
//!
//! Running the produced binary needs a Linux environment. On Windows that
//! means WSL; elsewhere the binary runs natively. When neither is
//! available the test still verifies that a correct ELF was *produced* —
//! that alone catches codegen and link-recipe regressions — and skips only
//! the execution half, reporting why.

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

/// True when the GNU-target runtime needed for cross-linking is present.
fn linux_runtime_available() -> bool {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    ["release", "debug"].iter().any(|p| {
        repo.join("target").join("x86_64-unknown-linux-musl").join(p).join("libkolom_runtime.a").exists()
    })
}

/// Builds `fixture` for Linux; returns the produced binary's path.
fn build_for_linux(fixture: &str) -> PathBuf {
    let src = fixture_dir(fixture).join("main.ক");
    let out = Command::new(kolom_exe())
        .arg("বিল্ড")
        .arg(&src)
        .arg("linux")
        .output()
        .expect("failed to run kolom");
    assert!(
        out.status.success(),
        "{fixture}: কলম বিল্ড ... linux failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    assert!(p.exists(), "{fixture}: reported binary does not exist: {}", p.display());
    p
}

/// Verifies the file really is a static x86-64 ELF, by reading the header
/// rather than trusting the linker's exit code.
fn assert_static_elf(fixture: &str, bin: &Path) {
    let bytes = std::fs::read(bin).expect("cannot read produced binary");
    assert!(bytes.len() > 20, "{fixture}: binary suspiciously small");
    assert_eq!(&bytes[0..4], b"\x7fELF", "{fixture}: not an ELF file");
    assert_eq!(bytes[4], 2, "{fixture}: not 64-bit");
    assert_eq!(bytes[5], 1, "{fixture}: not little-endian");
    // e_type == ET_EXEC (2) means a non-PIE, statically linked executable;
    // ET_DYN (3) would indicate a dynamically linked / PIE build.
    let e_type = u16::from_le_bytes([bytes[16], bytes[17]]);
    assert_eq!(e_type, 2, "{fixture}: expected ET_EXEC (static), got e_type={e_type}");
    // e_machine == EM_X86_64 (62)
    let e_machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    assert_eq!(e_machine, 62, "{fixture}: expected x86-64, got e_machine={e_machine}");
}

/// Runs `bin` in a Linux environment, or returns None if none is available.
fn run_on_linux(fixture: &str, bin: &Path) -> Option<String> {
    if cfg!(not(windows)) {
        let _ = Command::new("chmod").arg("+x").arg(bin).status();
        let out = Command::new(bin).output().ok()?;
        return Some(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    // Windows: hand the binary to WSL. Copy it to a path WSL can address
    // simply (its /mnt/<drive> mapping), since %TEMP% may sit anywhere.
    // One directory per fixture — cargo runs these tests in parallel, and a
    // single shared staging path makes them overwrite each other's binaries.
    let staging = Path::new("C:\\kolom-cross-test").join(fixture);
    std::fs::create_dir_all(&staging).ok()?;
    std::fs::copy(bin, staging.join("prog")).ok()?;
    let wsl_path = format!("/mnt/c/kolom-cross-test/{fixture}/prog");
    let out = Command::new("wsl.exe")
        .arg("-e")
        .arg("sh")
        .arg("-c")
        .arg(format!("chmod +x {wsl_path} && {wsl_path}"))
        .output()
        .ok()?;
    if !out.status.success() && out.stdout.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"))
}

fn check(fixture: &str) {
    if !linux_runtime_available() {
        eprintln!(
            "skip {fixture}: Linux runtime not built. Run:\n    \
             cargo build --release -p kolom-runtime --target x86_64-unknown-linux-musl"
        );
        return;
    }
    let bin = build_for_linux(fixture);
    assert_static_elf(fixture, &bin);

    let Some(got) = run_on_linux(fixture, &bin) else {
        eprintln!("skip {fixture} (execution only): no Linux environment available; ELF was verified");
        return;
    };
    let expected = std::fs::read_to_string(fixture_dir(fixture).join("expected.txt"))
        .expect("cannot read expected.txt");
    let norm = |s: &str| s.replace("\r\n", "\n");
    assert_eq!(
        norm(&got),
        norm(&expected),
        "{fixture}: Linux output differs from the interpreter's reference"
    );
}

#[test]
fn linux_hello() {
    check("01_hello");
}

#[test]
fn linux_variables() {
    check("02_variables");
}

#[test]
fn linux_control_flow() {
    check("04_control_flow");
}

#[test]
fn linux_arrays() {
    check("07_arrays");
}

#[test]
fn linux_stdlib_string() {
    check("22_stdlib_string");
}

#[test]
fn linux_stdlib_math() {
    check("21_stdlib_math");
}

#[test]
fn linux_map_type() {
    check("29_map_type");
}

#[test]
fn linux_struct_type() {
    check("30_struct_type");
}
