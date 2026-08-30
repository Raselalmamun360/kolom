//! Milestone-2 acceptance test: containers + ownership — arrays,
//! `প্রতি`/foreach, `শেয়ার` shared refs, structs, `কপি`. Checked against
//! the interpreter's reference `expected.txt` files.

mod harness;
use harness::{assert_matches_reference, assert_matches_reference_module, run};

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

/// struct-typed array elements and a struct-typed `শেয়ার` cell — `drop_addr_for`
/// had no `Ty::Struct` case at all until this fixture was added, so
/// `[পণ্য("চাল", ৬৫), ...]` and `শেয়ার বিন্দু` both errored outright.
#[test]
fn golden_39_struct_containers() {
    assert_matches_reference("39_struct_containers");
}

/// Arrays, an empty array, and single-field/single-entry struct and map
/// printing — `লেখো(x)` for any of these had no Cranelift lowering at all
/// until this fixture was added.
#[test]
fn golden_40_print_containers() {
    assert_matches_reference("40_print_containers");
}

/// A struct with multiple fields and a map with multiple entries, printed
/// directly. Not part of `40_print_containers`'s strict `expected.txt`
/// comparison, because the interpreter represents both as a
/// `HashMap<String, Value>` — its own print order is not stable run to run
/// (verified directly: two interpreter runs of the same program produced
/// `{"বয়স": 25, "নাম": রহিম}` and `{"নাম": রহিম, "বয়স": 25}`). This checks
/// content instead of exact order: every field/entry appears, correctly
/// formatted, regardless of which order either backend picks.
#[test]
fn print_multi_field_containers() {
    let src = r#"
ডাটা ব্যক্তি { নাম: লেখা, বয়স: সংখ্যা }

অ্যাপ {
    ধরি প = ব্যক্তি("রহিম", ২৫)
    লেখো(প)

    ধরি ম: ম্যাপ[লেখা, সংখ্যা] = ম্যাপ_তৈরি()
    ম["ক"] = ১
    ম["খ"] = ২
    লেখো(ম)
}
"#;
    let out = run("print_multi_field", src);
    let mut lines = out.lines();

    let struct_line = lines.next().expect("struct line");
    assert!(struct_line.starts_with('{') && struct_line.ends_with('}'), "not object-shaped: {struct_line:?}");
    assert!(struct_line.contains("\"নাম\": রহিম"), "missing নাম field: {struct_line:?}");
    assert!(struct_line.contains("\"বয়স\": 25"), "missing বয়স field: {struct_line:?}");

    let map_line = lines.next().expect("map line");
    assert!(map_line.starts_with('{') && map_line.ends_with('}'), "not object-shaped: {map_line:?}");
    assert!(map_line.contains("\"ক\": 1"), "missing ক entry: {map_line:?}");
    assert!(map_line.contains("\"খ\": 2"), "missing খ entry: {map_line:?}");

    assert!(lines.next().is_none(), "unexpected extra output: {out:?}");
}
