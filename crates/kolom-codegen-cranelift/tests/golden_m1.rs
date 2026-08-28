//! Milestone-1 acceptance test: core language (variables, functions,
//! control flow), compiled natively and checked against the SAME
//! `expected.txt` the interpreter is verified against — see
//! `harness::assert_matches_reference`.

mod harness;
use harness::assert_matches_reference;

#[test]
fn golden_02_variables() {
    assert_matches_reference("02_variables");
}

#[test]
fn golden_03_functions() {
    assert_matches_reference("03_functions");
}

#[test]
fn golden_04_control_flow() {
    assert_matches_reference("04_control_flow");
}

#[test]
fn golden_01_hello() {
    assert_matches_reference("01_hello");
}

#[test]
fn golden_05_strings() {
    assert_matches_reference("05_strings");
}

#[test]
fn golden_06_escapes() {
    assert_matches_reference("06_escapes");
}
