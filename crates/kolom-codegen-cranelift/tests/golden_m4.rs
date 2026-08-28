//! Milestone-4 acceptance test: the native UI + 2D graphics engine.
//!
//! These run headlessly via the same `KLOM_UI_AUTOCLOSE_MS` /
//! `KLOM_UI_SCRIPT_CLICKS` env hooks the C backend's UI tests use — a real
//! window IS created and a real Win32 message loop DOES run; it just closes
//! itself (or drives scripted button clicks) instead of waiting for a human.

#![cfg(windows)]

mod harness;
use harness::run_with;

/// Static widget tree: opens a real window, paints টেক্সট/বাটন/ইনপুট, closes.
#[test]
fn ui_static_smoke() {
    let src = include_str!("../../kolom-cli/tests/ui/main.ক");
    run_with("ui_static", src, &[("KLOM_UI_AUTOCLOSE_MS", "500")], None);
}

/// Container widgets + scripted clicks. Asserting on stdout proves the
/// handlers really dispatched through the live message loop.
#[test]
fn ui_dynamic_counter_rebuild() {
    let src = r#"
ধ্রুবক গণনা: শেয়ার সংখ্যা = শেয়ার_করো(০)

ফাংশন বাড়াও() -> ফাঁকা {
    লেখো("বাড়ল")
}

ফাংশন কমাও() -> ফাঁকা {
    লেখো("কমল")
}

অ্যাপ গণক {
    ডিসপ্লে {
        টেক্সট("কলম গণক")
        সারি() {
            বাটন("+", বাড়াও)
            বাটন("-", কমাও)
        }
        ইনপুট()
    }
}
"#;
    let out = run_with(
        "ui_dynamic",
        src,
        &[("KLOM_UI_AUTOCLOSE_MS", "1500"), ("KLOM_UI_SCRIPT_CLICKS", "0,0,1")],
        None,
    );
    // clicks: button 0, button 0, button 1
    assert_eq!(out, "বাড়ল\nবাড়ল\nকমল\n");
}

/// Canvas + গ্রাফিক্স draw commands + টিক timer callback.
#[test]
fn ui_graphics_canvas_tick() {
    let src = include_str!("../../kolom-cli/tests/golden/27_stdlib_graphics/main.ক");
    let out = run_with("ui_graphics", src, &[("KLOM_UI_AUTOCLOSE_MS", "900")], None);
    assert!(out.contains("টিক"), "expected tick handler output, got: {out:?}");
}
