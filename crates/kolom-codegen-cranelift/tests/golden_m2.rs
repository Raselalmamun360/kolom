//! Milestone-2 acceptance test: containers + ownership — arrays,
//! `প্রতি`/foreach, `শেয়ার` shared refs, structs, `কপি`. Checked against
//! the interpreter's reference `expected.txt` files.

mod harness;
use harness::assert_matches_reference;

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
