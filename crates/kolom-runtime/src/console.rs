//! `কলম কনসোল` — a native window that runs a `.ক` program and displays its
//! output, using the same USP10/Uniscribe complex-script shaping `ui.rs`
//! already relies on for `ডিসপ্লে`/`টেক্সট` (see `ui::imp::draw_shaped`).
//! Every terminal emulator renders text into a fixed monospace character-cell
//! grid, which cannot represent Bengali's reordering vowel signs (কার —
//! ে/ো/ৌ, which must appear *before* the consonant they logically follow) —
//! an unsolved, industry-wide limitation, not something fixable by terminal
//! configuration. This window sidesteps that entirely by shaping text the
//! same way a real text layout engine (Notepad, a browser) would.
//!
//! Sound only under the same single-UI-thread assumption `ui.rs` documents
//! for its own `static mut` engine state (line 16-20 there) — see this
//! module's `imp` doc comment.
//! Split the same way `kolom-cli/src/editor.rs` is: plain, `crossterm`/
//! `windows-sys`-free structs and functions first (unit-testable with
//! synthetic input), thin unsafe Win32 glue last. The engine runs entirely on
//! one thread — see `imp::run`'s doc comment for why (in short: the AST's
//! `Rc<FuncDecl>` fields make `Program` `!Send`, so there is no background
//! thread to hand it to; a nested Win32 message loop stands in for one).

#![allow(static_mut_refs)]

// ============================================================================
// Scrollback — append-only line buffer + scroll math. No Win32 dependency.
// ============================================================================

pub struct Scrollback {
    lines: Vec<String>,
    max_lines: usize,
}

impl Scrollback {
    pub fn new() -> Self {
        Scrollback { lines: vec![String::new()], max_lines: 5000 }
    }

    /// Appends arbitrary text, which may embed `\n` and may be a partial
    /// line with none — the only place line-splitting happens. Continues the
    /// last line across calls, so streamed output (one `লেখো` call's worth
    /// of bytes at a time) accumulates correctly rather than starting a new
    /// line per call.
    pub fn append(&mut self, text: &str) {
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                self.lines.push(String::new());
            }
            self.lines.last_mut().unwrap().push_str(part);
        }
        while self.lines.len() > self.max_lines {
            self.lines.remove(0);
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn visible(&self, top: usize, count: usize) -> &[String] {
        let top = top.min(self.lines.len());
        let end = (top + count).min(self.lines.len());
        &self.lines[top..end]
    }

    /// Clamps a requested scroll-top (e.g. from a mouse-wheel delta, which
    /// can go negative) to `[0, line_count - viewport_rows]`.
    pub fn clamp_top(&self, requested_top: i64, viewport_rows: usize) -> usize {
        let max_top = self.lines.len().saturating_sub(viewport_rows);
        requested_top.max(0).min(max_top as i64) as usize
    }

    /// The scroll-top that shows the last `viewport_rows` lines.
    pub fn bottom_top(&self, viewport_rows: usize) -> usize {
        self.lines.len().saturating_sub(viewport_rows)
    }

    /// CRLF-joined — the clipboard convention on Windows, where this feeds
    /// `CF_UNICODETEXT` for the console's "copy all output" action.
    pub fn all_text(&self) -> String {
        self.lines.join("\r\n")
    }
}

// ============================================================================
// Utf8ChunkWriter — UTF-8-boundary-safe byte -> text buffering. `Write::write`
// can hand us a chunk that ends mid-multibyte-sequence (a Bengali codepoint
// is 3 bytes), so a trailing partial sequence must be held for the next call
// rather than decoded (or dropped) immediately.
// ============================================================================

pub struct Utf8ChunkWriter {
    pending: Vec<u8>,
}

impl Utf8ChunkWriter {
    pub fn new() -> Self {
        Utf8ChunkWriter { pending: Vec::new() }
    }

    /// Feeds raw bytes, returns whatever is now decodable as valid UTF-8. A
    /// trailing incomplete multi-byte sequence is held in `pending` for the
    /// next call. A genuinely invalid byte is replaced with U+FFFD and
    /// skipped, rather than ever hanging or panicking.
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap());
                    match e.error_len() {
                        Some(bad) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..valid + bad);
                        }
                        None => {
                            // Incomplete trailing sequence — keep it for the next feed().
                            self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        out
    }
}

// ============================================================================
// backspace_one_scalar — fixes a real bug in ui.rs's existing W_INPUT widget,
// which does `inbuf.pop()` (removes one UTF-16 *code unit*). That can strand
// a lone surrogate half: an invalid UTF-16 sequence that later breaks
// ScriptStringAnalyse, not just a cosmetic issue. Popping a full scalar value
// (one or two UTF-16 units) is the minimum-viable correctness bar for v1 —
// full grapheme-cluster-aware backspace (one press deleting an entire
// Bengali conjunct) is deliberately out of scope.
// ============================================================================

pub fn backspace_one_scalar(inbuf: &mut Vec<u16>) {
    if let Some(&last) = inbuf.last() {
        if (0xDC00..=0xDFFF).contains(&last) && inbuf.len() >= 2 {
            inbuf.pop();
            inbuf.pop();
        } else {
            inbuf.pop();
        }
    }
}

// ============================================================================
// Win32 glue — thin, unsafe, deliberately untested (mirrors kolom-cli's
// `editor.rs` split: everything above this line is plain and unit-tested;
// everything below is event-loop wiring over it).
//
// Runs entirely on ONE thread — no background thread, no channels. Two
// reasons: `kolom_syntax::ast::Program.funcs: Vec<Rc<FuncDecl>>` uses `Rc`,
// which is never `Send`, so `Program` cannot be moved into a spawned thread
// without a much larger AST-wide `Rc` -> `Arc` change that would cost every
// other consumer (kolom-sema, kolom-interp, kolom-codegen-cranelift,
// kolom-lsp) for one feature's benefit; and `ui.rs`'s own module doc already
// establishes this engine's invariant that all `static mut` UI state is
// single-UI-thread-only. Instead, `পড়ো_লাইন` blocks via a *nested* Win32
// message loop — the same technique `MessageBox`/`DialogBox`/menu tracking
// use internally — so the window stays alive and responsive without ever
// leaving this thread.
// ============================================================================

#[cfg(windows)]
mod imp {
    use super::{backspace_one_scalar, Scrollback, Utf8ChunkWriter};
    use crate::ui::imp::{draw_shaped, load_usp, ui_font, wide};
    use kolom_interp::LineInput;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const TIMER_AUTOCLOSE: usize = 1;
    const CF_UNICODETEXT: u32 = 13;
    const LEFT_MARGIN: i32 = 8;
    const TOP_MARGIN: i32 = 6;

    fn env_ms(name: &str) -> Option<u32> {
        std::env::var(name).ok().and_then(|v| v.trim().parse::<u32>().ok()).filter(|&v| v > 0)
    }

    struct ConsoleWin {
        hwnd: HWND,
        line_height: i32,
        scrollback: Scrollback,
        scroll_top: usize,
        follow_tail: bool,
        inbuf: Vec<u16>,
        waiting_for_input: bool,
        submitted_line: Option<String>,
    }

    // Sound only because this whole engine is single-threaded by construction
    // (one window, one nested-loop-using thread) — see the module doc above.
    static mut CONSOLE: Option<ConsoleWin> = None;

    fn console() -> &'static mut ConsoleWin {
        unsafe { CONSOLE.as_mut().expect("console window not initialized") }
    }

    fn viewport_rows(cs: &ConsoleWin) -> usize {
        unsafe {
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(cs.hwnd, &mut rc);
            let usable = (rc.bottom - rc.top - TOP_MARGIN * 2).max(0);
            (usable / cs.line_height.max(1)).max(1) as usize
        }
    }

    fn copy_all_to_clipboard(hwnd: HWND) {
        unsafe {
            let text = wide(&console().scrollback.all_text());
            let bytes = text.len() * std::mem::size_of::<u16>();
            let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if hmem.is_null() {
                return;
            }
            let dst = GlobalLock(hmem) as *mut u16;
            if dst.is_null() {
                return;
            }
            std::ptr::copy_nonoverlapping(text.as_ptr(), dst, text.len());
            GlobalUnlock(hmem);
            if OpenClipboard(hwnd) != 0 {
                EmptyClipboard();
                SetClipboardData(CF_UNICODETEXT, hmem as _);
                CloseClipboard();
            }
        }
    }

    unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(h, &mut ps);
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(h, &mut rc);
                FillRect(dc, &rc, GetStockObject(WHITE_BRUSH) as _);

                let f = ui_font();
                let old = SelectObject(dc, f as _);
                SetBkMode(dc, TRANSPARENT as i32);

                let cs = console();
                let rows = viewport_rows(cs);
                let visible = cs.scrollback.visible(cs.scroll_top, rows);
                let mut y = TOP_MARGIN;
                for line in visible {
                    let w = wide(line);
                    draw_shaped(dc, &w, LEFT_MARGIN, y);
                    y += cs.line_height;
                }
                if cs.waiting_for_input {
                    let prompt: Vec<u16> = std::iter::once(b'>' as u16).chain(cs.inbuf.iter().copied()).chain(std::iter::once(0)).collect();
                    draw_shaped(dc, &prompt, LEFT_MARGIN, y);
                }

                SelectObject(dc, old);
                DeleteObject(f as _);
                EndPaint(h, &ps);
                0
            }
            WM_SIZE => {
                let cs = console();
                if cs.follow_tail {
                    cs.scroll_top = cs.scrollback.bottom_top(viewport_rows(cs));
                }
                InvalidateRect(h, std::ptr::null(), 0);
                0
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
                let cs = console();
                let rows = viewport_rows(cs);
                let requested = cs.scroll_top as i64 - ((delta / 120) * 3) as i64;
                cs.scroll_top = cs.scrollback.clamp_top(requested, rows);
                cs.follow_tail = cs.scroll_top == cs.scrollback.bottom_top(rows);
                InvalidateRect(h, std::ptr::null(), 0);
                0
            }
            WM_KEYDOWN => {
                let ctrl_down = (GetKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0;
                if ctrl_down && (wp == b'A' as usize || wp == b'C' as usize) {
                    copy_all_to_clipboard(h);
                }
                0
            }
            WM_CHAR => {
                let c = wp as u16;
                let cs = console();
                if !cs.waiting_for_input {
                    return 0;
                }
                if c == 13 {
                    // Enter
                    let line = String::from_utf16_lossy(&cs.inbuf);
                    cs.scrollback.append(&format!("{}\n", line));
                    cs.inbuf.clear();
                    cs.waiting_for_input = false;
                    cs.submitted_line = Some(line);
                } else if c == 8 {
                    backspace_one_scalar(&mut cs.inbuf);
                } else if c >= 32 && cs.inbuf.len() < 500 {
                    cs.inbuf.push(c);
                } else {
                    return 0; // control characters (Ctrl+A -> 1, Ctrl+C -> 3, ...) are no-ops here
                }
                if cs.follow_tail {
                    cs.scroll_top = cs.scrollback.bottom_top(viewport_rows(cs));
                }
                InvalidateRect(h, std::ptr::null(), 0);
                0
            }
            WM_TIMER if wp == TIMER_AUTOCLOSE => {
                PostQuitMessage(0);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }

    /// Drains messages already queued (non-blocking) so the window stays
    /// responsive — repaints, resizes, scrolls, Ctrl+C — during a
    /// print-heavy loop, without needing a background thread.
    unsafe fn pump_pending() {
        let mut m: MSG = std::mem::zeroed();
        while PeekMessageW(&mut m, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }

    unsafe fn create_window() -> HWND {
        let cls = wide("KolomConsoleWin");
        let hinst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst as _,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetStockObject(WHITE_BRUSH) as _,
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
        };
        RegisterClassW(&wc);
        let title = wide("কলম কনসোল");
        CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            900,
            600,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst as _,
            std::ptr::null(),
        )
    }

    /// Measures `ui_font()`'s line height once, via a temporary (non-painting)
    /// DC, so scrolling math has a real row height rather than a guess.
    unsafe fn measure_line_height(hwnd: HWND) -> i32 {
        let dc = GetDC(hwnd);
        let f = ui_font();
        let old = SelectObject(dc, f as _);
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        GetTextMetricsW(dc, &mut tm);
        SelectObject(dc, old);
        DeleteObject(f as _);
        ReleaseDC(hwnd, dc);
        (tm.tmHeight + tm.tmExternalLeading).max(16)
    }

    struct ConsoleWriter {
        buf: Utf8ChunkWriter,
        hwnd: HWND,
    }

    impl std::io::Write for ConsoleWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let s = self.buf.feed(bytes);
            if !s.is_empty() {
                let cs = console();
                cs.scrollback.append(&s);
                if cs.follow_tail {
                    cs.scroll_top = cs.scrollback.bottom_top(viewport_rows(cs));
                }
                unsafe {
                    InvalidateRect(self.hwnd, std::ptr::null(), 0);
                    pump_pending(); // keeps resize/scroll/Ctrl+C responsive mid-loop
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct ConsoleLineInput {
        hwnd: HWND,
        script: Vec<String>,
        script_pos: usize,
    }

    impl LineInput for ConsoleLineInput {
        fn read_line(&mut self) -> std::io::Result<Option<String>> {
            // Headless/CI path: KLOM_CONSOLE_SCRIPT_INPUT_FILE-provided lines
            // are consumed immediately, no message pump needed.
            if self.script_pos < self.script.len() {
                let line = self.script[self.script_pos].clone();
                self.script_pos += 1;
                let cs = console();
                cs.scrollback.append(&format!("{}\n", line));
                unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
                return Ok(Some(line));
            }
            console().waiting_for_input = true;
            unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
            unsafe {
                loop {
                    let mut m: MSG = std::mem::zeroed();
                    let r = GetMessageW(&mut m, std::ptr::null_mut(), 0, 0); // blocking: a deliberate nested message loop
                    if r <= 0 {
                        return Ok(None); // WM_QUIT (window closed) == EOF
                    }
                    TranslateMessage(&m);
                    DispatchMessageW(&m); // reaches wndproc's WM_CHAR handling above
                    if let Some(line) = console().submitted_line.take() {
                        return Ok(Some(line));
                    }
                }
            }
        }
    }

    pub(crate) fn run(prog: &kolom_syntax::ast::Program, argv: Vec<String>) -> std::io::Result<()> {
        unsafe {
            load_usp();
            let hwnd = create_window();
            let line_height = measure_line_height(hwnd);
            CONSOLE = Some(ConsoleWin {
                hwnd,
                line_height,
                scrollback: Scrollback::new(),
                scroll_top: 0,
                follow_tail: true,
                inbuf: Vec::new(),
                waiting_for_input: false,
                submitted_line: None,
            });
            if let Some(ms) = env_ms("KLOM_CONSOLE_AUTOCLOSE_MS") {
                SetTimer(hwnd, TIMER_AUTOCLOSE, ms, None);
            }
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);

            let script: Vec<String> = std::env::var("KLOM_CONSOLE_SCRIPT_INPUT_FILE")
                .ok()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|s| s.lines().map(String::from).collect())
                .unwrap_or_default();
            let mut writer = ConsoleWriter { buf: Utf8ChunkWriter::new(), hwnd };
            let mut input = ConsoleLineInput { hwnd, script, script_pos: 0 };

            // Runs synchronously on this (the UI) thread; `পড়ো_লাইন` pumps
            // messages via the nested loop above, keeping the window alive
            // and responsive throughout — see the module doc for why there
            // is no background thread here.
            let result = kolom_interp::run_with_io(prog, &mut writer, argv, &mut input);
            let cs = console();
            match result {
                Ok(()) => cs.scrollback.append("\n[প্রোগ্রাম শেষ হয়েছে]\n"),
                Err(e) => cs.scrollback.append(&format!("\nরানটাইম ত্রুটি {}:{}: {}\n", e.line, e.col, e.message)),
            }
            if cs.follow_tail {
                cs.scroll_top = cs.scrollback.bottom_top(viewport_rows(cs));
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);

            // Keep the window alive/scrollable/copyable after the program ends.
            let mut m: MSG = std::mem::zeroed();
            while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn run_console(_prog: &kolom_syntax::ast::Program, _argv: Vec<String>) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "কলম কনসোল শুধু Windows-এ সমর্থিত"))
}

#[cfg(windows)]
pub fn run_console(prog: &kolom_syntax::ast::Program, argv: Vec<String>) -> std::io::Result<()> {
    imp::run(prog, argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Scrollback ----

    #[test]
    fn append_single_line_no_newline() {
        let mut sb = Scrollback::new();
        sb.append("হ্যালো");
        assert_eq!(sb.line_count(), 1);
        assert_eq!(sb.visible(0, 10), &["হ্যালো"]);
    }

    #[test]
    fn append_splits_on_embedded_newlines() {
        let mut sb = Scrollback::new();
        sb.append("এক\nদুই\nতিন");
        assert_eq!(sb.visible(0, 10), &["এক", "দুই", "তিন"]);
    }

    #[test]
    fn append_continues_last_line_across_calls() {
        let mut sb = Scrollback::new();
        sb.append("হ্যা");
        sb.append("লো\n");
        sb.append("বিশ্ব");
        assert_eq!(sb.visible(0, 10), &["হ্যালো", "বিশ্ব"]);
    }

    #[test]
    fn visible_window_slice() {
        let mut sb = Scrollback::new();
        for i in 0..10 {
            sb.append(&format!("{}\n", i));
        }
        // 11 lines total (10 "\n"-terminated + one trailing empty line).
        assert_eq!(sb.line_count(), 11);
        assert_eq!(sb.visible(2, 3), &["2", "3", "4"]);
    }

    #[test]
    fn clamp_top_clamps_low_and_high() {
        let mut sb = Scrollback::new();
        for i in 0..20 {
            sb.append(&format!("{}\n", i));
        }
        assert_eq!(sb.clamp_top(-5, 10), 0);
        let max_top = sb.line_count() - 10;
        assert_eq!(sb.clamp_top(1_000_000, 10), max_top);
    }

    #[test]
    fn cap_drops_oldest_lines() {
        let mut sb = Scrollback { lines: vec![String::new()], max_lines: 5 };
        for i in 0..10 {
            sb.append(&format!("{}\n", i));
        }
        assert!(sb.line_count() <= 5);
        // The oldest lines ("0", "1", ...) must have been evicted.
        assert!(!sb.visible(0, sb.line_count()).contains(&"0".to_string()));
    }

    #[test]
    fn bengali_conjuncts_round_trip_unchanged() {
        // ক্ষ (ka + virama + ssa, a conjunct) and গ্যালারি-style ে/ো-kar
        // sequences (reordering vowel signs) — the exact bug class this
        // feature exists to fix must not be touched by the buffer itself.
        let mut sb = Scrollback::new();
        let text = "ক্ষমতা এবং কোনো ব্যবহারকারী";
        sb.append(text);
        assert_eq!(sb.visible(0, 1)[0], text);
    }

    // ---- Utf8ChunkWriter ----

    #[test]
    fn whole_string_returns_immediately() {
        let mut w = Utf8ChunkWriter::new();
        assert_eq!(w.feed("হ্যালো".as_bytes()), "হ্যালো");
    }

    #[test]
    fn bengali_char_split_byte_by_byte_across_three_feeds() {
        let bytes = "ক".as_bytes(); // 3 UTF-8 bytes
        assert_eq!(bytes.len(), 3);
        let mut w = Utf8ChunkWriter::new();
        assert_eq!(w.feed(&bytes[0..1]), "");
        assert_eq!(w.feed(&bytes[1..2]), "");
        assert_eq!(w.feed(&bytes[2..3]), "ক");
    }

    #[test]
    fn multiple_multibyte_chars_split_at_arbitrary_boundary() {
        let s = "কলম";
        let bytes = s.as_bytes();
        let mut w = Utf8ChunkWriter::new();
        let mut out = String::new();
        out.push_str(&w.feed(&bytes[0..4])); // splits mid-second-char
        out.push_str(&w.feed(&bytes[4..]));
        assert_eq!(out, s);
    }

    #[test]
    fn invalid_byte_does_not_hang_or_panic() {
        let mut w = Utf8ChunkWriter::new();
        let out = w.feed(&[b'a', 0xFF, b'b']);
        assert_eq!(out, "a\u{FFFD}b");
    }

    // ---- backspace_one_scalar ----

    #[test]
    fn pops_single_bmp_unit() {
        let mut buf: Vec<u16> = "ক".encode_utf16().collect();
        backspace_one_scalar(&mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn pops_full_surrogate_pair_not_just_half() {
        // U+1F600 (an astral-plane codepoint) encodes as one surrogate pair.
        let mut buf: Vec<u16> = "😀".encode_utf16().collect();
        assert_eq!(buf.len(), 2);
        backspace_one_scalar(&mut buf);
        assert!(buf.is_empty(), "must remove the whole pair, not strand a lone surrogate half");
    }

    #[test]
    fn empty_buffer_is_noop() {
        let mut buf: Vec<u16> = Vec::new();
        backspace_one_scalar(&mut buf);
        assert!(buf.is_empty());
    }
}
