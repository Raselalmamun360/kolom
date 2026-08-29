//! Target selection and linking for Cranelift-emitted object code — the
//! step that replaces "shell out to whatever gcc/clang/cc is on PATH".
//!
//! Nothing here compiles anything. `rust-lld` is a *linker*: it combines
//! already-generated machine code (our object file + the prebuilt
//! kolom-runtime static library + system import libraries) into a PE
//! executable. That is the same job the old backend delegated to `cc`,
//! minus the "compile C source" half, which Cranelift now does in-process.
//!
//! # Why the GNU ABI
//!
//! Kolom targets `x86_64-pc-windows-gnu` (MinGW-w64), not the MSVC ABI,
//! for one decisive reason: **redistribution**. Linking against the MSVC
//! ABI requires Microsoft's Windows SDK import libraries (`kernel32.Lib`,
//! `libcmt.lib`, …), and Microsoft's own `Redist.txt` — the authoritative
//! list of code licensed for redistribution — does not list `.lib` files
//! at all. Those libraries are meant for building on a machine that has
//! the SDK installed; shipping them inside a kolom distribution is not
//! something that list permits.
//!
//! MinGW-w64's equivalents are freely redistributable, and as a bonus are
//! roughly ten times smaller (~5 MB against ~51 MB), so a self-contained
//! kolom install is both legally shippable and materially lighter.
//!
//! # The kolom sysroot
//!
//! Like rustc — which ships its standard library and linker in a sysroot
//! next to the compiler rather than embedding them — kolom looks for its
//! support files in a sysroot directory:
//!
//! ```text
//! <dir of kolom.exe>/
//!   kolom.exe
//!   sysroot/
//!     libkolom_runtime.a    the Kolom runtime (required)
//!     bin/rust-lld.exe      the bundled linker (optional; see below)
//!     lib/*.a, *.o          MinGW-w64 CRT objects and import libraries
//! ```
//!
//! Each piece has a development-time fallback so a `cargo build` checkout
//! works with no install step:
//! - **runtime**: falls back to `target/<profile>/` for the GNU target.
//! - **linker**: falls back to `rust-lld` inside the active rustc sysroot,
//!   which every Rust toolchain ships.
//! - **CRT/import libs**: falls back to an installed MinGW-w64.
//!
//! Populating `sysroot/bin` and `sysroot/lib` at package time is what makes
//! an installed kolom fully self-contained. `KOLOM_SYSROOT` overrides the
//! sysroot location.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A platform Kolom can produce native executables for.
///
/// Both choices are driven by redistributability: MinGW-w64 on Windows
/// (see module docs) and musl on Linux. musl is MIT-licensed and built for
/// static linking, so a Kolom binary is one self-contained ELF that runs on
/// any distribution, with no glibc version coupling and no LGPL question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// x86_64 Windows, MinGW-w64 ABI.
    WindowsGnu,
    /// x86_64 Linux, statically linked against musl.
    LinuxMusl,
}

impl Target {
    pub fn triple(self) -> &'static str {
        match self {
            Target::WindowsGnu => "x86_64-pc-windows-gnu",
            Target::LinuxMusl => "x86_64-unknown-linux-musl",
        }
    }

    /// Maps a `কলম বিল্ড <file> <target>` name to a target.
    pub fn from_name(name: &str) -> Option<Target> {
        match name {
            "windows" => Some(Target::WindowsGnu),
            "linux" => Some(Target::LinuxMusl),
            _ => None,
        }
    }

    /// The platform this build of kolom runs on, used when no target is given.
    pub fn host() -> Target {
        if cfg!(windows) { Target::WindowsGnu } else { Target::LinuxMusl }
    }

    pub fn exe_suffix(self) -> &'static str {
        match self {
            Target::WindowsGnu => ".exe",
            Target::LinuxMusl => "",
        }
    }
}

/// Backwards-compatible alias for the default target's triple.
pub const TARGET_TRIPLE: &str = "x86_64-pc-windows-gnu";

/// What a link attempt needed but could not find, with a Bengali message
/// aimed at the person running `কলম বিল্ড`.
#[derive(Debug)]
pub struct LinkError(pub String);

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for LinkError {}

fn err<T>(msg: impl Into<String>) -> Result<T, LinkError> {
    Err(LinkError(msg.into()))
}

/// Cranelift ISA builder for the Kolom target. Explicitly requests the GNU
/// triple rather than the host's, so object files carry the ABI the linker
/// below expects, regardless of which toolchain built `kolom` itself.
pub fn isa_builder(target: Target) -> Result<cranelift_codegen::isa::Builder, String> {
    let triple: target_lexicon::Triple =
        target.triple().parse().map_err(|e| format!("অবৈধ টার্গেট ট্রিপল: {:?}", e))?;
    cranelift_codegen::isa::lookup(triple).map_err(|e| format!("Cranelift টার্গেট সমর্থন করে না: {}", e))
}

/// Expands one path pattern segment-by-segment; `"*"` matches any entry.
/// Used to find versioned SDK/toolchain directories without a glob crate
/// (kolom's workspace deliberately keeps its dependency set small).
fn glob(base: &Path, segments: &[&str]) -> Vec<PathBuf> {
    let mut current = vec![base.to_path_buf()];
    for seg in segments {
        let mut next = Vec::new();
        for dir in &current {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if *seg == "*" || name.eq_ignore_ascii_case(seg) {
                    next.push(entry.path());
                }
            }
        }
        current = next;
    }
    current
}

/// The kolom sysroot: `$KOLOM_SYSROOT`, else `<dir of kolom.exe>/sysroot`.
pub fn sysroot() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KOLOM_SYSROOT") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("sysroot"))
}

const RUNTIME_LIB_NAME: &str = "libkolom_runtime.a";

/// Locates the Kolom runtime for `target`: sysroot first, then the cargo
/// target directory (dev checkout).
///
/// A sysroot may hold runtimes for several targets, so each lives under its
/// own triple directory; a bare `libkolom_runtime.a` at the sysroot root is
/// still accepted for the host target, which is how single-target bundles
/// produced before cross-compilation existed are laid out.
pub fn find_runtime_lib(target: Target) -> Option<PathBuf> {
    if let Some(sr) = sysroot() {
        let per_target = sr.join(target.triple()).join(RUNTIME_LIB_NAME);
        if per_target.exists() {
            return Some(per_target);
        }
        if target == Target::host() {
            let flat = sr.join(RUNTIME_LIB_NAME);
            if flat.exists() {
                return Some(flat);
            }
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..4 {
        for profile in ["release", "debug"] {
            let p = dir.join("target").join(target.triple()).join(profile).join(RUNTIME_LIB_NAME);
            if p.exists() {
                return Some(p);
            }
            let p2 = dir.join(target.triple()).join(profile).join(RUNTIME_LIB_NAME);
            if p2.exists() {
                return Some(p2);
            }
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

#[cfg(windows)]
const RUST_LLD_NAME: &str = "rust-lld.exe";
#[cfg(not(windows))]
const RUST_LLD_NAME: &str = "rust-lld";

/// Locates `rust-lld` — a single self-contained executable, so bundling it
/// into a kolom sysroot is one file copy. (The `gcc-ld/ld.lld` alongside it
/// in a Rust toolchain is only a thin wrapper that re-invokes `rust-lld`
/// through a relative path, so it does NOT survive being copied alone.)
pub fn find_linker() -> Option<PathBuf> {
    if let Some(sr) = sysroot() {
        let p = sr.join("bin").join(RUST_LLD_NAME);
        if p.exists() {
            return Some(p);
        }
    }
    let out = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;
    let rust_sysroot = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if rust_sysroot.is_empty() {
        return None;
    }
    glob(Path::new(&rust_sysroot), &["lib", "rustlib", "*", "bin", RUST_LLD_NAME]).pop()
}

/// Directories holding the target's CRT startup objects and libraries.
///
/// Windows: MinGW-w64. Linux: the musl libc that ships *inside the Rust
/// toolchain* (`.../rustlib/<triple>/lib/self-contained/`), which is why
/// Linux builds need nothing installed beyond Rust itself.
pub fn find_lib_dirs(target: Target) -> Vec<PathBuf> {
    if let Some(sr) = sysroot() {
        let per_target = sr.join(target.triple()).join("lib");
        if per_target.is_dir() {
            return vec![per_target];
        }
        if target == Target::host() {
            let flat = sr.join("lib");
            if flat.is_dir() {
                return vec![flat];
            }
        }
    }
    match target {
        Target::LinuxMusl => rust_self_contained_dir(target).into_iter().collect(),
        Target::WindowsGnu => find_mingw_lib_dirs(),
    }
}

/// `<rustc sysroot>/lib/rustlib/<triple>/lib/self-contained` — where Rust
/// keeps musl's `crt1.o`/`crti.o`/`crtn.o`/`libc.a`.
pub fn rust_self_contained_dir(target: Target) -> Option<PathBuf> {
    let out = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;
    let root = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if root.is_empty() {
        return None;
    }
    let p = Path::new(&root)
        .join("lib").join("rustlib").join(target.triple()).join("lib").join("self-contained");
    p.is_dir().then_some(p)
}

fn find_mingw_lib_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("MINGW64_ROOT") {
        if !p.trim().is_empty() {
            roots.push(PathBuf::from(p));
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&paths) {
            if d.join("gcc.exe").exists() {
                if let Some(parent) = d.parent() {
                    roots.push(parent.to_path_buf());
                }
            }
        }
    }
    for r in [r"C:\msys64\mingw64", r"C:\mingw64"] {
        roots.push(PathBuf::from(r));
    }
    for root in roots {
        let libdir = root.join("x86_64-w64-mingw32").join("lib");
        if libdir.is_dir() {
            dirs.push(libdir);
            if let Some(g) = glob(&root.join("lib").join("gcc").join("x86_64-w64-mingw32"), &["*"]).pop() {
                dirs.push(g);
            }
            break;
        }
    }
    dirs
}

/// Windows system libraries, in GNU link order. The UI engine needs
/// gdi32/user32; the rest back Rust's `std`.
const WINDOWS_LIBS: &[&str] = &[
    "-lmingw32", "-lgcc", "-lgcc_eh", "-lmoldname", "-lmingwex", "-lucrt",
    "-lkernel32", "-luser32", "-lgdi32", "-ladvapi32", "-lshell32",
    "-luserenv", "-lws2_32", "-lbcrypt", "-lntdll", "-lsynchronization",
    "-lole32", "-loleaut32",
];

/// Links `obj_path` into `exe_path` for `target`.
pub fn link_executable_for(target: Target, obj_path: &Path, exe_path: &Path) -> Result<(), LinkError> {
    let linker = match find_linker() {
        Some(l) => l,
        None => {
            return err(
                "লিংকার পাওয়া যায়নি — kolom sysroot-এ 'bin/rust-lld' রাখো,                  অথবা Rust টুলচেইন ইনস্টল করো (KOLOM_SYSROOT দিয়ে পথ বদলানো যায়)",
            )
        }
    };
    let runtime = match find_runtime_lib(target) {
        Some(r) => r,
        None => {
            return err(format!(
                "'{}' টার্গেটের কলম রানটাইম পাওয়া যায়নি — বিল্ড করো:
                     cargo build --release -p kolom-runtime --target {}",
                target.triple(), target.triple()
            ))
        }
    };
    let lib_dirs = find_lib_dirs(target);
    if lib_dirs.is_empty() {
        return err(match target {
            Target::WindowsGnu => "MinGW-w64 লাইব্রেরি পাওয়া যায়নি — kolom sysroot-এ 'lib/' ফোল্ডারে রাখো, অথবা MinGW-w64 ইনস্টল করো (MINGW64_ROOT দিয়ে পথ দেওয়া যায়)".to_string(),
            Target::LinuxMusl => format!(
                "musl লাইব্রেরি পাওয়া যায়নি — যোগ করো:
    rustup target add {}",
                target.triple()
            ),
        });
    }

    let find_obj = |name: &str| lib_dirs.iter().map(|d| d.join(name)).find(|p| p.exists());
    let mut cmd = Command::new(&linker);
    cmd.arg("-flavor").arg("gnu").arg("-o").arg(exe_path);
    for d in &lib_dirs {
        cmd.arg(format!("-L{}", d.display()));
    }

    match target {
        Target::WindowsGnu => {
            let crt2 = match find_obj("crt2.o") {
                Some(p) => p,
                None => return err("MinGW-এর 'crt2.o' পাওয়া যায়নি — sysroot/lib অসম্পূর্ণ"),
            };
            cmd.arg("-m").arg("i386pep").arg("--subsystem").arg("console");
            cmd.arg(&crt2);
            if let Some(p) = find_obj("crtbegin.o") {
                cmd.arg(p);
            }
            cmd.arg(obj_path).arg(&runtime);
            for l in WINDOWS_LIBS {
                cmd.arg(l);
            }
            if let Some(p) = find_obj("crtend.o") {
                cmd.arg(p);
            }
        }
        Target::LinuxMusl => {
            // Fully static: one ELF with no shared-library dependencies, so
            // the binary runs on any distribution.
            let crt1 = match find_obj("crt1.o") {
                Some(p) => p,
                None => return err("musl-এর 'crt1.o' পাওয়া যায়নি — sysroot/lib অসম্পূর্ণ"),
            };
            cmd.arg("-m").arg("elf_x86_64").arg("-static");
            cmd.arg(&crt1);
            if let Some(p) = find_obj("crti.o") {
                cmd.arg(p);
            }
            cmd.arg(obj_path).arg(&runtime);
            cmd.arg("-lc");
            if let Some(p) = find_obj("libunwind.a") {
                cmd.arg(p);
            }
            if let Some(p) = find_obj("crtn.o") {
                cmd.arg(p);
            }
        }
    }

    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => err(format!(
            "লিংক ব্যর্থ (কোড {}):
{}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => err(format!("লিংকার '{}' চালানো যায়নি: {}", linker.display(), e)),
    }
}

/// Links for the host platform.
pub fn link_executable(obj_path: &Path, exe_path: &Path) -> Result<(), LinkError> {
    link_executable_for(Target::host(), obj_path, exe_path)
}
