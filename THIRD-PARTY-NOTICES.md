# Third-Party Notices

Kolom itself is licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).

A Kolom **distribution** (the `dist/` bundle produced by
`scripts/make-sysroot.sh`) additionally redistributes binaries from the
projects listed below. Their licenses require that these notices travel with
the redistributed files, so this document ships inside every bundle.

---

## 1. LLVM / LLD — `sysroot/bin/rust-lld.exe`

The linker Kolom uses to produce executables. Distributed as part of the Rust
toolchain.

- Project: LLVM Project (LLD linker)
- License: **Apache License 2.0 WITH LLVM-exception**
- https://llvm.org/LICENSE.txt

The LLVM exception grants relief from the Apache-2.0 attribution requirements
for object code embedded into compiled output, and addresses GPLv2
compatibility.

---

## 2. MinGW-w64 — `sysroot/lib/*.a`, `sysroot/lib/*.o`

The C runtime startup objects (`crt2.o`, `crtbegin.o`, `crtend.o`) and Win32
import libraries that Kolom links every native program against.

- Project: MinGW-w64
- License: MinGW-w64 runtime licensing terms — a permissive, redistribution-
  friendly set of licenses (public domain, MIT-like, and ZPL 2.1 for portions
  derived from FreeBSD/Zope contributions)
- https://www.mingw-w64.org/
- Full terms ship as `COPYING.MinGW-w64-runtime.txt` in a MinGW-w64
  installation.

**Why MinGW-w64 rather than the Microsoft ABI:** Microsoft's `Redist.txt` —
the authoritative list of Visual Studio components licensed for
redistribution — does not cover `.lib` import libraries. MinGW-w64's
equivalents are freely redistributable, which is what makes a self-contained
Kolom distribution possible at all. (They are also about ten times smaller.)

---

## 3. Cranelift — compiled into `kolom.exe`

The code generator that turns Kolom programs into machine code.

- Project: Bytecode Alliance — Cranelift
- License: **Apache License 2.0 WITH LLVM-exception**
- https://github.com/bytecodealliance/wasmtime

---

## 4. Rust standard library — linked into `sysroot/libkolom_runtime.a`

The Kolom runtime is written in Rust and statically links parts of Rust's
standard library.

- Project: The Rust Project
- License: **MIT OR Apache-2.0**
- https://github.com/rust-lang/rust

---

## 5. Other Rust crates

Compiled into `kolom.exe` and/or the Kolom runtime. All are permissively
licensed; none are copyleft.

| Crate | License |
|-------|---------|
| `cranelift-codegen`, `-frontend`, `-module`, `-object`, `-native` | Apache-2.0 WITH LLVM-exception |
| `target-lexicon` | Apache-2.0 WITH LLVM-exception |
| `object` | Apache-2.0 OR MIT |
| `windows-sys` | MIT OR Apache-2.0 |

A complete, machine-generated inventory can be produced with
[`cargo-about`](https://github.com/EmbarkStudios/cargo-about) or
[`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny).

---

## Programs you compile with Kolom

Kolom statically links its runtime (`libkolom_runtime.a`) into every program
it builds. Because Kolom is licensed permissively (MIT OR Apache-2.0), **the
programs you compile carry no copyleft obligation** — you may license and
distribute them however you like, including as closed source.

This is a deliberate design decision. A copyleft runtime would force that
license onto every program compiled with the toolchain; it is the reason GCC
ships a dedicated "Runtime Library Exception". Permissive licensing avoids
the problem outright.
