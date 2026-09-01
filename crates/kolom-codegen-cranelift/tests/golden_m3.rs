//! Milestone-3 acceptance test: standard library (গণিত/লেখা/ফাইল/র‍্যান্ডম/
//! ফাইলসিস্টেম/জেসন/ম্যাট্রিক্স/জ্যামিতি/পরিসংখ্যান/সময়) + real `ম্যাপ[K,V]`,
//! checked against the interpreter's reference `expected.txt` files.
//!
//! নেটওয়ার্ক has no fixture and গ্রাফিক্স is covered by golden_m4 (it needs
//! the UI engine), so neither appears here.

mod harness;
use harness::{assert_matches_reference, assert_matches_reference_module, assert_matches_reference_with};

#[test]
fn golden_21_stdlib_math() {
    assert_matches_reference("21_stdlib_math");
}

#[test]
fn golden_49_time_deterministic() {
    assert_matches_reference("49_time_deterministic");
}

#[test]
fn golden_22_stdlib_string() {
    assert_matches_reference("22_stdlib_string");
}

#[test]
fn golden_26_stdlib_json() {
    assert_matches_reference("26_stdlib_json");
}

#[test]
fn golden_29_map_type() {
    assert_matches_reference("29_map_type");
}

/// A `: Type[]` annotated empty `[]`, and a bare stdlib constant reference
/// (`গণিত.পাই`/`ই`, not called) — both had no Cranelift lowering at all
/// until this fixture was added; the empty array fell through to a "not
/// supported" error even with its required annotation present, and a
/// comment beside the bare-qualified-expression code literally said this
/// case "isn't wired up yet".
#[test]
fn golden_37_empty_array_and_const() {
    assert_matches_reference("37_empty_array_and_const");
}

// These create files, so they run in a freshly-cleared working directory
// rather than wherever the test binary happens to start.
#[test]
fn golden_23_stdlib_io() {
    assert_matches_reference_with("23_stdlib_io", &[], true);
}

#[test]
fn golden_25_stdlib_fs() {
    assert_matches_reference_with("25_stdlib_fs", &[], true);
}

#[test]
fn golden_41_stdlib_fs2() {
    assert_matches_reference_with("41_stdlib_fs2", &[], true);
}

#[test]
fn golden_42_stdlib_path() {
    assert_matches_reference("42_stdlib_path");
}

#[test]
fn golden_43_stdlib_matrix() {
    assert_matches_reference("43_stdlib_matrix");
}

#[test]
fn golden_44_stdlib_geometry() {
    assert_matches_reference("44_stdlib_geometry");
}

#[test]
fn golden_45_stdlib_statistics() {
    assert_matches_reference("45_stdlib_statistics");
}

#[test]
fn golden_46_builtin_sort() {
    assert_matches_reference("46_builtin_sort");
}

#[test]
fn golden_47_builtin_parse() {
    assert_matches_reference("47_builtin_parse");
}

/// র‍্যান্ডম used to be the one fixture that couldn't use the shared
/// reference — the interpreter and this backend ran different PRNGs. Both
/// now share the same xorshift64 implementation and seed mixing (see
/// `kolom-runtime`'s `xorshift_next`/`kolom-interp`'s `Interp::rng_next`),
/// so a `বীজ`-seeded sequence is identical either way and this fixture
/// diffs its whole output against the interpreter's `expected.txt` like
/// every other one.
#[test]
fn golden_24_stdlib_random() {
    assert_matches_reference("24_stdlib_random");
}

/// জেসন_মডিউল — a full recursive-descent JSON parser + serializer written
/// entirely as ordinary Kolom source (see the doc comment at the top of
/// জেসন_মডিউল.ক for why: a runtime-level implementation can't construct
/// instances of a user-declared struct, since that struct's field layout is
/// only known to the compiler pass compiling *this* program, never to
/// kolom-runtime, which is built once ahead of any user's source existing).
#[test]
fn golden_50_json_dom() {
    assert_matches_reference_module("50_json_dom");
}
