//! Milestone-4: the native UI + 2D graphics engine, ported from the
//! since-removed legacy C backend's `ui_runtime()`/`graphics_runtime()`
//! C-text generators to real Rust over `windows-sys`.
//!
//! Design notes vs. the C original:
//! - The C version regenerated this whole engine as C source text on every
//!   build; here it's compiled once into kolom-runtime and merely *linked*.
//! - Widget storage is a plain `Vec` rather than the C fixed `KL_MAXW`/
//!   `KL_MAXK` pools, so widget/child counts are no longer capped at 256/24.
//! - Bengali complex-script shaping still goes through USP10
//!   (`ScriptStringAnalyse`/`Out`/`Free`), dynamically loaded exactly as the
//!   C version did, with a `TextOutW` fallback when unavailable.
//! - The `KLOM_UI_AUTOCLOSE_MS` / `KLOM_UI_SCRIPT_CLICKS` test hooks are
//!   preserved verbatim so existing headless UI tests keep working.
//!
//! Everything here is process-global mutable state driven by a single UI
//! thread (the Win32 message loop), matching the C original's design; the
//! `static mut` accesses are sound only under that single-threaded
//! assumption, which the engine enforces by construction (one window, one
//! loop, handlers invoked synchronously from the wndproc).

#![allow(static_mut_refs)]

#[cfg(windows)]
pub(crate) mod imp {
    use std::ffi::c_void;
    use windows_sys::core::PCWSTR;
    use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_UTF8};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const W_TEXT: i32 = 0;
    const W_BUTTON: i32 = 1;
    const W_INPUT: i32 = 2;
    const W_ROW: i32 = 3;
    const W_COL: i32 = 4;
    const W_CARD: i32 = 5;
    const W_DIALOG: i32 = 6;
    const W_SCROLL: i32 = 7;
    const W_CANVAS: i32 = 8;
    const W_IMAGE: i32 = 9;

    const C_COLOR: i32 = 0;
    const C_PIXEL: i32 = 1;
    const C_LINE: i32 = 2;
    const C_RECT: i32 = 3;
    const C_FILLRECT: i32 = 4;
    const C_CIRCLE: i32 = 5;
    const C_FILLCIRCLE: i32 = 6;
    const C_TEXT: i32 = 7;
    const C_FONT: i32 = 8;
    const C_ELLIPSE: i32 = 9;
    const C_FILLELLIPSE: i32 = 10;
    const C_PATH: i32 = 11;
    const C_POLY: i32 = 12;
    const C_FILLPOLY: i32 = 13;
    const C_ARC: i32 = 14;
    const C_SECTOR: i32 = 15;
    const C_FILLSECTOR: i32 = 16;

    const LEAF_H: i32 = 52;
    const GAP: i32 = 16;

    const TIMER_AUTOCLOSE: usize = 1;
    const TIMER_CLICKS: usize = 2;
    const TIMER_TICK: usize = 3;

    fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
        r | (g << 8) | (b << 16)
    }

    pub type Handler = extern "C" fn();

    #[derive(Default)]
    struct Widget {
        kind: i32,
        text: Vec<u16>,
        handler: Option<Handler>,
        inbuf: Vec<u16>,
        focused: bool,
        children: Vec<usize>,
        rect: RECT,
        gw: i32,
        gh: i32,
        bmp: HBITMAP,
    }

    struct GCmd {
        op: i32,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        /// A 5th int parameter — only `আর্ক`/`সেক্টর`/`ভরাট_সেক্টর` need it
        /// (cx, cy, r, start°, end°: five values, one more than a/b/c/d hold).
        e: i32,
        txt: Vec<u16>,
        /// Point list for `পথ`/`বহুভুজ`/`ভরাট_বহুভুজ` — empty for every other
        /// op. The same "one extra variable-length field on every command"
        /// pattern `txt` already established for text, just for points.
        pts: Vec<POINT>,
    }

    struct Ui {
        pool: Vec<Widget>,
        stack: Vec<usize>,
        root: Option<usize>,
        hwnd: HWND,
        on_rebuild: Option<Handler>,
        tick_fn: Option<Handler>,
        gbuf: Vec<GCmd>,
        gcolor: COLORREF,
        click_script: Vec<i32>,
        click_pos: usize,
    }

    static mut UI: Option<Ui> = None;

    fn ui() -> &'static mut Ui {
        unsafe {
            if UI.is_none() {
                UI = Some(Ui {
                    pool: Vec::new(),
                    stack: Vec::new(),
                    root: None,
                    hwnd: std::ptr::null_mut(),
                    on_rebuild: None,
                    tick_fn: None,
                    gbuf: Vec::new(),
                    gcolor: rgb(0, 0, 0),
                    click_script: Vec::new(),
                    click_pos: 0,
                });
            }
            UI.as_mut().unwrap()
        }
    }

    pub(crate) fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Converts a NUL-terminated UTF-8 C string to a NUL-terminated UTF-16
    /// buffer, mirroring the C engine's `kl_to_wide`.
    unsafe fn wide_from_c(p: *const u8) -> Vec<u16> {
        if p.is_null() {
            return vec![0];
        }
        let n = MultiByteToWideChar(CP_UTF8, 0, p, -1, std::ptr::null_mut(), 0);
        if n <= 0 {
            return vec![0];
        }
        let mut buf = vec![0u16; n as usize];
        MultiByteToWideChar(CP_UTF8, 0, p, -1, buf.as_mut_ptr(), n);
        buf
    }

    unsafe fn kolom_str_to_wide(p: *mut u8) -> Vec<u16> {
        // kolom `kl_str` layout: [rc: i64][len: i64][bytes...]
        if p.is_null() {
            return vec![0];
        }
        let len = *(p.add(8) as *const i64) as usize;
        let bytes = std::slice::from_raw_parts(p.add(16), len);
        let s = String::from_utf8_lossy(bytes);
        wide(&s)
    }

    // kolom array layout: [rc: i64][len: i64][cap: i64][elem_size: i64]
    // [drop_elem: i64][data...] — see kolom-runtime's own ARR_HEADER. `পথ`/
    // `বহুভুজ`/`ভরাট_বহুভুজ` take a `দশমিক[][]` (an array of `[x, y]`
    // points, `জ্যামিতি`'s shape-generator convention), decoded here by hand
    // rather than calling back into the crate root, matching how
    // `kolom_str_to_wide` above already reads `kl_str` directly.
    const ARR_HEADER: usize = 40;

    unsafe fn decode_points(p: *mut u8) -> Vec<POINT> {
        if p.is_null() {
            return Vec::new();
        }
        let outer_len = *(p.add(8) as *const i64);
        let mut pts = Vec::with_capacity(outer_len.max(0) as usize);
        for i in 0..outer_len {
            let inner_ptr = *(p.add(ARR_HEADER + (i as usize) * 8) as *const *mut u8);
            if inner_ptr.is_null() {
                continue;
            }
            let inner_len = *(inner_ptr.add(8) as *const i64);
            if inner_len < 2 {
                continue;
            }
            let x = *(inner_ptr.add(ARR_HEADER) as *const f64);
            let y = *(inner_ptr.add(ARR_HEADER + 8) as *const f64);
            pts.push(POINT { x: x as i32, y: y as i32 });
        }
        pts
    }

    // ---- USP10 complex-script shaping (dynamically loaded, as in C) ----

    type ScriptStringAnalyseFn = unsafe extern "system" fn(
        HDC,
        *const c_void,
        i32,
        i32,
        i32,
        u32,
        i32,
        *const c_void,
        *const c_void,
        *const i32,
        *const c_void,
        *const u8,
        *mut *mut c_void,
    ) -> i32;
    type ScriptStringOutFn =
        unsafe extern "system" fn(*mut c_void, i32, i32, u32, *const RECT, i32, i32, i32) -> i32;
    type ScriptStringFreeFn = unsafe extern "system" fn(*mut *mut c_void) -> i32;

    static mut USP_ANALYSE: Option<ScriptStringAnalyseFn> = None;
    static mut USP_OUT: Option<ScriptStringOutFn> = None;
    static mut USP_FREE: Option<ScriptStringFreeFn> = None;
    static mut USP_TRIED: bool = false;

    const SSA_GLYPHS: u32 = 0x00000080;
    const SSA_FALLBACK: u32 = 0x00000020;

    pub(crate) unsafe fn load_usp() {
        if USP_TRIED {
            return;
        }
        USP_TRIED = true;
        let name = wide("usp10.dll");
        let lib = LoadLibraryW(name.as_ptr());
        if lib.is_null() {
            return;
        }
        let get = |n: &[u8]| GetProcAddress(lib, n.as_ptr());
        if let Some(p) = get(b"ScriptStringAnalyse\0") {
            USP_ANALYSE = Some(std::mem::transmute::<_, ScriptStringAnalyseFn>(p));
        }
        if let Some(p) = get(b"ScriptStringOut\0") {
            USP_OUT = Some(std::mem::transmute::<_, ScriptStringOutFn>(p));
        }
        if let Some(p) = get(b"ScriptStringFree\0") {
            USP_FREE = Some(std::mem::transmute::<_, ScriptStringFreeFn>(p));
        }
    }

    pub(crate) fn wlen(w: &[u16]) -> i32 {
        w.iter().position(|&c| c == 0).unwrap_or(w.len()) as i32
    }

    pub(crate) unsafe fn draw_shaped(dc: HDC, txt: &[u16], x: i32, y: i32) {
        let n = wlen(txt);
        if let (Some(analyse), Some(out), Some(free)) = (USP_ANALYSE, USP_OUT, USP_FREE) {
            let mut ssa: *mut c_void = std::ptr::null_mut();
            let hr = analyse(
                dc,
                txt.as_ptr() as *const c_void,
                n,
                0,
                -1,
                SSA_GLYPHS | SSA_FALLBACK,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                &mut ssa,
            );
            if hr >= 0 && !ssa.is_null() {
                out(ssa, x, y, 0, std::ptr::null(), 0, 0, 0);
                free(&mut ssa);
                return;
            }
        }
        TextOutW(dc, x, y, txt.as_ptr(), n);
    }

    pub(crate) unsafe fn ui_font() -> HFONT {
        let face = wide("Nirmala UI");
        CreateFontW(
            -26,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_DONTCARE) as u32,
            face.as_ptr(),
        )
    }

    // ---- widget tree construction ----

    fn alloc(kind: i32) -> usize {
        let u = ui();
        let idx = u.pool.len();
        u.pool.push(Widget { kind, ..Default::default() });
        if let Some(&parent) = u.stack.last() {
            u.pool[parent].children.push(idx);
        }
        idx
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_begin() {
        let u = ui();
        u.pool.clear();
        u.stack.clear();
        u.root = None;
        // `গ্রাফিক্স.*` calls inside `ডিসপ্লে` re-run on every rebuild (a tick
        // or a state-changing handler), same as the widget tree above — but
        // unlike `pool`, `gbuf` was never cleared here, so every redraw just
        // appended to the last one. A moving/color-changing shape animated
        // via `টিক` accumulated an ever-growing trail instead of updating in
        // place, until the 4096-command cap silently froze the canvas.
        u.gbuf.clear();
        let r = alloc(W_COL);
        u.root = Some(r);
        ui().stack.push(r);
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_text(s: *mut u8) {
        let w = unsafe { kolom_str_to_wide(s) };
        let i = alloc(W_TEXT);
        ui().pool[i].text = w;
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_button(s: *mut u8, h: i64) {
        let w = unsafe { kolom_str_to_wide(s) };
        let i = alloc(W_BUTTON);
        ui().pool[i].text = w;
        if h != 0 {
            ui().pool[i].handler = Some(unsafe { std::mem::transmute::<usize, Handler>(h as usize) });
        }
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_input() {
        alloc(W_INPUT);
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_canvas(w: i64, h: i64) {
        let i = alloc(W_CANVAS);
        ui().pool[i].gw = w as i32;
        ui().pool[i].gh = h as i32;
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_image(path: *mut u8) {
        let w = unsafe { kolom_str_to_wide(path) };
        let i = alloc(W_IMAGE);
        let bmp = unsafe {
            LoadImageW(
                std::ptr::null_mut(),
                w.as_ptr(),
                IMAGE_BITMAP,
                0,
                0,
                LR_LOADFROMFILE | LR_CREATEDIBSECTION,
            )
        };
        ui().pool[i].bmp = bmp as HBITMAP;
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_push(kind: i64) {
        let i = alloc(kind as i32);
        ui().stack.push(i);
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_pop() {
        let u = ui();
        if u.stack.len() > 1 {
            u.stack.pop();
        }
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_row_kind() -> i64 {
        W_ROW as i64
    }
    #[no_mangle]
    pub extern "C" fn kl_ui_col_kind() -> i64 {
        W_COL as i64
    }
    #[no_mangle]
    pub extern "C" fn kl_ui_card_kind() -> i64 {
        W_CARD as i64
    }
    #[no_mangle]
    pub extern "C" fn kl_ui_dialog_kind() -> i64 {
        W_DIALOG as i64
    }
    #[no_mangle]
    pub extern "C" fn kl_ui_scroll_kind() -> i64 {
        W_SCROLL as i64
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_set_rebuild(h: i64) {
        ui().on_rebuild = if h == 0 {
            None
        } else {
            Some(unsafe { std::mem::transmute::<usize, Handler>(h as usize) })
        };
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_tick(ms: i64, h: i64) {
        let u = ui();
        u.tick_fn = if h == 0 {
            None
        } else {
            Some(unsafe { std::mem::transmute::<usize, Handler>(h as usize) })
        };
        let ms = ms.max(16) as u32;
        unsafe {
            SetTimer(u.hwnd, TIMER_TICK, ms, None);
        }
    }

    // ---- layout ----

    fn measure(idx: usize, w: i32) -> i32 {
        let u = ui();
        let kind = u.pool[idx].kind;
        let children = u.pool[idx].children.clone();
        match kind {
            W_CANVAS => {
                let gh = u.pool[idx].gh;
                if gh > 0 {
                    gh + 8
                } else {
                    88
                }
            }
            W_IMAGE => {
                let bmp = u.pool[idx].bmp;
                if !bmp.is_null() {
                    let mut bm: BITMAP = unsafe { std::mem::zeroed() };
                    let got = unsafe {
                        GetObjectW(
                            bmp as _,
                            std::mem::size_of::<BITMAP>() as i32,
                            &mut bm as *mut _ as *mut c_void,
                        )
                    };
                    if got != 0 {
                        return bm.bmHeight;
                    }
                }
                80
            }
            W_ROW => {
                let k = children.len().max(1) as i32;
                let cw = (w - (k - 1) * GAP) / k;
                children.iter().map(|&c| measure(c, cw)).max().unwrap_or(0)
            }
            W_CARD | W_DIALOG => {
                let mut total = 24;
                for &c in &children {
                    total += measure(c, w - 24) + GAP;
                }
                total
            }
            _ => {
                let mut total = 0;
                for &c in &children {
                    total += measure(c, w) + GAP;
                }
                if total > 0 {
                    total - GAP
                } else {
                    LEAF_H
                }
            }
        }
    }

    // ---- painting ----

    unsafe fn paint_canvas(dc: HDC, box_: RECT) {
        let u = ui();
        let saved = SaveDC(dc);
        IntersectClipRect(dc, box_.left, box_.top, box_.right, box_.bottom);
        let mut oldorg = POINT { x: 0, y: 0 };
        SetViewportOrgEx(dc, box_.left, box_.top, &mut oldorg);
        for i in 0..u.gbuf.len() {
            let (op, a, b, c, d, e) = {
                let cm = &u.gbuf[i];
                (cm.op, cm.a, cm.b, cm.c, cm.d, cm.e)
            };
            match op {
                C_COLOR => {
                    let col = rgb(a as u32, b as u32, c as u32);
                    u.gcolor = col;
                    SetTextColor(dc, col);
                }
                C_PIXEL => {
                    SetPixel(dc, a, b, u.gcolor);
                }
                C_LINE => {
                    let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                    let old = SelectObject(dc, pen as _);
                    MoveToEx(dc, a, b, std::ptr::null_mut());
                    LineTo(dc, c, d);
                    SelectObject(dc, old);
                    DeleteObject(pen as _);
                }
                C_RECT => {
                    let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                    let oldpen = SelectObject(dc, pen as _);
                    let oldbr = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                    Rectangle(dc, a, b, a + c, b + d);
                    SelectObject(dc, oldbr);
                    SelectObject(dc, oldpen);
                    DeleteObject(pen as _);
                }
                C_FILLRECT => {
                    let br = CreateSolidBrush(u.gcolor);
                    let fr = RECT { left: a, top: b, right: a + c, bottom: b + d };
                    FillRect(dc, &fr, br);
                    DeleteObject(br as _);
                }
                C_CIRCLE => {
                    let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                    let oldpen = SelectObject(dc, pen as _);
                    let oldbr = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                    Ellipse(dc, a - c, b - c, a + c, b + c);
                    SelectObject(dc, oldbr);
                    SelectObject(dc, oldpen);
                    DeleteObject(pen as _);
                }
                C_FILLCIRCLE => {
                    let br = CreateSolidBrush(u.gcolor);
                    let oldbr = SelectObject(dc, br as _);
                    let oldpen = SelectObject(dc, GetStockObject(NULL_PEN));
                    Ellipse(dc, a - c, b - c, a + c, b + c);
                    SelectObject(dc, oldpen);
                    SelectObject(dc, oldbr);
                    DeleteObject(br as _);
                }
                C_ELLIPSE => {
                    let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                    let oldpen = SelectObject(dc, pen as _);
                    let oldbr = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                    Ellipse(dc, a - c, b - d, a + c, b + d);
                    SelectObject(dc, oldbr);
                    SelectObject(dc, oldpen);
                    DeleteObject(pen as _);
                }
                C_FILLELLIPSE => {
                    let br = CreateSolidBrush(u.gcolor);
                    let oldbr = SelectObject(dc, br as _);
                    let oldpen = SelectObject(dc, GetStockObject(NULL_PEN));
                    Ellipse(dc, a - c, b - d, a + c, b + d);
                    SelectObject(dc, oldpen);
                    SelectObject(dc, oldbr);
                    DeleteObject(br as _);
                }
                C_PATH => {
                    let pts = u.gbuf[i].pts.clone();
                    if pts.len() >= 2 {
                        let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                        let old = SelectObject(dc, pen as _);
                        Polyline(dc, pts.as_ptr(), pts.len() as i32);
                        SelectObject(dc, old);
                        DeleteObject(pen as _);
                    }
                }
                C_POLY => {
                    let pts = u.gbuf[i].pts.clone();
                    if pts.len() >= 2 {
                        let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                        let oldpen = SelectObject(dc, pen as _);
                        let oldbr = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                        Polygon(dc, pts.as_ptr(), pts.len() as i32);
                        SelectObject(dc, oldbr);
                        SelectObject(dc, oldpen);
                        DeleteObject(pen as _);
                    }
                }
                C_FILLPOLY => {
                    let pts = u.gbuf[i].pts.clone();
                    if pts.len() >= 2 {
                        let br = CreateSolidBrush(u.gcolor);
                        let oldbr = SelectObject(dc, br as _);
                        let oldpen = SelectObject(dc, GetStockObject(NULL_PEN));
                        Polygon(dc, pts.as_ptr(), pts.len() as i32);
                        SelectObject(dc, oldpen);
                        SelectObject(dc, oldbr);
                        DeleteObject(br as _);
                    }
                }
                C_ARC | C_SECTOR | C_FILLSECTOR => {
                    // (a, b, c) = cx, cy, r; (d, e) = start°, end° — GDI's
                    // Arc/Pie sweep between two *points* on the ellipse, not
                    // angles directly, so the boundary points are computed
                    // here. Same angle convention as `জ্যামিতি.ঘোরানো`/
                    // `নিয়মিত_বহুভুজ`: 0° at +x, growing toward +y (which
                    // reads clockwise on screen, since screen y grows down).
                    let (cxf, cyf, rf) = (a as f64, b as f64, c as f64);
                    let r1 = (d as f64).to_radians();
                    let r2 = (e as f64).to_radians();
                    let x1 = (cxf + rf * r1.cos()) as i32;
                    let y1 = (cyf + rf * r1.sin()) as i32;
                    let x2 = (cxf + rf * r2.cos()) as i32;
                    let y2 = (cyf + rf * r2.sin()) as i32;
                    let (left, top, right, bottom) = (a - c, b - c, a + c, b + c);
                    match op {
                        C_ARC => {
                            let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                            let old = SelectObject(dc, pen as _);
                            Arc(dc, left, top, right, bottom, x1, y1, x2, y2);
                            SelectObject(dc, old);
                            DeleteObject(pen as _);
                        }
                        C_SECTOR => {
                            let pen = CreatePen(PS_SOLID, 1, u.gcolor);
                            let oldpen = SelectObject(dc, pen as _);
                            let oldbr = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                            Pie(dc, left, top, right, bottom, x1, y1, x2, y2);
                            SelectObject(dc, oldbr);
                            SelectObject(dc, oldpen);
                            DeleteObject(pen as _);
                        }
                        _ => {
                            let br = CreateSolidBrush(u.gcolor);
                            let oldbr = SelectObject(dc, br as _);
                            let oldpen = SelectObject(dc, GetStockObject(NULL_PEN));
                            Pie(dc, left, top, right, bottom, x1, y1, x2, y2);
                            SelectObject(dc, oldpen);
                            SelectObject(dc, oldbr);
                            DeleteObject(br as _);
                        }
                    }
                }
                C_TEXT => {
                    let txt = u.gbuf[i].txt.clone();
                    if !txt.is_empty() {
                        draw_shaped(dc, &txt, a, b);
                    }
                }
                C_FONT => {
                    let face = u.gbuf[i].txt.clone();
                    let face = if face.is_empty() { wide("Nirmala UI") } else { face };
                    let nf = CreateFontW(
                        -a,
                        0,
                        0,
                        0,
                        FW_NORMAL as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET as u32,
                        OUT_DEFAULT_PRECIS as u32,
                        CLIP_DEFAULT_PRECIS as u32,
                        CLEARTYPE_QUALITY as u32,
                        (DEFAULT_PITCH | FF_DONTCARE) as u32,
                        face.as_ptr(),
                    );
                    SelectObject(dc, nf as _);
                }
                _ => {}
            }
        }
        RestoreDC(dc, saved);
    }

    unsafe fn paint(dc: HDC, idx: usize, box_: RECT) {
        {
            let u = ui();
            u.pool[idx].rect = box_;
        }
        let f = ui_font();
        let old = SelectObject(dc, f as _);
        SetBkMode(dc, TRANSPARENT as i32);

        let kind = ui().pool[idx].kind;
        match kind {
            W_TEXT => {
                SetTextColor(dc, rgb(25, 25, 25));
                let t = ui().pool[idx].text.clone();
                draw_shaped(dc, &t, box_.left + 6, box_.top + 6);
            }
            W_BUTTON => {
                let br = CreateSolidBrush(rgb(0, 120, 215));
                FillRect(dc, &box_, br);
                DeleteObject(br as _);
                FrameRect(dc, &box_, GetStockObject(GRAY_BRUSH) as HBRUSH);
                SetTextColor(dc, rgb(255, 255, 255));
                let t = ui().pool[idx].text.clone();
                let mut sz = std::mem::zeroed();
                GetTextExtentPoint32W(dc, t.as_ptr(), wlen(&t), &mut sz);
                draw_shaped(
                    dc,
                    &t,
                    box_.left + ((box_.right - box_.left) - sz.cx) / 2,
                    box_.top + ((box_.bottom - box_.top) - sz.cy) / 2,
                );
            }
            W_INPUT => {
                let br = CreateSolidBrush(rgb(250, 250, 250));
                FillRect(dc, &box_, br);
                DeleteObject(br as _);
                let focused = ui().pool[idx].focused;
                let fb = CreateSolidBrush(if focused { rgb(0, 120, 215) } else { rgb(160, 160, 160) });
                FrameRect(dc, &box_, fb);
                DeleteObject(fb as _);
                SetTextColor(dc, rgb(25, 25, 25));
                let buf = ui().pool[idx].inbuf.clone();
                let mut z = buf.clone();
                z.push(0);
                draw_shaped(dc, &z, box_.left + 10, box_.top + 10);
                if focused {
                    let mut sz = std::mem::zeroed();
                    GetTextExtentPoint32W(dc, buf.as_ptr(), buf.len() as i32, &mut sz);
                    let cr = RECT {
                        left: box_.left + 12 + sz.cx,
                        top: box_.top + 10,
                        right: box_.left + 14 + sz.cx,
                        bottom: box_.bottom - 10,
                    };
                    let cb = CreateSolidBrush(rgb(0, 120, 215));
                    FillRect(dc, &cr, cb);
                    DeleteObject(cb as _);
                }
            }
            W_CANVAS => {
                let br = CreateSolidBrush(rgb(255, 255, 255));
                FillRect(dc, &box_, br);
                DeleteObject(br as _);
                FrameRect(dc, &box_, GetStockObject(GRAY_BRUSH) as HBRUSH);
                paint_canvas(dc, box_);
            }
            W_IMAGE => {
                let bmp = ui().pool[idx].bmp;
                if !bmp.is_null() {
                    let mem = CreateCompatibleDC(dc);
                    let oldb = SelectObject(mem, bmp as _);
                    let mut bm: BITMAP = std::mem::zeroed();
                    if GetObjectW(bmp as _, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut c_void) != 0 {
                        let (bw, bh) = (bm.bmWidth, bm.bmHeight);
                        let bwid = box_.right - box_.left;
                        let mut sc = if bwid > 0 { bwid as f64 / bw as f64 } else { 1.0 };
                        if (bh as f64 * sc) > (box_.bottom - box_.top) as f64 {
                            sc = (box_.bottom - box_.top) as f64 / bh as f64;
                        }
                        SetStretchBltMode(dc, COLORONCOLOR);
                        StretchBlt(
                            dc,
                            box_.left,
                            box_.top,
                            (bw as f64 * sc) as i32,
                            (bh as f64 * sc) as i32,
                            mem,
                            0,
                            0,
                            bw,
                            bh,
                            SRCCOPY,
                        );
                    }
                    SelectObject(mem, oldb);
                    DeleteDC(mem);
                } else {
                    let br = CreateSolidBrush(rgb(220, 220, 220));
                    FillRect(dc, &box_, br);
                    DeleteObject(br as _);
                    FrameRect(dc, &box_, GetStockObject(GRAY_BRUSH) as HBRUSH);
                }
            }
            W_ROW => {
                let children = ui().pool[idx].children.clone();
                let k = children.len().max(1) as i32;
                let cw = ((box_.right - box_.left) - (k - 1) * GAP) / k;
                let mut cx = box_.left;
                for &c in &children {
                    let ch = measure(c, cw);
                    let cr = RECT { left: cx, top: box_.top, right: cx + cw, bottom: box_.top + ch };
                    paint(dc, c, cr);
                    cx += cw + GAP;
                }
            }
            W_CARD | W_DIALOG => {
                let br = CreateSolidBrush(if kind == W_CARD { rgb(245, 245, 245) } else { rgb(235, 240, 250) });
                FillRect(dc, &box_, br);
                DeleteObject(br as _);
                FrameRect(dc, &box_, GetStockObject(GRAY_BRUSH) as HBRUSH);
                let children = ui().pool[idx].children.clone();
                let mut y = box_.top + 12;
                for &c in &children {
                    let ch = measure(c, (box_.right - box_.left) - 24);
                    let cr = RECT { left: box_.left + 12, top: y, right: box_.right - 12, bottom: y + ch };
                    paint(dc, c, cr);
                    y += ch + GAP;
                }
            }
            _ => {
                let children = ui().pool[idx].children.clone();
                let mut y = box_.top;
                for &c in &children {
                    let ch = measure(c, box_.right - box_.left);
                    let cr = RECT { left: box_.left, top: y, right: box_.right, bottom: y + ch };
                    paint(dc, c, cr);
                    y += ch + GAP;
                }
            }
        }
        SelectObject(dc, old);
        DeleteObject(f as _);
    }

    // ---- graphics command buffer (গ্রাফিক্স module) ----

    fn gpush(op: i32, a: i32, b: i32, c: i32, d: i32, txt: Vec<u16>) {
        gpush_full(op, a, b, c, d, 0, txt, Vec::new());
    }

    /// আর্ক/সেক্টর family — need a 5th int parameter.
    fn gpush_e(op: i32, a: i32, b: i32, c: i32, d: i32, e: i32) {
        gpush_full(op, a, b, c, d, e, Vec::new(), Vec::new());
    }

    /// পথ/বহুভুজ/ভরাট_বহুভুজ family — need a point list instead of a/b/c/d.
    fn gpush_pts(op: i32, pts: Vec<POINT>) {
        gpush_full(op, 0, 0, 0, 0, 0, Vec::new(), pts);
    }

    fn gpush_full(op: i32, a: i32, b: i32, c: i32, d: i32, e: i32, txt: Vec<u16>, pts: Vec<POINT>) {
        let u = ui();
        if u.gbuf.len() >= 4096 {
            return;
        }
        u.gbuf.push(GCmd { op, a, b, c, d, e, txt, pts });
    }

    #[no_mangle]
    pub extern "C" fn kl_g_color(r: i64, g: i64, b: i64) {
        ui().gcolor = rgb(r as u32, g as u32, b as u32);
        gpush(C_COLOR, r as i32, g as i32, b as i32, 0, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_pixel(x: i64, y: i64) {
        gpush(C_PIXEL, x as i32, y as i32, 0, 0, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_line(x1: i64, y1: i64, x2: i64, y2: i64) {
        gpush(C_LINE, x1 as i32, y1 as i32, x2 as i32, y2 as i32, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_rect(x: i64, y: i64, w: i64, h: i64) {
        gpush(C_RECT, x as i32, y as i32, w as i32, h as i32, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_fillrect(x: i64, y: i64, w: i64, h: i64) {
        gpush(C_FILLRECT, x as i32, y as i32, w as i32, h as i32, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_circle(cx: i64, cy: i64, r: i64) {
        gpush(C_CIRCLE, cx as i32, cy as i32, r as i32, 0, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_fillcircle(cx: i64, cy: i64, r: i64) {
        gpush(C_FILLCIRCLE, cx as i32, cy as i32, r as i32, 0, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_ellipse(cx: i64, cy: i64, rx: i64, ry: i64) {
        gpush(C_ELLIPSE, cx as i32, cy as i32, rx as i32, ry as i32, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_fillellipse(cx: i64, cy: i64, rx: i64, ry: i64) {
        gpush(C_FILLELLIPSE, cx as i32, cy as i32, rx as i32, ry as i32, Vec::new());
    }
    #[no_mangle]
    pub extern "C" fn kl_g_path(points: *mut u8) {
        gpush_pts(C_PATH, unsafe { decode_points(points) });
    }
    #[no_mangle]
    pub extern "C" fn kl_g_polygon(points: *mut u8) {
        gpush_pts(C_POLY, unsafe { decode_points(points) });
    }
    #[no_mangle]
    pub extern "C" fn kl_g_fillpolygon(points: *mut u8) {
        gpush_pts(C_FILLPOLY, unsafe { decode_points(points) });
    }
    #[no_mangle]
    pub extern "C" fn kl_g_arc(cx: i64, cy: i64, r: i64, start: i64, end: i64) {
        gpush_e(C_ARC, cx as i32, cy as i32, r as i32, start as i32, end as i32);
    }
    #[no_mangle]
    pub extern "C" fn kl_g_sector(cx: i64, cy: i64, r: i64, start: i64, end: i64) {
        gpush_e(C_SECTOR, cx as i32, cy as i32, r as i32, start as i32, end as i32);
    }
    #[no_mangle]
    pub extern "C" fn kl_g_fillsector(cx: i64, cy: i64, r: i64, start: i64, end: i64) {
        gpush_e(C_FILLSECTOR, cx as i32, cy as i32, r as i32, start as i32, end as i32);
    }
    #[no_mangle]
    pub extern "C" fn kl_g_text(x: i64, y: i64, s: *mut u8) {
        let w = unsafe { kolom_str_to_wide(s) };
        gpush(C_TEXT, x as i32, y as i32, 0, 0, w);
    }
    #[no_mangle]
    pub extern "C" fn kl_g_font(s: *mut u8, size: i64) {
        let w = unsafe { kolom_str_to_wide(s) };
        gpush(C_FONT, size as i32, 0, 0, 0, w);
    }

    // ---- window + message loop ----

    fn env_ms(name: &str) -> Option<u32> {
        std::env::var(name).ok().and_then(|v| v.trim().parse::<u32>().ok()).filter(|&v| v > 0)
    }

    fn next_scripted_click() -> Option<i32> {
        let u = ui();
        if u.click_pos < u.click_script.len() {
            let v = u.click_script[u.click_pos];
            u.click_pos += 1;
            Some(v)
        } else {
            None
        }
    }

    unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(h, &mut ps);
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(h, &mut rc);
                FillRect(dc, &rc, GetStockObject(WHITE_BRUSH) as HBRUSH);
                if let Some(root) = ui().root {
                    if root < ui().pool.len() {
                        paint(dc, root, rc);
                    }
                }
                EndPaint(h, &ps);
                0
            }
            WM_LBUTTONDOWN => {
                let mx = (lp & 0xFFFF) as i16 as i32;
                let my = ((lp >> 16) & 0xFFFF) as i16 as i32;
                let u = ui();
                let mut hit_btn: Option<usize> = None;
                let mut hit_in: Option<usize> = None;
                for i in 0..u.pool.len() {
                    let r = u.pool[i].rect;
                    if mx >= r.left && mx <= r.right && my >= r.top && my <= r.bottom {
                        if u.pool[i].kind == W_BUTTON && hit_btn.is_none() {
                            hit_btn = Some(i);
                        }
                        if u.pool[i].kind == W_INPUT {
                            hit_in = Some(i);
                        }
                    }
                }
                let mut changed = false;
                for i in 0..u.pool.len() {
                    if u.pool[i].kind == W_INPUT {
                        let want = Some(i) == hit_in;
                        if u.pool[i].focused != want {
                            u.pool[i].focused = want;
                            changed = true;
                        }
                    }
                }
                if let Some(bi) = hit_btn {
                    if let Some(handler) = u.pool[bi].handler {
                        handler();
                        if let Some(rb) = ui().on_rebuild {
                            rb();
                            InvalidateRect(h, std::ptr::null(), 1);
                            return 0;
                        }
                        changed = true;
                    }
                }
                if changed {
                    InvalidateRect(h, std::ptr::null(), 1);
                }
                SetFocus(h);
                0
            }
            WM_CHAR => {
                let c = wp as u16;
                let u = ui();
                for i in 0..u.pool.len() {
                    if u.pool[i].kind == W_INPUT && u.pool[i].focused {
                        if c == 8 {
                            u.pool[i].inbuf.pop();
                        } else if c >= 32 && u.pool[i].inbuf.len() < 126 {
                            u.pool[i].inbuf.push(c);
                        }
                        InvalidateRect(h, std::ptr::null(), 1);
                        return 0;
                    }
                }
                DefWindowProcW(h, msg, wp, lp)
            }
            WM_TIMER => {
                match wp {
                    TIMER_TICK => {
                        if let Some(t) = ui().tick_fn {
                            t();
                        }
                        if let Some(rb) = ui().on_rebuild {
                            rb();
                            InvalidateRect(h, std::ptr::null(), 1);
                        }
                        0
                    }
                    TIMER_CLICKS => {
                        if let Some(mut idx) = next_scripted_click() {
                            let u = ui();
                            let mut target = None;
                            for i in 0..u.pool.len() {
                                if u.pool[i].kind == W_BUTTON {
                                    if idx == 0 {
                                        target = Some(i);
                                        break;
                                    }
                                    idx -= 1;
                                }
                            }
                            if let Some(i) = target {
                                if let Some(handler) = u.pool[i].handler {
                                    handler();
                                }
                                if let Some(rb) = ui().on_rebuild {
                                    rb();
                                }
                            }
                            InvalidateRect(h, std::ptr::null(), 1);
                            let ms = env_ms("KLOM_UI_AUTOCLOSE_MS").map(|v| v / 3).unwrap_or(400).max(60);
                            SetTimer(h, TIMER_CLICKS, ms, None);
                            return 0;
                        }
                        PostQuitMessage(0);
                        0
                    }
                    _ => {
                        KillTimer(h, wp);
                        PostQuitMessage(0);
                        0
                    }
                }
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_init(title: *mut u8) {
        unsafe {
            load_usp();
            let cls = wide("KolomWin");
            let hinst: HINSTANCE = GetModuleHandleW(std::ptr::null()) as HINSTANCE;
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: GetStockObject(WHITE_BRUSH) as HBRUSH,
                lpszMenuName: std::ptr::null(),
                lpszClassName: cls.as_ptr(),
            };
            RegisterClassW(&wc);
            let t = kolom_str_to_wide(title);
            let style = WS_OVERLAPPEDWINDOW & !(WS_MAXIMIZEBOX | WS_THICKFRAME);
            let hwnd = CreateWindowExW(
                0,
                cls.as_ptr(),
                t.as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                440,
                620,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            );
            ui().hwnd = hwnd;

            if let Ok(sc) = std::env::var("KLOM_UI_SCRIPT_CLICKS") {
                if !sc.trim().is_empty() {
                    let u = ui();
                    u.click_script = sc.split(',').filter_map(|p| p.trim().parse::<i32>().ok()).collect();
                    u.click_pos = 0;
                    SetTimer(hwnd, TIMER_CLICKS, 300, None);
                    return;
                }
            }
            if let Some(ms) = env_ms("KLOM_UI_AUTOCLOSE_MS") {
                SetTimer(hwnd, TIMER_AUTOCLOSE, ms, None);
            }
        }
    }

    #[no_mangle]
    pub extern "C" fn kl_ui_show_and_run() {
        unsafe {
            let hwnd = ui().hwnd;
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            let mut m: MSG = std::mem::zeroed();
            while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
    }

    /// Unused today but kept so `kl_ui_image`'s `wide_from_c` path and the
    /// PCWSTR import don't drift out of sync with the rest of the module.
    #[allow(dead_code)]
    unsafe fn _unused(p: *const u8) -> PCWSTR {
        let v = wide_from_c(p);
        v.as_ptr()
    }
}

// ---------------------------------------------------------------------------
// Non-Windows: console stubs — a UI program still compiles and runs
// (headlessly) on other platforms.
// ---------------------------------------------------------------------------
#[cfg(not(windows))]
mod imp {
    macro_rules! stub {
        ($name:ident ( $($arg:ident : $ty:ty),* )) => {
            #[no_mangle]
            pub extern "C" fn $name($(_: $ty),*) {}
        };
        ($name:ident ( $($arg:ident : $ty:ty),* ) -> $ret:ty = $val:expr) => {
            #[no_mangle]
            pub extern "C" fn $name($(_: $ty),*) -> $ret { $val }
        };
    }

    stub!(kl_ui_begin());
    stub!(kl_ui_text(s: *mut u8));
    stub!(kl_ui_button(s: *mut u8, h: i64));
    stub!(kl_ui_input());
    stub!(kl_ui_canvas(w: i64, h: i64));
    stub!(kl_ui_image(p: *mut u8));
    stub!(kl_ui_push(k: i64));
    stub!(kl_ui_pop());
    stub!(kl_ui_set_rebuild(h: i64));
    stub!(kl_ui_tick(ms: i64, h: i64));
    stub!(kl_ui_init(t: *mut u8));
    stub!(kl_ui_show_and_run());
    stub!(kl_ui_row_kind() -> i64 = 3);
    stub!(kl_ui_col_kind() -> i64 = 4);
    stub!(kl_ui_card_kind() -> i64 = 5);
    stub!(kl_ui_dialog_kind() -> i64 = 6);
    stub!(kl_ui_scroll_kind() -> i64 = 7);
    stub!(kl_g_color(r: i64, g: i64, b: i64));
    stub!(kl_g_pixel(x: i64, y: i64));
    stub!(kl_g_line(a: i64, b: i64, c: i64, d: i64));
    stub!(kl_g_rect(a: i64, b: i64, c: i64, d: i64));
    stub!(kl_g_fillrect(a: i64, b: i64, c: i64, d: i64));
    stub!(kl_g_circle(a: i64, b: i64, c: i64));
    stub!(kl_g_fillcircle(a: i64, b: i64, c: i64));
    stub!(kl_g_ellipse(a: i64, b: i64, c: i64, d: i64));
    stub!(kl_g_fillellipse(a: i64, b: i64, c: i64, d: i64));
    stub!(kl_g_path(points: *mut u8));
    stub!(kl_g_polygon(points: *mut u8));
    stub!(kl_g_fillpolygon(points: *mut u8));
    stub!(kl_g_arc(a: i64, b: i64, c: i64, d: i64, e: i64));
    stub!(kl_g_sector(a: i64, b: i64, c: i64, d: i64, e: i64));
    stub!(kl_g_fillsector(a: i64, b: i64, c: i64, d: i64, e: i64));
    stub!(kl_g_text(x: i64, y: i64, s: *mut u8));
    stub!(kl_g_font(s: *mut u8, size: i64));
}
