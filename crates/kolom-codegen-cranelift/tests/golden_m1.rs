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

/// `চলবে` and `বিরতি` across all three loop forms. Continue had no fixture at
/// all until the keyword was renamed from `চলো`, so this covers both the new
/// spelling and a control-flow path that was previously untested end to end.
#[test]
fn golden_36_continue() {
    assert_matches_reference("36_continue");
}

/// সংখ্যা/দশমিক mixed arithmetic (promotion) and `%` on দশমিক — neither had
/// a Cranelift lowering until a v1.0 audit found both: sema accepts mixed
/// arithmetic (`language.md` §৬ documents it explicitly, `%` included) but
/// codegen only had matching-type arms, and had no float-remainder path at
/// all since Cranelift has no such instruction.
#[test]
fn golden_38_mixed_arithmetic() {
    assert_matches_reference("38_mixed_arithmetic");
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
