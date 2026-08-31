pub mod ui;

use std::alloc::{alloc, dealloc, Layout};
use std::io::{Read, Write};

fn write_line(bytes: &[u8]) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(bytes);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

/// Bengali-numeral rendering. Used ONLY for diagnostic messages. Program
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
        digits.push('\u{2212}'); // U+2212 MINUS SIGN
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
// copy uniformly as a refcount bump (incref on read, decref at scope exit).
// It costs a few more atomic-free increments than a precise move analysis
// would, but it's correct and keeps codegen from needing sema's (currently
// unexported) internal move/drop-point data.
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

/// `লেখায়(দশমিক)` — ASCII digits, unlike integers' Bengali-digit `kl_bn`:
/// no established Bengali decimal notation to convert to.
#[no_mangle]
pub extern "C" fn kl_dec_to_text(v: f64) -> *mut u8 {
    str_from_rust(&format!("{}", v))
}

/// `সংখ্যায়`/`দশমিকে` — Kolom source itself accepts both Bengali (২৫) and
/// ASCII (25) numeral literals, so a command-line argument or file content
/// a Bengali-typing user produced plausibly uses Bengali digits too. Rust's
/// `str::parse` only understands ASCII, so map ০-৯ to 0-9 first.
fn bengali_digits_to_ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '০'..='৯' => char::from(b'0' + (c as u32 - '০' as u32) as u8),
            other => other,
        })
        .collect()
}

/// `লেখায়`-এর বিপরীত — `লেখা` পার্স করে `সংখ্যা`। ব্যর্থ হলে (অসংখ্যাসূচক
/// ইনপুট) `চেষ্টা`/`ধরো`-catchable রানটাইম ত্রুটি, নীরবে ০ নয়।
#[no_mangle]
pub extern "C" fn kl_parse_num(p: *mut u8) -> i64 {
    let s = String::from_utf8_lossy(unsafe { str_slice(p) }).into_owned();
    match bengali_digits_to_ascii(s.trim()).parse::<i64>() {
        Ok(n) => n,
        Err(_) => {
            fail(format!("'{}' সংখ্যা নয়", s));
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn kl_parse_dec(p: *mut u8) -> f64 {
    let s = String::from_utf8_lossy(unsafe { str_slice(p) }).into_owned();
    match bengali_digits_to_ascii(s.trim()).parse::<f64>() {
        Ok(n) => n,
        Err(_) => {
            fail(format!("'{}' দশমিক নয়", s));
            0.0
        }
    }
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
                    // Each slot holds an 8-byte *pointer value* (every owning
                    // element type is a single heap pointer) — the drop
                    // function must be called on that stored pointer, not on
                    // the address of the slot holding it. Passing the slot
                    // address silently corrupted the element in place (it
                    // was treated as an rc-header pointer and decremented),
                    // which Windows' allocator tolerated well enough to
                    // never visibly crash, but which Android's Scudo
                    // allocator reliably caught as a heap corruption a few
                    // frees later, at a call site that had nothing to do
                    // with the array that actually caused it.
                    let elem_slot = arr_data(p).add((i * es) as usize) as *const *mut u8;
                    f(*elem_slot);
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
                // Same bug, same fix as `kl_arr_decref`: the payload slot
                // holds a pointer value (owning inner types are always one
                // heap pointer), so the drop function needs that stored
                // pointer, not the address of the slot.
                let f: DropFn = std::mem::transmute(drop_addr as usize);
                let payload_slot = p.add(24) as *const *mut u8;
                f(*payload_slot);
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
/// `a % b` on দশমিক. Cranelift has no float-remainder instruction (nor does
/// the underlying hardware), so this backs the `%` operator via Rust's `%`
/// on `f64` — the same operator the interpreter uses, so the two agree.
#[no_mangle]
pub extern "C" fn kl_math_fmod(a: f64, b: f64) -> f64 {
    a % b
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
        fail(format!("ফাইলে এপেন্ড করা যায়নি '{}': {}", p, e));
    }
}

/// Same `kl_arr<kl_str>`-building pattern as `kl_fs_list` below, but of
/// content lines rather than directory entries.
#[no_mangle]
pub extern "C" fn kl_io_read_lines(path: *mut u8) -> *mut u8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let lines: Vec<String> = match std::fs::read_to_string(&p) {
        Ok(content) => content.lines().map(|l| l.to_string()).collect(),
        Err(e) => {
            fail(format!("ফাইল পড়া যায়নি '{}': {}", p, e));
            Vec::new()
        }
    };
    let drop_addr = kl_str_decref as *const () as usize as i64;
    let arr = kl_arr_new(8, lines.len() as i64, drop_addr);
    for l in &lines {
        let s = str_from_rust(l);
        kl_arr_push(arr, (&s as *const *mut u8) as *const u8);
    }
    arr
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

/// File-only — errors on a directory rather than silently recursing, so
/// `ফাইলসিস্টেম.মুছো` behaves identically to the interpreter and can't
/// surprise-delete a whole tree. Recursive delete is `kl_fs_rmdir_all`
/// (`ডিরেক্টরি_মুছো`), its own explicit, opt-in name.
#[no_mangle]
pub extern "C" fn kl_fs_remove(path: *mut u8) {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    if let Err(e) = std::fs::remove_file(&p) {
        fail(format!("মোছা যায়নি '{}': {}", p, e));
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_rmdir_all(path: *mut u8) {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    if let Err(e) = std::fs::remove_dir_all(&p) {
        fail(format!("ডিরেক্টরি মুছতে ব্যর্থ '{}': {}", p, e));
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

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

#[no_mangle]
pub extern "C" fn kl_fs_copy_dir_all(src: *mut u8, dst: *mut u8) {
    let s = String::from_utf8_lossy(unsafe { str_slice(src) }).into_owned();
    let d = String::from_utf8_lossy(unsafe { str_slice(dst) }).into_owned();
    if let Err(e) = copy_dir_recursive(std::path::Path::new(&s), std::path::Path::new(&d)) {
        fail(format!("ডিরেক্টরি কপি ব্যর্থ '{}' -> '{}': {}", s, d, e));
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

#[no_mangle]
pub extern "C" fn kl_fs_size(path: *mut u8) -> i64 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    match std::fs::metadata(&p) {
        Ok(m) => m.len() as i64,
        Err(e) => {
            fail(format!("আকার পড়া যায়নি '{}': {}", p, e));
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_mtime_ms(path: *mut u8) -> i64 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    match std::fs::metadata(&p).and_then(|m| m.modified()) {
        Ok(t) => t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0),
        Err(e) => {
            fail(format!("পরিবর্তনের সময় পড়া যায়নি '{}': {}", p, e));
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn kl_fs_cwd() -> *mut u8 {
    match std::env::current_dir() {
        Ok(p) => str_from_rust(&p.to_string_lossy()),
        Err(e) => {
            fail(format!("বর্তমান ডিরেক্টরি পড়া যায়নি: {}", e));
            str_from_rust("")
        }
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
// পাথ — লেক্সিক্যাল পাথ ম্যানিপুলেশন, ডিস্কে কিছু ছোঁয় না, তাই ইনফ্যালিবল
// (ব্যর্থ হওয়ার কিছু নেই — খারাপ ইনপুটে ফাঁকা/best-effort ফল দেয়)
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_path_join(a: *mut u8, b: *mut u8) -> *mut u8 {
    let a = String::from_utf8_lossy(unsafe { str_slice(a) }).into_owned();
    let b = String::from_utf8_lossy(unsafe { str_slice(b) }).into_owned();
    str_from_rust(&std::path::Path::new(&a).join(&b).to_string_lossy())
}

#[no_mangle]
pub extern "C" fn kl_path_basename(path: *mut u8) -> *mut u8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let name = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    str_from_rust(&name)
}

#[no_mangle]
pub extern "C" fn kl_path_dirname(path: *mut u8) -> *mut u8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let dir = std::path::Path::new(&p).parent().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    str_from_rust(&dir)
}

#[no_mangle]
pub extern "C" fn kl_path_extension(path: *mut u8) -> *mut u8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let ext = std::path::Path::new(&p).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    str_from_rust(&ext)
}

/// Lexical, not `std::fs::canonicalize` — the latter requires the path to
/// exist and, on Windows, returns a `\\?\`-prefixed extended-length path
/// (hit and worked around this exact footgun elsewhere in this project's
/// tooling this session) — wrong for a general "make this absolute" utility
/// that should work on paths that don't exist yet.
#[no_mangle]
pub extern "C" fn kl_path_abs(path: *mut u8) -> *mut u8 {
    let p = String::from_utf8_lossy(unsafe { str_slice(path) }).into_owned();
    let path = std::path::Path::new(&p);
    let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    str_from_rust(&abs.to_string_lossy())
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
// লেখা (string) — codepoint-indexed, consistent with how the language
// counts everywhere else (`দৈর্ঘ্য`, slicing, etc.).
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

/// Entries are stored densely, in insertion order, up to `map_len(p)` — see
/// `kl_map_find`'s own linear scan. These two accessors expose that ordering
/// to codegen for printing a map's entries; nothing else needs direct index
/// access, since `kl_map_find`/`kl_map_set_slot` look up by key instead.
#[no_mangle]
pub extern "C" fn kl_map_entry_key(p: *mut u8, i: i64) -> i64 {
    unsafe { *(map_entry_ptr(p, i) as *const i64) }
}
#[no_mangle]
pub extern "C" fn kl_map_entry_val_ptr(p: *mut u8, i: i64) -> *mut u8 {
    unsafe { map_entry_ptr(p, i).add(8) }
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

// ============================================================================
// ম্যাট্রিক্স (matrix) — ভেক্টর = দশমিক[] (elem_size 8, raw f64, no drop fn),
// ম্যাট্রিক্স = দশমিক[][] (elem_size 8, each slot a pointer to a নেস্টেড
// দশমিক[], drop fn kl_arr_decref so the rows get released with the outer
// array — same convention as কল_map_keys' array-of-strings, just with
// arrays instead of strings as the owned element).
// ============================================================================

unsafe fn vecf_from_arr(p: *mut u8) -> Vec<f64> {
    let len = kl_arr_len(p);
    (0..len).map(|i| unsafe { *(kl_arr_get_ptr(p, i) as *const f64) }).collect()
}

fn arr_from_vecf(v: &[f64]) -> *mut u8 {
    let arr = kl_arr_new(8, v.len() as i64, 0);
    for x in v {
        kl_arr_push(arr, (x as *const f64) as *const u8);
    }
    arr
}

unsafe fn matf_from_arr(p: *mut u8) -> Vec<Vec<f64>> {
    let len = kl_arr_len(p);
    (0..len)
        .map(|i| {
            let row_ptr = unsafe { *(kl_arr_get_ptr(p, i) as *const *mut u8) };
            unsafe { vecf_from_arr(row_ptr) }
        })
        .collect()
}

fn arr_from_matf(m: &[Vec<f64>]) -> *mut u8 {
    let drop_addr = kl_arr_decref as *const () as usize as i64;
    let arr = kl_arr_new(8, m.len() as i64, drop_addr);
    for row in m {
        let r = arr_from_vecf(row);
        kl_arr_push(arr, (&r as *const *mut u8) as *const u8);
    }
    arr
}

/// Every row's length, or `None` if the rows disagree — `দশমিক[][]` is just
/// nested arrays with no built-in rectangularity guarantee.
fn mat_rect_cols(m: &[Vec<f64>]) -> Option<usize> {
    let cols = m.first().map(|r| r.len()).unwrap_or(0);
    if m.iter().all(|r| r.len() == cols) {
        Some(cols)
    } else {
        None
    }
}

/// Gaussian elimination with partial pivoting — shared by কল_মাত_det/inv.
fn mat_eliminate(mut m: Vec<Vec<f64>>) -> (Vec<Vec<f64>>, f64, u32) {
    let n = m.len();
    let mut det = 1.0;
    let mut swaps = 0u32;
    for col in 0..n {
        let pivot_row = (col..n)
            .max_by(|&a, &b| m[a][col].abs().partial_cmp(&m[b][col].abs()).unwrap())
            .unwrap();
        if pivot_row != col {
            m.swap(col, pivot_row);
            swaps += 1;
        }
        let pivot = m[col][col];
        det *= pivot;
        if pivot.abs() < 1e-12 {
            continue;
        }
        for row in (col + 1)..n {
            let factor = m[row][col] / pivot;
            for c in col..n {
                m[row][c] -= factor * m[col][c];
            }
        }
    }
    (m, det, swaps)
}

fn mat_det_of(m: Vec<Vec<f64>>) -> f64 {
    let (_, det, swaps) = mat_eliminate(m);
    if swaps % 2 == 1 { -det } else { det }
}

/// Gauss-Jordan on the augmented `[m | I]` matrix. `None` if singular.
fn mat_inv_of(m: Vec<Vec<f64>>) -> Option<Vec<Vec<f64>>> {
    let n = m.len();
    let mut aug: Vec<Vec<f64>> = m
        .into_iter()
        .enumerate()
        .map(|(i, mut row)| {
            let mut id = vec![0.0; n];
            id[i] = 1.0;
            row.extend(id);
            row
        })
        .collect();
    for col in 0..n {
        let pivot_row = (col..n)
            .max_by(|&a, &b| aug[a][col].abs().partial_cmp(&aug[b][col].abs()).unwrap())
            .unwrap();
        aug.swap(col, pivot_row);
        let pivot = aug[col][col];
        if pivot.abs() < 1e-12 {
            return None;
        }
        for c in 0..(2 * n) {
            aug[col][c] /= pivot;
        }
        for row in 0..n {
            if row == col {
                continue;
            }
            let factor = aug[row][col];
            for c in 0..(2 * n) {
                aug[row][c] -= factor * aug[col][c];
            }
        }
    }
    Some(aug.into_iter().map(|row| row[n..].to_vec()).collect())
}

#[no_mangle]
pub extern "C" fn kl_mat_vec_add(a: *mut u8, b: *mut u8) -> *mut u8 {
    let (a, b) = unsafe { (vecf_from_arr(a), vecf_from_arr(b)) };
    if a.len() != b.len() {
        fail("ভেক্টর_যোগ: দুই ভেক্টরের দৈর্ঘ্য সমান হতে হবে".into());
        return arr_from_vecf(&[]);
    }
    arr_from_vecf(&a.iter().zip(&b).map(|(x, y)| x + y).collect::<Vec<_>>())
}

#[no_mangle]
pub extern "C" fn kl_mat_vec_sub(a: *mut u8, b: *mut u8) -> *mut u8 {
    let (a, b) = unsafe { (vecf_from_arr(a), vecf_from_arr(b)) };
    if a.len() != b.len() {
        fail("ভেক্টর_বিয়োগ: দুই ভেক্টরের দৈর্ঘ্য সমান হতে হবে".into());
        return arr_from_vecf(&[]);
    }
    arr_from_vecf(&a.iter().zip(&b).map(|(x, y)| x - y).collect::<Vec<_>>())
}

#[no_mangle]
pub extern "C" fn kl_mat_vec_scale(v: *mut u8, k: f64) -> *mut u8 {
    let v = unsafe { vecf_from_arr(v) };
    arr_from_vecf(&v.iter().map(|x| x * k).collect::<Vec<_>>())
}

#[no_mangle]
pub extern "C" fn kl_mat_dot(a: *mut u8, b: *mut u8) -> f64 {
    let (a, b) = unsafe { (vecf_from_arr(a), vecf_from_arr(b)) };
    if a.len() != b.len() {
        fail("ডট: দুই ভেক্টরের দৈর্ঘ্য সমান হতে হবে".into());
        return 0.0;
    }
    a.iter().zip(&b).map(|(x, y)| x * y).sum()
}

#[no_mangle]
pub extern "C" fn kl_mat_cross(a: *mut u8, b: *mut u8) -> *mut u8 {
    let (a, b) = unsafe { (vecf_from_arr(a), vecf_from_arr(b)) };
    if a.len() != 3 || b.len() != 3 {
        fail("ক্রস: শুধু ৩-মাত্রিক ভেক্টরের জন্য সংজ্ঞায়িত".into());
        return arr_from_vecf(&[0.0, 0.0, 0.0]);
    }
    arr_from_vecf(&[
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ])
}

#[no_mangle]
pub extern "C" fn kl_mat_norm(v: *mut u8) -> f64 {
    let v = unsafe { vecf_from_arr(v) };
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

#[no_mangle]
pub extern "C" fn kl_mat_add(a: *mut u8, b: *mut u8) -> *mut u8 {
    let (a, b) = unsafe { (matf_from_arr(a), matf_from_arr(b)) };
    let (ca, cb) = (mat_rect_cols(&a), mat_rect_cols(&b));
    if ca.is_none() || cb.is_none() || a.len() != b.len() || ca != cb {
        fail("যোগ: দুই ম্যাট্রিক্সের মাত্রা সমান ও আয়তাকার হতে হবে".into());
        return arr_from_matf(&[]);
    }
    arr_from_matf(&a.iter().zip(&b).map(|(ra, rb)| ra.iter().zip(rb).map(|(x, y)| x + y).collect()).collect::<Vec<_>>())
}

#[no_mangle]
pub extern "C" fn kl_mat_sub(a: *mut u8, b: *mut u8) -> *mut u8 {
    let (a, b) = unsafe { (matf_from_arr(a), matf_from_arr(b)) };
    let (ca, cb) = (mat_rect_cols(&a), mat_rect_cols(&b));
    if ca.is_none() || cb.is_none() || a.len() != b.len() || ca != cb {
        fail("বিয়োগ: দুই ম্যাট্রিক্সের মাত্রা সমান ও আয়তাকার হতে হবে".into());
        return arr_from_matf(&[]);
    }
    arr_from_matf(&a.iter().zip(&b).map(|(ra, rb)| ra.iter().zip(rb).map(|(x, y)| x - y).collect()).collect::<Vec<_>>())
}

#[no_mangle]
pub extern "C" fn kl_mat_scale(m: *mut u8, k: f64) -> *mut u8 {
    let m = unsafe { matf_from_arr(m) };
    arr_from_matf(&m.into_iter().map(|row| row.into_iter().map(|x| x * k).collect()).collect::<Vec<_>>())
}

#[no_mangle]
pub extern "C" fn kl_mat_mul(a: *mut u8, b: *mut u8) -> *mut u8 {
    let (a, b) = unsafe { (matf_from_arr(a), matf_from_arr(b)) };
    let (Some(ca), Some(cb)) = (mat_rect_cols(&a), mat_rect_cols(&b)) else {
        fail("গুণ: ম্যাট্রিক্সের প্রতিটি সারি একই দৈর্ঘ্যের হতে হবে".into());
        return arr_from_matf(&[]);
    };
    let (ra, rb) = (a.len(), b.len());
    if ca != rb {
        fail(format!("গুণ: প্রথম ম্যাট্রিক্সের কলাম ({}) দ্বিতীয়টির সারির ({}) সমান হতে হবে", ca, rb));
        return arr_from_matf(&[]);
    }
    let mut out = vec![vec![0.0; cb]; ra];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..ca).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    arr_from_matf(&out)
}

#[no_mangle]
pub extern "C" fn kl_mat_transpose(m: *mut u8) -> *mut u8 {
    let m = unsafe { matf_from_arr(m) };
    let Some(cols) = mat_rect_cols(&m) else {
        fail("ট্রান্সপোজ: ম্যাট্রিক্সের প্রতিটি সারি একই দৈর্ঘ্যের হতে হবে".into());
        return arr_from_matf(&[]);
    };
    let rows = m.len();
    let mut out = vec![vec![0.0; rows]; cols];
    for (i, row) in m.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            out[j][i] = *v;
        }
    }
    arr_from_matf(&out)
}

#[no_mangle]
pub extern "C" fn kl_mat_det(m: *mut u8) -> f64 {
    let m = unsafe { matf_from_arr(m) };
    match mat_rect_cols(&m) {
        Some(cols) if cols == m.len() => mat_det_of(m),
        _ => {
            fail("নির্ণায়ক: শুধু বর্গ ম্যাট্রিক্সের জন্য সংজ্ঞায়িত".into());
            0.0
        }
    }
}

#[no_mangle]
pub extern "C" fn kl_mat_inv(m: *mut u8) -> *mut u8 {
    let m = unsafe { matf_from_arr(m) };
    match mat_rect_cols(&m) {
        Some(cols) if cols == m.len() => match mat_inv_of(m) {
            Some(inv) => arr_from_matf(&inv),
            None => {
                fail("বিপরীত: ম্যাট্রিক্সটি ইনভার্টিবল নয় (নির্ণায়ক শূন্য)".into());
                arr_from_matf(&[])
            }
        },
        _ => {
            fail("বিপরীত: শুধু বর্গ ম্যাট্রিক্সের জন্য সংজ্ঞায়িত".into());
            arr_from_matf(&[])
        }
    }
}

#[no_mangle]
pub extern "C" fn kl_mat_identity(n: i64) -> *mut u8 {
    if n < 1 {
        fail("অভেদক: আকার কমপক্ষে ১ হতে হবে".into());
        return arr_from_matf(&[]);
    }
    let n = n as usize;
    arr_from_matf(&(0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect::<Vec<_>>())
}

#[no_mangle]
pub extern "C" fn kl_mat_zeros(rows: i64, cols: i64) -> *mut u8 {
    if rows < 1 || cols < 1 {
        fail("শূন্য_ম্যাট্রিক্স: সারি ও কলাম কমপক্ষে ১ হতে হবে".into());
        return arr_from_matf(&[]);
    }
    arr_from_matf(&vec![vec![0.0; cols as usize]; rows as usize])
}

// ============================================================================
// জ্যামিতি (geometry) — বিন্দু = `[x, y]` (দশমিক[]), বহুভুজ = বিন্দুর তালিকা
// (দশমিক[][]) — ম্যাট্রিক্স-এর ভেক্টর/ম্যাট্রিক্স রিপ্রেজেন্টেশন ও
// vecf_from_arr/arr_from_vecf/matf_from_arr হেল্পার পুনর্ব্যবহার করে।
// ============================================================================

const PI: f64 = std::f64::consts::PI;

#[no_mangle]
pub extern "C" fn kl_geo_distance(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt()
}

#[no_mangle]
pub extern "C" fn kl_geo_angle(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    (y2 - y1).atan2(x2 - x1)
}

#[no_mangle]
pub extern "C" fn kl_geo_rotate(x: f64, y: f64, cx: f64, cy: f64, angle: f64) -> *mut u8 {
    let (dx, dy) = (x - cx, y - cy);
    let (sin, cos) = angle.sin_cos();
    arr_from_vecf(&[cx + dx * cos - dy * sin, cy + dx * sin + dy * cos])
}

#[no_mangle]
pub extern "C" fn kl_geo_circle_area(r: f64) -> f64 {
    PI * r * r
}

#[no_mangle]
pub extern "C" fn kl_geo_circle_circumference(r: f64) -> f64 {
    2.0 * PI * r
}

#[no_mangle]
pub extern "C" fn kl_geo_ellipse_area(rx: f64, ry: f64) -> f64 {
    PI * rx * ry
}

/// Ramanujan's first approximation — an ellipse's circumference has no
/// elementary closed form (it needs an elliptic integral), but this stays
/// within practical accuracy across every rx/ry ratio.
#[no_mangle]
pub extern "C" fn kl_geo_ellipse_circumference(rx: f64, ry: f64) -> f64 {
    let h = ((rx - ry) / (rx + ry)).powi(2);
    PI * (rx + ry) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()))
}

#[no_mangle]
pub extern "C" fn kl_geo_triangle_area(x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> f64 {
    (x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2)).abs() / 2.0
}

/// Shoelace formula. `points` must be at least 3 rows of exactly 2 columns —
/// checked here since a `দশমিক[][]` value carries no shape guarantee.
#[no_mangle]
pub extern "C" fn kl_geo_polygon_area(points: *mut u8) -> f64 {
    let pts = unsafe { matf_from_arr(points) };
    if pts.len() < 3 || pts.iter().any(|p| p.len() != 2) {
        fail("বহুভুজের_ক্ষেত্রফল: কমপক্ষে ৩টি [x, y] বিন্দু দরকার".into());
        return 0.0;
    }
    let n = pts.len();
    let sum: f64 = (0..n)
        .map(|i| {
            let (x1, y1) = (pts[i][0], pts[i][1]);
            let (x2, y2) = (pts[(i + 1) % n][0], pts[(i + 1) % n][1]);
            x1 * y2 - x2 * y1
        })
        .sum();
    sum.abs() / 2.0
}

#[no_mangle]
pub extern "C" fn kl_geo_sphere_volume(r: f64) -> f64 {
    4.0 / 3.0 * PI * r.powi(3)
}

#[no_mangle]
pub extern "C" fn kl_geo_sphere_surface(r: f64) -> f64 {
    4.0 * PI * r * r
}

#[no_mangle]
pub extern "C" fn kl_geo_cone_volume(r: f64, h: f64) -> f64 {
    PI * r * r * h / 3.0
}

/// Total surface area (base + lateral).
#[no_mangle]
pub extern "C" fn kl_geo_cone_surface(r: f64, h: f64) -> f64 {
    PI * r * (r + (r * r + h * h).sqrt())
}

#[no_mangle]
pub extern "C" fn kl_geo_cylinder_volume(r: f64, h: f64) -> f64 {
    PI * r * r * h
}

#[no_mangle]
pub extern "C" fn kl_geo_cylinder_surface(r: f64, h: f64) -> f64 {
    2.0 * PI * r * (r + h)
}

/// `n` evenly spaced points on the ellipse centered at `(cx, cy)` with radii
/// `(rx, ry)` — shared body behind নিয়মিত_বহুভুজ (rx == ry) and উপবৃত্ত_বিন্দু.
fn ellipse_points(cx: f64, cy: f64, rx: f64, ry: f64, n: i64) -> Vec<Vec<f64>> {
    (0..n)
        .map(|i| {
            let angle = 2.0 * PI * (i as f64) / (n as f64);
            vec![cx + rx * angle.cos(), cy + ry * angle.sin()]
        })
        .collect()
}

#[no_mangle]
pub extern "C" fn kl_geo_regular_polygon(cx: f64, cy: f64, r: f64, n: i64) -> *mut u8 {
    if n < 3 {
        fail("নিয়মিত_বহুভুজ: কমপক্ষে ৩ বাহু দরকার".into());
        return arr_from_matf(&[]);
    }
    arr_from_matf(&ellipse_points(cx, cy, r, r, n))
}

#[no_mangle]
pub extern "C" fn kl_geo_ellipse_points(cx: f64, cy: f64, rx: f64, ry: f64, n: i64) -> *mut u8 {
    if n < 3 {
        fail("উপবৃত্ত_বিন্দু: কমপক্ষে ৩টি বিন্দু দরকার".into());
        return arr_from_matf(&[]);
    }
    arr_from_matf(&ellipse_points(cx, cy, rx, ry, n))
}

/// Intersection of the two *infinite* lines through `(x1,y1)-(x2,y2)` and
/// `(x3,y3)-(x4,y4)` — not clipped to either segment.
#[no_mangle]
pub extern "C" fn kl_geo_line_intersect(x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64, x4: f64, y4: f64) -> *mut u8 {
    let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if denom.abs() < 1e-12 {
        fail("রেখার_ছেদ: রেখা দুটি সমান্তরাল, কোনো ছেদবিন্দু নেই".into());
        return arr_from_vecf(&[0.0, 0.0]);
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
    arr_from_vecf(&[x1 + t * (x2 - x1), y1 + t * (y2 - y1)])
}

// ============================================================================
// পরিসংখ্যান (statistics) — সব দশমিক[] উপর কাজ করে, ম্যাট্রিক্সের vecf_from_arr/
// arr_from_vecf পুনর্ব্যবহার করে। ভেদাংক/আদর্শ_বিচ্যুতি/সহভেদাংক population
// সংস্করণ (n দিয়ে ভাগ) — পুরো ডেটাসেট হাতে আছে ধরে, sample-থেকে-অনুমান নয়।
// ============================================================================

fn stat_mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn stat_median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

fn stat_mode(v: &[f64]) -> f64 {
    let mut best = v[0];
    let mut best_count = 0usize;
    for &x in v {
        let count = v.iter().filter(|&&y| y == x).count();
        if count > best_count {
            best_count = count;
            best = x;
        }
    }
    best
}

fn stat_variance(v: &[f64]) -> f64 {
    let m = stat_mean(v);
    v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64
}

fn stat_covariance(a: &[f64], b: &[f64]) -> f64 {
    let (ma, mb) = (stat_mean(a), stat_mean(b));
    a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum::<f64>() / a.len() as f64
}

#[no_mangle]
pub extern "C" fn kl_stat_sum(v: *mut u8) -> f64 {
    let v = unsafe { vecf_from_arr(v) };
    if v.is_empty() {
        fail("সমষ্টি: খালি ভেক্টরে সংজ্ঞায়িত নয়".into());
        return 0.0;
    }
    v.iter().sum()
}

#[no_mangle]
pub extern "C" fn kl_stat_mean(v: *mut u8) -> f64 {
    let v = unsafe { vecf_from_arr(v) };
    if v.is_empty() {
        fail("গড়: খালি ভেক্টরে সংজ্ঞায়িত নয়".into());
        return 0.0;
    }
    stat_mean(&v)
}

#[no_mangle]
pub extern "C" fn kl_stat_median(v: *mut u8) -> f64 {
    let v = unsafe { vecf_from_arr(v) };
    if v.is_empty() {
        fail("মধ্যক: খালি ভেক্টরে সংজ্ঞায়িত নয়".into());
        return 0.0;
    }
    stat_median(&v)
}

#[no_mangle]
pub extern "C" fn kl_stat_mode(v: *mut u8) -> f64 {
    let v = unsafe { vecf_from_arr(v) };
    if v.is_empty() {
        fail("প্রচুরক: খালি ভেক্টরে সংজ্ঞায়িত নয়".into());
        return 0.0;
    }
    stat_mode(&v)
}

#[no_mangle]
pub extern "C" fn kl_stat_variance(v: *mut u8) -> f64 {
    let v = unsafe { vecf_from_arr(v) };
    if v.is_empty() {
        fail("ভেদাংক: খালি ভেক্টরে সংজ্ঞায়িত নয়".into());
        return 0.0;
    }
    stat_variance(&v)
}

#[no_mangle]
pub extern "C" fn kl_stat_stddev(v: *mut u8) -> f64 {
    let v = unsafe { vecf_from_arr(v) };
    if v.is_empty() {
        fail("আদর্শ_বিচ্যুতি: খালি ভেক্টরে সংজ্ঞায়িত নয়".into());
        return 0.0;
    }
    stat_variance(&v).sqrt()
}

#[no_mangle]
pub extern "C" fn kl_stat_covariance(a: *mut u8, b: *mut u8) -> f64 {
    let (a, b) = unsafe { (vecf_from_arr(a), vecf_from_arr(b)) };
    if a.len() != b.len() || a.len() < 2 {
        fail("সহভেদাংক: দুই ভেক্টরের দৈর্ঘ্য সমান ও কমপক্ষে ২ হতে হবে".into());
        return 0.0;
    }
    stat_covariance(&a, &b)
}

#[no_mangle]
pub extern "C" fn kl_stat_correlation(a: *mut u8, b: *mut u8) -> f64 {
    let (a, b) = unsafe { (vecf_from_arr(a), vecf_from_arr(b)) };
    if a.len() != b.len() || a.len() < 2 {
        fail("সহসম্পর্ক: দুই ভেক্টরের দৈর্ঘ্য সমান ও কমপক্ষে ২ হতে হবে".into());
        return 0.0;
    }
    let (sa, sb) = (stat_variance(&a).sqrt(), stat_variance(&b).sqrt());
    if sa == 0.0 || sb == 0.0 {
        fail("সহসম্পর্ক: একটি ভেক্টরের আদর্শ-বিচ্যুতি শূন্য (সব মান সমান)".into());
        return 0.0;
    }
    stat_covariance(&a, &b) / (sa * sb)
}

#[no_mangle]
pub extern "C" fn kl_stat_linreg(x: *mut u8, y: *mut u8) -> *mut u8 {
    let (x, y) = unsafe { (vecf_from_arr(x), vecf_from_arr(y)) };
    if x.len() != y.len() || x.len() < 2 {
        fail("রৈখিক_রিগ্রেশন: দুই ভেক্টরের দৈর্ঘ্য সমান ও কমপক্ষে ২ হতে হবে".into());
        return arr_from_vecf(&[0.0, 0.0]);
    }
    let (mx, my) = (stat_mean(&x), stat_mean(&y));
    let cov: f64 = x.iter().zip(&y).map(|(xi, yi)| (xi - mx) * (yi - my)).sum();
    let varx: f64 = x.iter().map(|xi| (xi - mx).powi(2)).sum();
    if varx == 0.0 {
        fail("রৈখিক_রিগ্রেশন: x-এর সব মান সমান, ঢাল অসংজ্ঞায়িত".into());
        return arr_from_vecf(&[0.0, 0.0]);
    }
    let slope = cov / varx;
    let intercept = my - slope * mx;
    arr_from_vecf(&[slope, intercept])
}

// ============================================================================
// সাজাও (global builtin, not a stdlib module member — same "always
// available" tier as `দৈর্ঘ্য`/`কপি`) — non-mutating, returns a fresh sorted
// array; the element type (সংখ্যা/দশমিক/লেখা) is fixed by sema at compile
// time, so lowering picks one of these three directly rather than needing a
// single polymorphic runtime entry point.
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_sort_num(p: *mut u8) -> *mut u8 {
    let len = kl_arr_len(p);
    let mut v: Vec<i64> = (0..len).map(|i| unsafe { *(kl_arr_get_ptr(p, i) as *const i64) }).collect();
    v.sort();
    let out = kl_arr_new(8, len, 0);
    for x in &v {
        kl_arr_push(out, (x as *const i64) as *const u8);
    }
    out
}

#[no_mangle]
pub extern "C" fn kl_sort_dec(p: *mut u8) -> *mut u8 {
    let len = kl_arr_len(p);
    let mut v: Vec<f64> = (0..len).map(|i| unsafe { *(kl_arr_get_ptr(p, i) as *const f64) }).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let out = kl_arr_new(8, len, 0);
    for x in &v {
        kl_arr_push(out, (x as *const f64) as *const u8);
    }
    out
}

#[no_mangle]
pub extern "C" fn kl_sort_txt(p: *mut u8) -> *mut u8 {
    let len = kl_arr_len(p);
    let mut items: Vec<(String, *mut u8)> = (0..len)
        .map(|i| {
            let ptr = unsafe { *(kl_arr_get_ptr(p, i) as *const *mut u8) };
            let s = String::from_utf8_lossy(unsafe { str_slice(ptr) }).into_owned();
            (s, ptr)
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let drop_addr = kl_str_decref as *const () as usize as i64;
    let out = kl_arr_new(8, len, drop_addr);
    for (_, ptr) in &items {
        // নতুন অ্যারে তার নিজস্ব রেফারেন্স ধরে রাখে — মূল অ্যারে অক্ষত থাকে
        // (`সাজাও` non-mutating), দুটোই একই স্ট্রিং শেয়ার করে refcount-এর মাধ্যমে।
        kl_rc_incref(*ptr);
        kl_arr_push(out, (ptr as *const *mut u8) as *const u8);
    }
    out
}

// ============================================================================
// সিস্টেম (system) — কমান্ড-লাইন আর্গুমেন্ট ও এনভায়রনমেন্ট ভ্যারিয়েবল। এই
// runtime যে প্রসেসে লিংক হয়েছে তারই `std::env::args()`/`var()` পড়ে —
// নেটিভ কম্পাইল করা .exe নিজের OS-দেওয়া argv সরাসরি পায়, কোনো বিশেষ
// পাস-থ্রু `কলম বিল্ড`-এর কাছ থেকে লাগে না (ইন্টারপ্রেটেড মোডে
// `kolom_interp::run_with_argv` একইভাবে `কলম চালাও file.ক ...`-এর অতিরিক্ত
// আর্গুমেন্ট আলাদাভাবে থ্রেড করে, যেহেতু সেখানে `std::env::args()` কলম
// নিজের argv (`kolom চালাও file.ক ...`) দেখাত, স্ক্রিপ্টের নয়)।
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_sys_args() -> *mut u8 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let drop_addr = kl_str_decref as *const () as usize as i64;
    let arr = kl_arr_new(8, args.len() as i64, drop_addr);
    for a in &args {
        let s = str_from_rust(a);
        kl_arr_push(arr, (&s as *const *mut u8) as *const u8);
    }
    arr
}

#[no_mangle]
pub extern "C" fn kl_sys_env(name: *mut u8) -> *mut u8 {
    let name = String::from_utf8_lossy(unsafe { str_slice(name) }).into_owned();
    str_from_rust(&std::env::var(&name).unwrap_or_default())
}

// ============================================================================
// সময় (time) — এই মডিউলটা সেমা/ইন্টারপ্রেটারে সবসময়ই কাজ করত, কিন্তু কখনো
// Cranelift-এ ওয়্যার করা হয়নি (`kl_time_now_ms`/`kl_time_clock` নামগুলো এখনো
// legacy C ব্যাকএন্ডের নিজস্ব ইনলাইন C কোড থেকে — ওটাই এই নামের একমাত্র
// আগের বাস্তবায়ন ছিল, রিমুভড, কিন্তু নাম দুটো ধরে রাখা হলো ধারাবাহিকতার
// জন্য) — `কলম বিল্ড`-এ `সময়.এখন_মিলিসেকেন্ড()` কল করলে "সমর্থিত নয়" ত্রুটি
// দিত। এখানে বাকি সব ক্যালেন্ডার ফাংশনের সাথে একবারে ফিক্স করা হলো।
// ============================================================================

#[no_mangle]
pub extern "C" fn kl_time_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn kl_time_clock() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Howard Hinnant's `civil_from_days` — days-since-epoch to (year, month,
/// day) in the proleptic Gregorian calendar. Public-domain algorithm, no
/// date crate needed.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn now_civil() -> (i64, u32, u32) {
    let days = kl_time_now_ms().div_euclid(1000).div_euclid(86400);
    civil_from_days(days)
}

fn now_time_of_day() -> (u32, u32, u32) {
    let secs = kl_time_now_ms().div_euclid(1000).rem_euclid(86400) as u32;
    (secs / 3600, (secs % 3600) / 60, secs % 60)
}

#[no_mangle]
pub extern "C" fn kl_time_year() -> i64 {
    now_civil().0
}
#[no_mangle]
pub extern "C" fn kl_time_month() -> i64 {
    now_civil().1 as i64
}
#[no_mangle]
pub extern "C" fn kl_time_day() -> i64 {
    now_civil().2 as i64
}
#[no_mangle]
pub extern "C" fn kl_time_hour() -> i64 {
    now_time_of_day().0 as i64
}
#[no_mangle]
pub extern "C" fn kl_time_minute() -> i64 {
    now_time_of_day().1 as i64
}
#[no_mangle]
pub extern "C" fn kl_time_second_part() -> i64 {
    now_time_of_day().2 as i64
}
#[no_mangle]
pub extern "C" fn kl_time_now_str() -> *mut u8 {
    let (y, mo, d) = now_civil();
    let (h, mi, s) = now_time_of_day();
    str_from_rust(&format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"))
}
