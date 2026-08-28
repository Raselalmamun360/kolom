#!/usr/bin/env bash
# Assembles a self-contained kolom distribution.
#
#   scripts/make-sysroot.sh [output-dir] [debug|release]
#
# Produces:
#   <out>/kolom.exe
#   <out>/sysroot/libkolom_runtime.a    Kolom runtime (GNU ABI)
#   <out>/sysroot/bin/rust-lld.exe      linker
#   <out>/sysroot/lib/                  MinGW-w64 CRT objects + import libs
#
# Everything copied here is redistributable: MinGW-w64's runtime is
# permissively licensed, and rust-lld is Apache-2.0 WITH LLVM-exception.
# This is why Kolom targets the GNU ABI rather than MSVC — Microsoft's
# Redist.txt does not license its .lib files for redistribution.
set -euo pipefail

OUT="${1:-dist}"
PROFILE="${2:-release}"
TARGET="x86_64-pc-windows-gnu"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

say() { printf '  %s\n' "$*"; }

# --- locate MinGW-w64 -------------------------------------------------------
find_mingw() {
  if [ -n "${MINGW64_ROOT:-}" ] && [ -d "$MINGW64_ROOT/x86_64-w64-mingw32/lib" ]; then
    echo "$MINGW64_ROOT"; return
  fi
  local d
  for d in $(command -v gcc 2>/dev/null || true); do
    local p; p="$(dirname "$(dirname "$d")")"
    [ -d "$p/x86_64-w64-mingw32/lib" ] && { echo "$p"; return; }
  done
  for d in /c/msys64/mingw64 /c/mingw64 \
           /c/Users/*/AppData/Local/Microsoft/WinGet/Packages/*WinLibs*/mingw64; do
    [ -d "$d/x86_64-w64-mingw32/lib" ] && { echo "$d"; return; }
  done
  return 1
}

MINGW="$(find_mingw)" || { echo "error: MinGW-w64 not found. Set MINGW64_ROOT." >&2; exit 1; }
say "mingw:  $MINGW"

LLD="$(find "$(rustc --print sysroot)" -name 'rust-lld.exe' | head -1)"
[ -n "$LLD" ] || { echo "error: rust-lld.exe not found in rustc sysroot" >&2; exit 1; }
say "linker: $LLD"

# --- build ------------------------------------------------------------------
cd "$ROOT"
FLAG=""; [ "$PROFILE" = "release" ] && FLAG="--release"
say "building kolom.exe ($PROFILE)"
cargo build $FLAG -p kolom-cli >/dev/null
say "building runtime for $TARGET ($PROFILE)"
cargo build $FLAG -p kolom-runtime --target "$TARGET" >/dev/null

# --- assemble ---------------------------------------------------------------
rm -rf "$OUT"
mkdir -p "$OUT/sysroot/bin" "$OUT/sysroot/lib"
cp "target/$PROFILE/kolom.exe" "$OUT/"
cp "target/$TARGET/$PROFILE/libkolom_runtime.a" "$OUT/sysroot/"
cp "$LLD" "$OUT/sysroot/bin/"

ML="$MINGW/x86_64-w64-mingw32/lib"
GL="$(ls -d "$MINGW"/lib/gcc/x86_64-w64-mingw32/* 2>/dev/null | tail -1)"

# CRT startup objects — without crt2.o there is no entry point.
for o in crt2.o crtbegin.o crtend.o; do
  cp "$ML/$o" "$OUT/sysroot/lib/"
done
# C runtime + compiler support
for a in libmingw32.a libmingwex.a libmoldname.a libucrt.a libucrtbase.a libmsvcrt.a; do
  [ -f "$ML/$a" ] && cp "$ML/$a" "$OUT/sysroot/lib/"
done
for a in libgcc.a libgcc_eh.a; do
  [ -n "$GL" ] && [ -f "$GL/$a" ] && cp "$GL/$a" "$OUT/sysroot/lib/"
done
# Win32 import libraries the runtime and UI engine need
for a in libkernel32.a libuser32.a libgdi32.a libadvapi32.a libshell32.a \
         libuserenv.a libws2_32.a libbcrypt.a libntdll.a libsynchronization.a \
         libole32.a liboleaut32.a libmsimg32.a libcomdlg32.a; do
  [ -f "$ML/$a" ] && cp "$ML/$a" "$OUT/sysroot/lib/"
done

# --- license notices --------------------------------------------------------
# Apache-2.0 (rust-lld, Cranelift) and MinGW-w64's terms both require their
# notices to travel with the redistributed binaries, so ship them alongside.
for f in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md; do
  [ -f "$f" ] && cp "$f" "$OUT/"
done

say "done -> $OUT  ($(du -sh "$OUT" | cut -f1))"
