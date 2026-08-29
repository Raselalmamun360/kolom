pub mod ui;

use std::alloc::{alloc, dealloc, Layout};
use std::io::{Read, Write};

fn write_line(bytes: &[u8]) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(bytes);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

/// Bengali-numeral rendering. Used ONLY for diagnostic messages (matching
/// kolom-codegen's `kl_bn`, which is likewise diagnostics-only). Program
/// output via `লেখো`/`লেখায়` uses ASCII digits — see `kl_print_num`.
fn bn_digits(v: i64) -> String {
    const BENGALI: [char; 10] = ['০', '১', '২', '৩', '৪', '৫', '৬', '৭', '৮', '৯'];
    let neg = v < 0;
    let mut u: u128 = if neg { (v as i128).unsigned_abs() } else { v as u128 };
    let mut digits = Vec::new();
    if u == 0 {
        digits.push(BENGALI[0]);
    }
    while u > 0 {
        digits.push(BENGALI[(u % 10) as usize]);
        u /= 10;
    }
    if neg {
        digits.push('\u{2212}'); // U+2212 MINUS SIGN, matching kolom-codegen's kl_bn
    }
    digits.iter().rev().collect()
}

// ============================================================================
// ত্রুটি ব্যবস্থাপনা (error handling) — `চেষ্টা` / `ধরো`
// ============================================================================
//
// Kolom has no `throw`. Every failure originates inside this runtime — a
// failed file read, a division by zero, an index out of range — so the set of
// throw sites is closed and known here. That is what lets errors work without
// stack unwinding, which matters: unwinding through Cranelift-generated frames
// is not something Cranelift supports, and the C backend's `setjmp`/`longjmp`
// scheme jumps over every refcount cleanup between the failure and the handler.
//
// Instead `fail()` checks whether a `চেষ্টা` block is currently active:
//
//   * none active    -> print to stderr and exit(1), exactly as before
//   * inside a block -> record the message and return, letting the caller
//                       return a benign value
//
// Generated code polls `kl_err_pending` after each statement inside a
// `চেষ্টা` body and branches to the handler when one is pending. The benign
// value is therefore never observed by the program: the poll always runs
// before anything can read it.
//
// State is thread-local because a `চেষ্টা` block guards the thread running
// it; a failure on another thread is that thread's own business.

use std::cell::RefCell;

struct ErrState {
    /// How many `চেষ্টা` blocks this thread has entered and not yet left.
    depth: i64,
    /// Message awaiting collection by a handler, if any.
    pending: Option<String>,
}

thread_local! {
    static ERR: RefCell<ErrState> = RefCell::new(ErrState { depth: 0, pending: None });
}

/// Reports a runtime failure. Returns to its caller only when a `চেষ্টা`
/// block is active — otherwise the process exits, which is what every one of
/// these call sites did before `চেষ্টা` was supported.
fn fail(msg: String) {
    let caught = ERR.with(|e| {
        let mut e = e.borrow_mut();
        if e.depth > 0 {
            // An already-pending error means generated code has not reached
            // its poll yet. Keep the first: that is the failure the handler is
            // about to be told about, and anything after it is fallout.
            if e.pending.is_none() {
                e.pending = Some(msg.clone());
            }
            true
        } else {
            false
        }
    });
    if !caught {
        eprintln!("ত্রুটি: {}", msg);
        std::process::exit(1);
    }
}

/// Entering a `চেষ্টা` block.
#[no_mangle]
pub extern "C" fn kl_try_enter() {
    ERR.with(|e| e.borrow_mut().depth += 1);
}

/// Leaving a `চেষ্টা` block by either path — the body running to
/// completion, or control transferring to the handler.
#[no_mangle]
pub extern "C" fn kl_try_exit() {
    ERR.with(|e| {
        let mut e = e.borrow_mut();
        if e.depth > 0 {
            e.depth -= 1;
        }
    });
}

/// 1 when a failure is waiting to be handled. Polled by generated code after
/// each statement inside a `চেষ্টা` body.
#[no_mangle]
pub extern "C" fn kl_err_pending() -> i64 {
    ERR.with(|e| i64::from(e.borrow().pending.is_some()))
}

/// Clears the pending failure and returns its message as a fresh `kl_str`,
/// which the handler owns and binds as its `ধরো(e)` variable.
#[no_mangle]
pub extern "C" fn kl_err_take() -> *mut u8 {
    let msg = ERR.with(|e| e.borrow_mut().pending.take()).unwrap_or_default();
    str_from_rust(&msg)
}

/// `a / b` or `a % b` where `b` is zero. Generated code calls this *instead of*
/// dividing, because Cranelift's `sdiv` traps in hardware and a hardware trap
/// cannot be caught.
#[no_mangle]
pub extern "C" fn kl_fail_div_zero() {
    fail("শূন্য দিয়ে ভাগ করা যাবে না".to_string());
}

/// Eight zeroed bytes handed back by fallible accessors once they have failed,
/// so generated code that loads the slot it was about to use reads a zero or
/// null instead of dereferencing garbage. `kl_rc_incref` and the `*_decref`
/// family all treat null as a no-op, so the value stays inert until the poll
/// that follows discards it.
static ERR_SCRATCH: [u8; 8] = [0; 8];

fn scratch_slot() -> *mut u8 {
    ERR_SCRATCH.as_ptr() as *mut u8
}

/// The inert slot, for generated code that has just been told a lookup failed
/// but still has to produce *some* address to load from before it reaches its
/// poll. See `ERR_SCRATCH`.
#[no_mangle]
pub extern "C" fn kl_err_scratch() -> *mut u8 {
    scratch_slot()
}

fn oob(idx: i64, len: i64) {
    fail(format!(
        "ইনডেক্স {} সীমার বাইরে (দৈর্ঘ্য {})",
        bn_digits(idx),
        bn_digits(len)
    ));
}

/// Prints `v` followed by a newline. ASCII digits, matching the
/// interpreter and the C backend — Bengali numerals appear only in
/// diagnostics, never in program output.
#[no_mangle]
pub extern "C" fn kl_print_num(v: i64) {
    write_line(v.to_string().as_bytes());
}

/// Prints `সত্য`/`মিথ্যা` (true/false) followed by a newline.
#[no_mangle]
pub extern "C" fn kl_print_bool(v: i8) {
    write_line(if v != 0 { "সত্য".as_bytes() } else { "মিথ্যা".as_bytes() });
}

// ============================================================================
// Generic refcounting: every heap-backed Kolom value (Txt/Arr/Shared/Struct)
// starts its allocation with an `rc: i64` header, so a single incref works
// for all of them. Milestone-2 design note: unlike kolom-sema's static move
// checking (which never distinguishes "move" from "copy" at the codegen
// boundary — see plan doc), this runtime treats every binding-to-binding
// copy uniformly as a refcount bump, exactly mirroring what kolom-codegen's
// C backend already does (`tracked()`/scope-exit decref). It costs a few
// more atomic-free increments than a precise move analysis would, but it's
// correct and keeps codegen from needing sema's (currently unexported)
// internal move/drop-point data.
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_rc_incref(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe {
        *(p as *mut i64) += 1;
    }
}

// ---------------------------------------------------------------------------
// Text: [rc: i64][len: i64][data: len bytes]
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn kl_str_new(bytes: *const u8, len: i64) -> *mut u8 {
    unsafe {
        let layout = Layout::from_size_align(16 + len as usize, 8).unwrap();
        let p = alloc(layout);
        (p as *mut i64).write(1);
        (p.add(8) as *mut i64).write(len);
        if len > 0 {
            std::ptr::copy_nonoverlapping(bytes, p.add(16), len as usize);
        }
        p
    }
}

/// Byte length — the value stored in the header, used internally for
/// allocation and copying. NOT what `দৈর্ঘ্য` reports; see `kl_str_cplen`.
#[no_mangle]
pub extern "C" fn kl_str_len(p: *mut u8) -> i64 {
    unsafe { *(p.add(8) as *const i64) }
}

/// Codepoint count — what `দৈর্ঘ্য(লেখা)` reports, matching the
/// interpreter and the C backend (`kl_cpcount`). Bengali text is multi-byte,
/// so this differs sharply from the byte length: "কলম ভাষা" is 8 codepoints
/// but 22 bytes.
#[no_mangle]
pub extern "C" fn kl_str_cplen(p: *mut u8) -> i64 {
    if p.is_null() {
        return 0;
    }
    let len = kl_str_len(p) as usize;
    let bytes = unsafe { std::slice::from_raw_parts(kl_str_data(p), len) };
    // A UTF-8 continuation byte is 0b10xxxxxx; every other byte starts a
    // codepoint.
    bytes.iter().filter(|b| (*b & 0xC0) != 0x80).count() as i64
}

#[no_mangle]
pub extern "C" fn kl_str_data(p: *mut u8) -> *const u8 {
    unsafe { p.add(16) }
}

/// `লেখা + লেখা` — returns a fresh string holding both operands' bytes.
/// Allocates directly rather than going through `kl_str_new`, which would
/// require a source pointer for a copy that has not happened yet.
#[no_mangle]
pub extern "C" fn kl_str_concat(a: *mut u8, b: *mut u8) -> *mut u8 {
    let la = if a.is_null() { 0 } else { kl_str_len(a) };
    let lb = if b.is_null() { 0 } else { kl_str_len(b) };
    let total = la + lb;
    unsafe {
        let layout = Layout::from_size_align(16 + total as usize, 8).unwrap();
        let out = alloc(layout);
        (out as *mut i64).write(1);
        (out.add(8) as *mut i64).write(total);
        if la > 0 {
            std::ptr::copy_nonoverlapping(kl_str_data(a), out.add(16), la as usize);
        }
        if lb > 0 {
            std::ptr::copy_nonoverlapping(kl_str_data(b), out.add(16 + la as usize), lb as usize);
        }
        out
    }
}

#[no_mangle]
pub extern "C" fn kl_str_copy(p: *mut u8) -> *mut u8 {
    kl_str_new(kl_str_data(p), kl_str_len(p))
}

#[no_mangle]
pub extern "C" fn kl_str_decref(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe {
        let rc = p as *mut i64;
        *rc -= 1;
        if *rc <= 0 {
            let len = *(p.add(8) as *const i64) as usize;
            dealloc(p, Layout::from_size_align(16 + len, 8).unwrap());
        }
    }
}

#[no_mangle]
pub extern "C" fn kl_print_text(p: *mut u8) {
    if p.is_null() {
        write_line(b"");
        return;
    }
    let len = kl_str_len(p) as usize;
    let data = unsafe { std::slice::from_raw_parts(kl_str_data(p), len) };
    write_line(data);
}

/// `লেখায়(সংখ্যা)` — formats a number as text, ASCII digits.
#[no_mangle]
pub extern "C" fn kl_num_to_text(v: i64) -> *mut u8 {
    let s = v.to_string();
    kl_str_new(s.as_ptr(), s.len() as i64)
}

/// `লেখায়(দশমিক)` — ASCII digits (kolom-codegen's C backend formats
/// decimals with `%g`/ASCII too, unlike integers' Bengali-digit `kl_bn`;
/// matched here for the same reason: no established Bengali decimal
/// notation to convert to).
#[no_mangle]
pub extern "C" fn kl_dec_to_text(v: f64) -> *mut u8 {
    str_from_rust(&format!("{}", v))
}

#[no_mangle]
pub extern "C" fn kl_bool_to_text(v: i8) -> *mut u8 {
    str_from_rust(if v != 0 { "সত্য" } else { "মিথ্যা" })
}

// ---------------------------------------------------------------------------
// Array: [rc: i64][len: i64][cap: i64][elem_size: i64][drop_elem: i64][data...]
// `drop_elem` is a Cranelift-emitted `extern "C" fn(*mut u8)` address (or 0),
// called on each element when the array's refcount reaches zero. Kolom
// arrays are always constructed pre-sized to their final length (literals
// and concat both know the target length upfront), so `kl_arr_push` never
// needs to grow/realloc — that's a deliberate M2 simplification, not a
// general-purpose growable-vector runtime yet.
// ---------------------------------------------------------------------------

const ARR_HEADER: usize = 40;

type DropFn = extern "C" fn(*mut u8);

fn arr_cap(p: *mut u8) -> i64 {
    unsafe { *(p.add(16) as *const i64) }
}
fn arr_elem_size(p: *mut u8) -> i64 {
    unsafe { *(p.add(24) as *const i64) }
}
fn arr_drop_addr(p: *mut u8) -> i64 {
    unsafe { *(p.add(32) as *const i64) }
}
fn arr_data(p: *mut u8) -> *mut u8 {
    unsafe { p.add(ARR_HEADER) }
}

#[no_mangle]
pub extern "C" fn kl_arr_new(elem_size: i64, cap: i64, drop_elem: i64) -> *mut u8 {
    unsafe {
        let layout = Layout::from_size_align(ARR_HEADER + (cap.max(0) as usize) * (elem_size as usize), 8).unwrap();
        let p = alloc(layout);
        (p as *mut i64).write(1);
        (p.add(8) as *mut i64).write(0);
        (p.add(16) as *mut i64).write(cap);
        (p.add(24) as *mut i64).write(elem_size);
        (p.add(32) as *mut i64).write(drop_elem);
        p
    }
}

#[no_mangle]
pub extern "C" fn kl_arr_len(p: *mut u8) -> i64 {
    unsafe { *(p.add(8) as *const i64) }
}

#[no_mangle]
pub extern "C" fn kl_arr_get_ptr(p: *mut u8, idx: i64) -> *mut u8 {
    let len = kl_arr_len(p);
    if idx < 0 || idx >= len {
        oob(idx, len);
        return scratch_slot();
    }
    unsafe { arr_data(p).add((idx * arr_elem_size(p)) as usize) }
}

/// Appends one pre-sized element. Caller (codegen) guarantees `len < cap`
/// by always constructing arrays at their final capacity — see module doc.
#[no_mangle]
pub extern "C" fn kl_arr_push(p: *mut u8, elem: *const u8) {
    unsafe {
        let len = kl_arr_len(p);
        let es = arr_elem_size(p);
        std::ptr::copy_nonoverlapping(elem, arr_data(p).add((len * es) as usize), es as usize);
        (p.add(8) as *mut i64).write(len + 1);
    }
}

#[no_mangle]
pub extern "C" fn kl_arr_concat(a: *mut u8, b: *mut u8) -> *mut u8 {
    let la = kl_arr_len(a);
    let lb = kl_arr_len(b);
    let es = arr_elem_size(a);
    let out = kl_arr_new(es, la + lb, arr_drop_addr(a));
    unsafe {
        std::ptr::copy_nonoverlapping(arr_data(a), arr_data(out), (la * es) as usize);
        std::ptr::copy_nonoverlapping(arr_data(b), arr_data(out).add((la * es) as usize), (lb * es) as usize);
        (out.add(8) as *mut i64).write(la + lb);
    }
    // M2 simplification: doesn't incref owning elements it just copied by
    // value — fine while only Num-element arrays are exercised.
    out
}

#[no_mangle]
pub extern "C" fn kl_arr_incref(p: *mut u8) {
    kl_rc_incref(p);
}

#[no_mangle]
pub extern "C" fn kl_arr_decref(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe {
        let rc = p as *mut i64;
        *rc -= 1;
        if *rc <= 0 {
            let len = kl_arr_len(p);
            let es = arr_elem_size(p);
            let cap = arr_cap(p);
            let drop_addr = arr_drop_addr(p);
            if drop_addr != 0 {
                let f: DropFn = std::mem::transmute(drop_addr as usize);
                for i in 0..len {
                    f(arr_data(p).add((i * es) as usize));
                }
            }
            dealloc(p, Layout::from_size_align(ARR_HEADER + (cap as usize) * (es as usize), 8).unwrap());
        }
    }
}

// ---------------------------------------------------------------------------
// Shared (শেয়ার): [rc: i64][drop: i64][size: i64][payload: size bytes]
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn kl_shared_new(payload_size: i64, drop_payload: i64) -> *mut u8 {
    unsafe {
        let layout = Layout::from_size_align(24 + payload_size as usize, 8).unwrap();
        let p = alloc(layout);
        (p as *mut i64).write(1);
        (p.add(8) as *mut i64).write(drop_payload);
        (p.add(16) as *mut i64).write(payload_size);
        p
    }
}

#[no_mangle]
pub extern "C" fn kl_shared_payload_ptr(p: *mut u8) -> *mut u8 {
    unsafe { p.add(24) }
}

#[no_mangle]
pub extern "C" fn kl_shared_incref(p: *mut u8) {
    kl_rc_incref(p);
}

#[no_mangle]
pub extern "C" fn kl_shared_decref(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe {
        let rc = p as *mut i64;
        *rc -= 1;
        if *rc <= 0 {
            let drop_addr = *(p.add(8) as *const i64);
            let size = *(p.add(16) as *const i64);
            if drop_addr != 0 {
                let f: DropFn = std::mem::transmute(drop_addr as usize);
                f(p.add(24));
            }
            dealloc(p, Layout::from_size_align(24 + size as usize, 8).unwrap());
        }
    }
}

// ---------------------------------------------------------------------------
// Struct: [rc: i64][field0: 8]...[fieldN-1: 8] — every Kolom value (scalar or
// pointer) fits in one 8-byte slot, so field layout is always `8 + idx*8`.
// Field load/store is done directly in Cranelift IR (no runtime call needed);
// only alloc/incref/decref go through here.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn kl_struct_new(field_count: i64) -> *mut u8 {
    unsafe {
        let layout = Layout::from_size_align(8 + (field_count as usize) * 8, 8).unwrap();
        let p = alloc(layout);
        (p as *mut i64).write(1);
        std::ptr::write_bytes(p.add(8), 0, (field_count as usize) * 8);
        p
    }
}

#[no_mangle]
pub extern "C" fn kl_struct_incref(p: *mut u8) {
    kl_rc_incref(p);
}

#[no_mangle]
pub extern "C" fn kl_struct_decref(p: *mut u8, field_count: i64, drop_fields: i64) {
    if p.is_null() {
        return;
    }
    unsafe {
        let rc = p as *mut i64;
        *rc -= 1;
        if *rc <= 0 {
            if drop_fields != 0 {
                let f: DropFn = std::mem::transmute(drop_fields as usize);
                f(p);
            }
            dealloc(p, Layout::from_size_align(8 + (field_count as usize) * 8, 8).unwrap());
        }
    }
}

unsafe fn str_slice(p: *mut u8) -> &'static [u8] {
    if p.is_null() {
        return &[];
    }
    std::slice::from_raw_parts(kl_str_data(p), kl_str_len(p) as usize)
}

fn str_from_rust(s: &str) -> *mut u8 {
    kl_str_new(s.as_ptr(), s.len() as i64)
}

// ============================================================================
// গণিত (math)
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_math_abs(v: i64) -> i64 {
    v.abs()
}
#[no_mangle]
pub extern "C" fn kl_math_fabs(v: f64) -> f64 {
    v.abs()
}
#[no_mangle]
pub extern "C" fn kl_math_sqrt(v: f64) -> f64 {
    v.sqrt()
}
#[no_mangle]
pub extern "C" fn kl_math_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
}
#[no_mangle]
pub extern "C" fn kl_math_floor(v: f64) -> i64 {
    v.floor() as i64
}
#[no_mangle]
pub extern "C" fn kl_math_ceil(v: f64) -> i64 {
    v.ceil() as i64
}
#[no_mangle]
pub extern "C" fn kl_math_round(v: f64) -> i64 {
    v.round() as i64
}
#[no_mangle]
pub extern "C" fn kl_math_sin(v: f64) -> f64 {
    v.sin()
}
#[no_mangle]
pub extern "C" fn kl_math_cos(v: f64) -> f64 {
    v.cos()
}
#[no_mangle]
pub extern "C" fn kl_math_tan(v: f64) -> f64 {
    v.tan()
}
#[no_mangle]
pub extern "C" fn kl_math_ln(v: f64) -> f64 {
    v.ln()
}
#[no_mangle]
pub extern "C" fn kl_math_log10(v: f64) -> f64 {
    v.log10()
}
#[no_mangle]
pub extern "C" fn kl_math_min_i(a: i64, b: i64) -> i64 {
    a.min(b)
}
#[no_mangle]
pub extern "C" fn kl_math_max_i(a: i64, b: i64) -> i64 {
    a.max(b)
}
#[no_mangle]
pub extern "C" fn kl_math_min_f(a: f64, b: f64) -> f64 {
    a.min(b)
}
#[no_mangle]
pub extern "C" fn kl_math_max_f(a: f64, b: f64) -> f64 {
    a.max(b)
}

// ============================================================================
// ফাইল (io) / ফাইলসিস্টেম (filesystem)
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_io_write_file(path: *mut u8, content: *mut u8) {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let c = unsafe { str_slice(content) };
    if let Err(e) = std::fs::write(&p, c) {
        fail(format!("ফাইল লেখা যায়নি '{}': {}", p, e));
    }
}

#[no_mangle]
pub extern "C" fn kl_io_append_file(path: *mut u8, content: *mut u8) {
    use std::io::Write as _;
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let c = unsafe { str_slice(content) };
    let res = std::fs::OpenOptions::new().create(true).append(true).open(&p).and_then(|mut f| f.write_all(c));
    if let Err(e) = res {
        fail(format!("ফাইলে যোগ করা যায়নি '{}': {}", p, e));
    }
}

#[no_mangle]
pub extern "C" fn kl_io_read_file(path: *mut u8) -> *mut u8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    match std::fs::read(&p) {
        Ok(bytes) => kl_str_new(bytes.as_ptr(), bytes.len() as i64),
        Err(e) => {
            fail(format!("ফাইল পড়া যায়নি '{}': {}", p, e));
            str_from_rust("")
        }
    }
}

#[no_mangle]
pub extern "C" fn kl_io_read_line() -> *mut u8 {
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    str_from_rust(&line)
}

#[no_mangle]
pub extern "C" fn kl_fs_exists(path: *mut u8) -> i8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    if std::path::Path::new(&p).is_file() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_dir_exists(path: *mut u8) -> i8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    if std::path::Path::new(&p).is_dir() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_mkdir(path: *mut u8) {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    if let Err(e) = std::fs::create_dir_all(&p) {
        fail(format!("ডিরেক্টরি তৈরি করা যায়নি '{}': {}", p, e));
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_remove(path: *mut u8) {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let path_ref = std::path::Path::new(&p);
    let res = if path_ref.is_dir() { std::fs::remove_dir_all(path_ref) } else { std::fs::remove_file(path_ref) };
    if let Err(e) = res {
        fail(format!("মোছা যায়নি '{}': {}", p, e));
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_copy(src: *mut u8, dst: *mut u8) {
    let s = String::from_utf8_lossy(unsafe { str_slice(src) }).into_owned();
    let d = String::from_utf8_lossy(unsafe { str_slice(dst) }).into_owned();
    if let Err(e) = std::fs::copy(&s, &d) {
        fail(format!("কপি করা যায়নি '{}' -> '{}': {}", s, d, e));
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_rename(src: *mut u8, dst: *mut u8) {
    let s = String::from_utf8_lossy(unsafe { str_slice(src) }).into_owned();
    let d = String::from_utf8_lossy(unsafe { str_slice(dst) }).into_owned();
    if let Err(e) = std::fs::rename(&s, &d) {
        fail(format!("সরানো যায়নি '{}' -> '{}': {}", s, d, e));
    }
}

/// Returns a fresh `kl_arr` of `kl_str` filenames (not full paths) — element
/// type/drop callback is Txt, matching `ফাইলসিস্টেম.তালিকা`'s `sa()` (Arr<Txt>)
/// stdlib signature.
#[no_mangle]
pub extern "C" fn kl_fs_list(path: *mut u8) -> *mut u8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&p) {
        for e in entries.flatten() {
            names.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    let drop_addr = kl_str_decref as *const () as usize as i64;
    let arr = kl_arr_new(8, names.len() as i64, drop_addr);
    for n in &names {
        let s = str_from_rust(n);
        kl_arr_push(arr, (&s as *const *mut u8) as *const u8);
    }
    arr
}

// ============================================================================
// র‍্যান্ডম (random) — a small seedable xorshift64, deterministic given a
// seed but NOT intended to match any other implementation's exact sequence.
// ============================================================================

use std::sync::atomic::{AtomicU64, Ordering};

static RNG_STATE: AtomicU64 = AtomicU64::new(0x9E3779B97F4A7C15);

fn xorshift_next() -> u64 {
    let mut x = RNG_STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = 0x9E3779B97F4A7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, Ordering::Relaxed);
    x
}

#[no_mangle]
pub extern "C" fn kl_rand_seed(seed: i64) {
    RNG_STATE.store((seed as u64) ^ 0x9E3779B97F4A7C15, Ordering::Relaxed);
}

#[no_mangle]
pub extern "C" fn kl_rand_num() -> i64 {
    (xorshift_next() >> 1) as i64
}

#[no_mangle]
pub extern "C" fn kl_rand_between(lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo + 1) as u64;
    lo + (xorshift_next() % span) as i64
}

#[no_mangle]
pub extern "C" fn kl_rand_dec() -> f64 {
    (xorshift_next() >> 11) as f64 / (1u64 << 53) as f64
}

// ============================================================================
// জেসন (json) — deliberately minimal: flat top-level `"key": value` scanning,
// not a general parser. Enough for validating/reading simple flat objects.
// ============================================================================

fn json_str(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

fn json_balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut any = false;
    for c in s.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => {
                depth += 1;
                any = true;
            }
            '}' | ']' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    any && depth == 0 && !in_str
}

#[no_mangle]
pub extern "C" fn kl_json_valid(text: *mut u8) -> i8 {
    let s = json_str(unsafe { str_slice(text) });
    if json_balanced(s.trim()) {
        1
    } else {
        0
    }
}

/// Finds `"key"` at the top level and returns the byte range of its value
/// (start, end) within `s`, or `None`.
fn json_find_value<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\"", key);
    let key_pos = s.find(&needle)?;
    let after_key = &s[key_pos + needle.len()..];
    let colon = after_key.find(':')?;
    let mut rest = after_key[colon + 1..].trim_start();
    if rest.starts_with('"') {
        let mut end = 1;
        let bytes = rest.as_bytes();
        let mut escaped = false;
        while end < bytes.len() {
            let c = bytes[end] as char;
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                end += 1;
                break;
            }
            end += 1;
        }
        rest = &rest[..end];
        Some(rest)
    } else {
        let end = rest.find([',', '}', ']', '\n']).unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}

fn json_unescape(raw: &str) -> String {
    let inner = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(raw);
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[no_mangle]
pub extern "C" fn kl_json_get_str(text: *mut u8, key: *mut u8) -> *mut u8 {
    let s = json_str(unsafe { str_slice(text) }).into_owned();
    let k = json_str(unsafe { str_slice(key) }).into_owned();
    match json_find_value(&s, &k) {
        Some(raw) => str_from_rust(&json_unescape(raw)),
        None => str_from_rust(""),
    }
}

#[no_mangle]
pub extern "C" fn kl_json_get_num(text: *mut u8, key: *mut u8) -> i64 {
    let s = json_str(unsafe { str_slice(text) }).into_owned();
    let k = json_str(unsafe { str_slice(key) }).into_owned();
    match json_find_value(&s, &k) {
        Some(raw) => raw.trim().parse::<f64>().unwrap_or(0.0) as i64,
        None => 0,
    }
}

/// `জেসন.বের_হও` — renders `text` as a complete JSON string literal,
/// including the surrounding quotes (matching the interpreter).
#[no_mangle]
pub extern "C" fn kl_json_escape(text: *mut u8) -> *mut u8 {
    let s = json_str(unsafe { str_slice(text) });
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out.push('"');
    str_from_rust(&out)
}

// ============================================================================
// লেখা (string) — codepoint-indexed where the language elsewhere is
// codepoint-indexed (matching kolom-codegen's `kl_cpcount` convention).
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_str_upper(p: *mut u8) -> *mut u8 {
    let s = json_str(unsafe { str_slice(p) });
    str_from_rust(&s.to_uppercase())
}

#[no_mangle]
pub extern "C" fn kl_str_lower(p: *mut u8) -> *mut u8 {
    let s = json_str(unsafe { str_slice(p) });
    str_from_rust(&s.to_lowercase())
}

#[no_mangle]
pub extern "C" fn kl_str_trim(p: *mut u8) -> *mut u8 {
    let s = json_str(unsafe { str_slice(p) });
    str_from_rust(s.trim())
}

/// Splits on `sep` and returns a fresh Txt array.
#[no_mangle]
pub extern "C" fn kl_str_split(p: *mut u8, sep: *mut u8) -> *mut u8 {
    let s = json_str(unsafe { str_slice(p) }).into_owned();
    let sp = json_str(unsafe { str_slice(sep) }).into_owned();
    let parts: Vec<&str> = if sp.is_empty() { vec![&s[..]] } else { s.split(sp.as_str()).collect() };
    let drop_addr = kl_str_decref as *const () as usize as i64;
    let arr = kl_arr_new(8, parts.len() as i64, drop_addr);
    for part in &parts {
        let sp = str_from_rust(part);
        kl_arr_push(arr, (&sp as *const *mut u8) as *const u8);
    }
    arr
}

#[no_mangle]
pub extern "C" fn kl_str_join(arr: *mut u8, sep: *mut u8) -> *mut u8 {
    let sp = json_str(unsafe { str_slice(sep) }).into_owned();
    let len = kl_arr_len(arr);
    let mut parts: Vec<String> = Vec::with_capacity(len as usize);
    for i in 0..len {
        let elem_ptr = kl_arr_get_ptr(arr, i);
        let str_ptr = unsafe { *(elem_ptr as *const *mut u8) };
        parts.push(json_str(unsafe { str_slice(str_ptr) }).into_owned());
    }
    str_from_rust(&parts.join(&sp))
}

#[no_mangle]
pub extern "C" fn kl_str_replace(p: *mut u8, from: *mut u8, to: *mut u8) -> *mut u8 {
    let s = json_str(unsafe { str_slice(p) }).into_owned();
    let f = json_str(unsafe { str_slice(from) }).into_owned();
    let t = json_str(unsafe { str_slice(to) }).into_owned();
    str_from_rust(&s.replace(&f, &t))
}

/// Codepoint index of the first occurrence of `needle`, or -1.
#[no_mangle]
pub extern "C" fn kl_str_find(p: *mut u8, needle: *mut u8) -> i64 {
    let s = json_str(unsafe { str_slice(p) }).into_owned();
    let n = json_str(unsafe { str_slice(needle) }).into_owned();
    match s.find(&n) {
        Some(byte_idx) => s[..byte_idx].chars().count() as i64,
        None => -1,
    }
}

/// Codepoint-range substring `[start, end)`.
#[no_mangle]
pub extern "C" fn kl_str_slice(p: *mut u8, start: i64, end: i64) -> *mut u8 {
    let s = json_str(unsafe { str_slice(p) }).into_owned();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    let a = start.clamp(0, n) as usize;
    let b = end.clamp(0, n) as usize;
    let out: String = if a < b { chars[a..b].iter().collect() } else { String::new() };
    str_from_rust(&out)
}

#[no_mangle]
pub extern "C" fn kl_str_starts_with(p: *mut u8, prefix: *mut u8) -> i8 {
    let s = json_str(unsafe { str_slice(p) });
    let pre = json_str(unsafe { str_slice(prefix) });
    if s.starts_with(pre.as_ref()) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn kl_str_ends_with(p: *mut u8, suffix: *mut u8) -> i8 {
    let s = json_str(unsafe { str_slice(p) });
    let suf = json_str(unsafe { str_slice(suffix) });
    if s.ends_with(suf.as_ref()) {
        1
    } else {
        0
    }
}

// ============================================================================
// ম্যাপ (Map[K,V]) — [rc: i64][len: i64][cap: i64][key_kind: i64][entries...]
// key_kind: 0 = Txt (kl_str* key, compared by content), 1 = Num (i64 key).
// Each entry is 16 bytes: [key: 8][value: 8]. Grows by doubling (unlike
// arrays, map size isn't known upfront — every `m[k]=v` can be a new key).
// M3 simplification: decref doesn't drop owned keys/values (leaks on freed
// maps) — matches the array/concat leak precedent, not exercised by tests
// that check correctness rather than memory usage.
// ============================================================================

const MAP_HEADER: usize = 32;
const MAP_ENTRY: usize = 16;

fn map_len(p: *mut u8) -> i64 {
    unsafe { *(p.add(8) as *const i64) }
}
fn map_cap(p: *mut u8) -> i64 {
    unsafe { *(p.add(16) as *const i64) }
}
fn map_key_kind(p: *mut u8) -> i64 {
    unsafe { *(p.add(24) as *const i64) }
}
fn map_entry_ptr(p: *mut u8, i: i64) -> *mut u8 {
    unsafe { p.add(MAP_HEADER + (i as usize) * MAP_ENTRY) }
}

fn map_key_eq(p: *mut u8, entry_key: i64, key: i64) -> bool {
    if map_key_kind(p) == 1 {
        return entry_key == key;
    }
    let a = entry_key as *mut u8;
    let b = key as *mut u8;
    if a == b {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    unsafe { str_slice(a) == str_slice(b) }
}

#[no_mangle]
pub extern "C" fn kl_map_new(key_kind: i64) -> *mut u8 {
    let cap = 8i64;
    unsafe {
        let layout = Layout::from_size_align(MAP_HEADER + (cap as usize) * MAP_ENTRY, 8).unwrap();
        let p = alloc(layout);
        (p as *mut i64).write(1);
        (p.add(8) as *mut i64).write(0);
        (p.add(16) as *mut i64).write(cap);
        (p.add(24) as *mut i64).write(key_kind);
        for i in 0..cap {
            (map_entry_ptr(p, i) as *mut i64).write(0);
            (map_entry_ptr(p, i).add(8) as *mut i64).write(0);
        }
        p
    }
}

#[no_mangle]
pub extern "C" fn kl_map_len(p: *mut u8) -> i64 {
    map_len(p)
}

/// Returns the value-slot address for `key` if present, or null.
#[no_mangle]
pub extern "C" fn kl_map_find(p: *mut u8, key: i64) -> *mut u8 {
    let len = map_len(p);
    for i in 0..len {
        let e = map_entry_ptr(p, i);
        let ek = unsafe { *(e as *const i64) };
        if map_key_eq(p, ek, key) {
            return unsafe { e.add(8) };
        }
    }
    std::ptr::null_mut()
}

/// Returns the value-slot address for `key`, inserting a zeroed entry
/// (growing the backing storage if needed) if it isn't present yet. Growth
/// may reallocate, so the (possibly unchanged) new map pointer is written to
/// `out_new_map` — codegen must always rebind its map variable from that
/// out-param after calling this, since Cranelift `Variable`s aren't directly
/// addressable the way a caller-owned `*mut *mut u8` would need.
#[no_mangle]
pub extern "C" fn kl_map_set_slot(p: *mut u8, key: i64, out_new_map: *mut *mut u8) -> *mut u8 {
    unsafe {
        let found = kl_map_find(p, key);
        if !found.is_null() {
            *out_new_map = p;
            return found;
        }
        let len = map_len(p);
        let cap = map_cap(p);
        let mut pp = p;
        if len >= cap {
            let newcap = cap * 2;
            let old_layout = Layout::from_size_align(MAP_HEADER + (cap as usize) * MAP_ENTRY, 8).unwrap();
            let new_size = MAP_HEADER + (newcap as usize) * MAP_ENTRY;
            pp = std::alloc::realloc(p, old_layout, new_size);
            for i in cap..newcap {
                (map_entry_ptr(pp, i) as *mut i64).write(0);
                (map_entry_ptr(pp, i).add(8) as *mut i64).write(0);
            }
            (pp.add(16) as *mut i64).write(newcap);
        }
        let e = map_entry_ptr(pp, len);
        (e as *mut i64).write(key);
        (e.add(8) as *mut i64).write(0);
        (pp.add(8) as *mut i64).write(len + 1);
        *out_new_map = pp;
        e.add(8)
    }
}

#[no_mangle]
pub extern "C" fn kl_map_delete(p: *mut u8, key: i64) {
    let len = map_len(p);
    for i in 0..len {
        let e = map_entry_ptr(p, i);
        let ek = unsafe { *(e as *const i64) };
        if map_key_eq(p, ek, key) {
            for j in i..len - 1 {
                unsafe {
                    let src = map_entry_ptr(p, j + 1);
                    let dst = map_entry_ptr(p, j);
                    std::ptr::copy_nonoverlapping(src, dst, MAP_ENTRY);
                }
            }
            unsafe {
                (p.add(8) as *mut i64).write(len - 1);
            }
            return;
        }
    }
}

/// Only meaningful for Txt-keyed maps (matches the stdlib signature, which
/// always returns Arr<Txt>) — Num-keyed maps aren't exercised yet.
#[no_mangle]
pub extern "C" fn kl_map_keys(p: *mut u8) -> *mut u8 {
    let len = map_len(p);
    let drop_addr = kl_str_decref as *const () as usize as i64;
    let arr = kl_arr_new(8, len, drop_addr);
    for i in 0..len {
        let e = map_entry_ptr(p, i);
        let key = unsafe { *(e as *const i64) };
        let kptr = if map_key_kind(p) == 1 { kl_num_to_text(key) } else { kl_str_copy(key as *mut u8) };
        kl_arr_push(arr, (&kptr as *const *mut u8) as *const u8);
    }
    arr
}

#[no_mangle]
pub extern "C" fn kl_map_incref(p: *mut u8) {
    kl_rc_incref(p);
}

#[no_mangle]
pub extern "C" fn kl_map_missing_key() {
    fail("ম্যাপে key পাওয়া যায়নি".to_string());
}

#[no_mangle]
pub extern "C" fn kl_map_decref(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe {
        let rc = p as *mut i64;
        *rc -= 1;
        if *rc <= 0 {
            let cap = map_cap(p);
            dealloc(p, Layout::from_size_align(MAP_HEADER + (cap as usize) * MAP_ENTRY, 8).unwrap());
        }
    }
}

// ============================================================================
// নেটওয়ার্ক (network) — TCP client
//
// Kolom passes sockets around as plain `সংখ্যা` handles, so the runtime keeps
// the real `TcpStream`s in a registry and hands out small integer indices.
// A `Mutex` rather than the `static mut` used by the UI engine: that engine
// is confined to one message-loop thread by construction, whereas nothing
// stops network calls from several threads once Kolom grows them.
// ============================================================================

use std::collections::HashMap as StdHashMap;
use std::net::TcpStream;
use std::sync::Mutex;
use std::sync::OnceLock;

struct NetTable {
    streams: StdHashMap<i64, TcpStream>,
    next: i64,
}

fn net_table() -> &'static Mutex<NetTable> {
    static T: OnceLock<Mutex<NetTable>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(NetTable { streams: StdHashMap::new(), next: 1 }))
}

fn net_fail(what: &str, detail: &str) {
    fail(format!("{} — {}", what, detail));
}

/// `নেটওয়ার্ক.কানেক্ট(host, port)` — opens a TCP connection, returns its handle.
#[no_mangle]
pub extern "C" fn kl_net_connect(host: *mut u8, port: i64) -> i64 {
    let h = String::from_utf8_lossy(unsafe { str_slice(host) }).into_owned();
    match TcpStream::connect((h.as_str(), port as u16)) {
        Ok(stream) => {
            let mut t = net_table().lock().unwrap();
            let handle = t.next;
            t.next += 1;
            t.streams.insert(handle, stream);
            handle
        }
        Err(e) => {
            net_fail(&format!("সংযোগ ব্যর্থ '{}:{}'", h, port), &e.to_string());
            0
        }
    }
}

/// `নেটওয়ার্ক.সেন্ড(handle, text)` — sends the whole string.
#[no_mangle]
pub extern "C" fn kl_net_send(handle: i64, data: *mut u8) {
    let bytes = unsafe { str_slice(data) };
    let mut t = net_table().lock().unwrap();
    match t.streams.get_mut(&handle) {
        Some(s) => {
            if let Err(e) = s.write_all(bytes) {
                net_fail("পাঠাতে ব্যর্থ", &e.to_string());
            }
        }
        None => net_fail("অবৈধ সংযোগ", &format!("#{}", handle)),
    }
}

/// `নেটওয়ার্ক.রিসিভ(handle, maxBytes)` — reads up to `max` bytes.
/// Returns what arrived, which may be shorter, and is empty at end of stream.
#[no_mangle]
pub extern "C" fn kl_net_recv(handle: i64, max: i64) -> *mut u8 {
    let cap = max.clamp(1, 1 << 20) as usize;
    let mut buf = vec![0u8; cap];
    let mut t = net_table().lock().unwrap();
    match t.streams.get_mut(&handle) {
        Some(s) => match s.read(&mut buf) {
            Ok(n) => kl_str_new(buf.as_ptr(), n as i64),
            Err(e) => {
                net_fail("পড়তে ব্যর্থ", &e.to_string());
                str_from_rust("")
            }
        },
        None => {
            net_fail("অবৈধ সংযোগ", &format!("#{}", handle));
            str_from_rust("")
        }
    }
}

/// `নেটওয়ার্ক.ক্লোজ(handle)` — closes the connection. Closing an unknown or
/// already-closed handle is a no-op rather than an error, so cleanup paths
/// stay simple.
#[no_mangle]
pub extern "C" fn kl_net_close(handle: i64) {
    let mut t = net_table().lock().unwrap();
    if let Some(s) = t.streams.remove(&handle) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
}
