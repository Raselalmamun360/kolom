//! Milestone-2 acceptance test: containers + ownership — arrays,
//! `প্রতি`/foreach, `শেয়ার` shared refs, structs, `কপি`. Checked against
//! the interpreter's reference `expected.txt` files.

mod harness;
use harness::{assert_matches_reference, assert_matches_reference_module};

#[test]
fn golden_07_arrays() {
    assert_matches_reference("07_arrays");
}

#[test]
fn golden_19_ok_for_each() {
    assert_matches_reference("19_ok_for_each");
}

#[test]
fn golden_20_ok_joth() {
    assert_matches_reference("20_ok_joth");
}

#[test]
fn golden_30_struct_type() {
    assert_matches_reference("30_struct_type");
}

#[test]
fn golden_13_ok_copy() {
    assert_matches_reference("13_ok_copy");
}

#[test]
fn golden_08_const_float() {
    assert_matches_reference("08_const_float");
}

#[test]
fn golden_32_nested_struct() {
    assert_matches_reference("32_nested_struct");
}

#[test]
fn golden_33_struct_fn() {
    assert_matches_reference("33_struct_fn");
}

/// The one fixture here that imports a sibling `.ক` module, so it goes
/// through `assert_matches_reference_module` rather than the plain
/// single-file path the rest of this file uses.
#[test]
fn golden_34_module_struct() {
    assert_matches_reference_module("34_module_struct");
}
