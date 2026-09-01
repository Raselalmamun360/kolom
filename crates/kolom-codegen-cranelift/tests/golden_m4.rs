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

/// `মান`/`বসাও` on a `শেয়ার` cell — the interior-mutability accessors every
/// UI example's state relies on (engine.md §৭). Handlers print the value
/// `মান` reads back after each click, so this proves state genuinely
/// persists across rebuilds through the runtime's payload pointer rather
/// than, say, reading a stale copy each time.
#[test]
fn ui_shared_cell_get_set() {
    let src = r#"
ধ্রুবক গণনা: শেয়ার সংখ্যা = শেয়ার_করো(০)

ফাংশন বাড়াও() -> ফাঁকা {
    বসাও(গণনা, মান(গণনা) + ১)
    লেখো(লেখায়(মান(গণনা)))
}

অ্যাপ গণক {
    ডিসপ্লে {
        টেক্সট("গণনা: " + লেখায়(মান(গণনা)))
        বাটন("+", বাড়াও)
    }
}
"#;
    let out = run_with(
        "ui_shared_cell",
        src,
        &[("KLOM_UI_AUTOCLOSE_MS", "1200"), ("KLOM_UI_SCRIPT_CLICKS", "0,0,0")],
        None,
    );
    assert_eq!(out, "1
2
3
", "মান/বসাও should thread state through three clicks: {out:?}");
}

/// Canvas + গ্রাফিক্স draw commands + টিক timer callback.
#[test]
fn ui_graphics_canvas_tick() {
    let src = include_str!("../../kolom-cli/tests/golden/27_stdlib_graphics/main.ক");
    let out = run_with("ui_graphics", src, &[("KLOM_UI_AUTOCLOSE_MS", "900")], None);
    assert!(out.contains("টিক"), "expected tick handler output, got: {out:?}");
}

/// গ্রাফিক্স keyboard/mouse polling (কী_চাপা_হলো/মাউস_ক্লিক_হলো/মাউস_x/y).
/// `KLOM_UI_SCRIPT_INPUT` drives real `WM_KEYDOWN`/`WM_KEYUP`/`WM_MOUSEMOVE`/
/// `WM_LBUTTONDOWN`/`WM_LBUTTONUP` through the live message loop (see
/// `Ui::input_script` in kolom-runtime/src/ui.rs), so a print from the টিক
/// handler proves the messages actually reached `wndproc` and updated the
/// polled state — not just that the accessor functions exist.
#[test]
fn ui_graphics_keyboard_mouse_input() {
    let src = r#"
ইম্পোর্ট গ্রাফিক্স

ফাংশন প্রতি_টিক() -> ফাঁকা {
    যদি (গ্রাফিক্স.কী_চাপা_হলো(গ্রাফিক্স.উপরের_তীর)) {
        লেখো("উপরে চাপা হয়েছে")
    }
    যদি (গ্রাফিক্স.কী_চাপা_হলো(গ্রাফিক্স.স্পেস)) {
        লেখো("স্পেস চাপা হয়েছে")
    }
    যদি (গ্রাফিক্স.মাউস_ক্লিক_হলো(০)) {
        লেখো("ক্লিক: " + লেখায়(গ্রাফিক্স.মাউস_x()) + "," + লেখায়(গ্রাফিক্স.মাউস_y()))
    }
}

অ্যাপ {
    ডিসপ্লে {
        ক্যানভাস(200, 120)
    }
    গ্রাফিক্স.টিক(৫০, প্রতি_টিক)
}
"#;
    let out = run_with(
        "ui_input",
        src,
        &[
            ("KLOM_UI_AUTOCLOSE_MS", "1500"),
            ("KLOM_UI_SCRIPT_INPUT", "কী:38;কী:32;মাউস:70,90,0"),
        ],
        None,
    );
    assert_eq!(out, "উপরে চাপা হয়েছে\nস্পেস চাপা হয়েছে\nক্লিক: 70,90\n");
}
