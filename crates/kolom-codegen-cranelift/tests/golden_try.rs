//! `চেষ্টা` / `ধরো` on the native backend, checked against the interpreter's
//! reference `expected.txt` files.
//!
//! Until this landed, `চেষ্টা` was the one statement the Cranelift backend
//! could not lower at all: `কলম চালাও` ran these programs and `কলম বিল্ড`
//! refused them. The interesting cases are less about catching an error than
//! about the ways control leaves a `চেষ্টা` block — a `ফেরাও` from inside
//! one, a `বিরতি` out of one and into an enclosing loop — because each has to
//! tell the runtime the block is closing.

mod harness;
use harness::{assert_matches_reference_with, build};
use std::process::Command;

/// The original fixture: a failed file read and a division by zero, both
/// caught, with ordinary code continuing afterwards.
///
/// Runs in a cleared directory so `নেই_এমন_ফাইল.txt` is reliably absent —
/// the assertion is on the failure message, so a stray file of that name in
/// the repo would quietly turn this into a pass for the wrong reason.
#[test]
fn golden_31_try_catch() {
    assert_matches_reference_with("31_try_catch", &[], true);
}

/// Nested blocks, `ফেরাও` from inside one, and `বিরতি` leaving one to break
/// an enclosing loop.
#[test]
fn golden_35_try_nested() {
    assert_matches_reference_with("35_try_nested", &[], true);
}

/// The other half of the contract: with no `চেষ্টা` open, a failure still
/// ends the process with its Bengali message, exactly as it did before this
/// was catchable.
///
/// Worth asserting explicitly, because the mechanism that makes an error
/// catchable is the same one that could have made it silently ignorable —
/// and because integer division used to reach `sdiv` unguarded, so this case
/// died by hardware trap with no message at all.
#[test]
fn uncaught_error_still_aborts() {
    let src = "অ্যাপ {\n    ধরি ক = ১০ / ০\n    লেখো(লেখায়(ক))\n}\n";
    let built = build("uncaught_div_zero", src);
    let out = Command::new(&built.exe).output().expect("failed to run produced exe");

    assert!(!out.status.success(), "expected a nonzero exit, got {:?}", out.status.code());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("শূন্য দিয়ে ভাগ করা যাবে না"),
        "expected the division-by-zero message on stderr, got: {stderr:?}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).is_empty(),
        "nothing should have been printed before the failure"
    );
}

/// A `চেষ্টা` block that completes normally must leave the runtime's depth
/// counter where it found it. If it did not, this second failure — raised
/// with no block open — would be recorded instead of ending the process, and
/// the program would print a value it should never have reached.
#[test]
fn depth_is_restored_after_a_block_succeeds() {
    let src = "অ্যাপ {\n    চেষ্টা {\n        লেখো(\"ঠিক আছে\")\n    } ধরো(ত) {\n        লেখো(\"ধরা\")\n    }\n    ধরি ক = ১ / ০\n    লেখো(লেখায়(ক))\n}\n";
    let built = build("depth_restored", src);
    let out = Command::new(&built.exe).output().expect("failed to run produced exe");

    assert!(!out.status.success(), "expected a nonzero exit, got {:?}", out.status.code());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        "ঠিক আছে\n",
        "the block's own output should appear, and nothing after the failure"
    );
}
