//! Milestone-0 spike: proves a Kolom program can go from Cranelift-emitted
//! object code to a running native .exe without ever invoking gcc/clang/cc/cl
//! (or searching PATH for one, the way the legacy C backend does).
//!
//! Unlike the other test files this one hand-builds its Cranelift function
//! rather than parsing source, so it stays a direct check of the
//! object-emission + link mechanism itself.
//!
//! The only external tool involved is the linker, which merely combines
//! already-compiled machine code — see `kolom_codegen_cranelift::link`.

use std::process::Command;

#[test]
fn hello_world_links_and_runs_without_external_c_compiler() {
    let out_dir = std::env::temp_dir().join("kolom-cranelift-spike");
    std::fs::create_dir_all(&out_dir).unwrap();
    let obj_path = out_dir.join("hello.obj");
    let exe_path = out_dir.join("hello.exe");

    kolom_codegen_cranelift::build_hello_object(&obj_path, "হ্যালো বিশ্ব").expect("cranelift object emission failed");
    assert!(obj_path.exists(), "object file was not written");

    kolom_codegen_cranelift::link_executable(&obj_path, &exe_path).expect("link failed");
    assert!(exe_path.exists(), "linked exe was not produced");

    let run = Command::new(&exe_path).output().expect("failed to run produced exe");
    assert!(run.status.success(), "produced exe exited non-zero");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("হ্যালো বিশ্ব"), "unexpected output: {stdout:?}");
}
