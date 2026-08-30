//! `কলম পাতা` — a small built-in terminal editor with live diagnostics and
//! a run-in-place keybind ("পাতা" = page, pairing with কলম = pen), so
//! writing and debugging a `.ক` file doesn't require installing a separate
//! editor.
//!
//! No C compiler anywhere in this path: `কলম পাতা` is `kolom-cli` itself
//! (a plain Rust binary), and its "run in place" keybind goes through
//! `kolom-interp` — the same interpreter `কলম চালাও` uses — never the
//! Cranelift or C build backends.
//!
//! Split deliberately into two halves: `Buffer`, `apply_key`, and
//! `analyze_buffer`/`build_program` are plain functions/structs with no
//! `crossterm` dependency at all — every test in this module drives them
//! directly with synthetic input, the same way `kolom-runtime`'s UI engine
//! tests script clicks instead of needing a human at a real window. The
//! actual `crossterm` event loop (`run`) is kept as thin glue over those
//! pieces, so the only code that can't be unit-tested is also the least
//! likely to hide a real bug.
//!
//! Live diagnostics replicate `kolom-lsp`'s `analyze()` directly (lex →
//! parse → resolve_user_modules → sema) rather than depending on
//! `kolom-lsp` — that crate already depends on `kolom-cli` for
//! `resolve_user_modules`, so the reverse dependency would be a cycle.

use std::io::{stdout, Write as _};
use std::path::{Path, PathBuf};

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, ClearType};
use crossterm::{execute, queue};

// ============================================================================
// Buffer — plain text-editing state, no terminal dependency
// ============================================================================

pub struct Buffer {
    lines: Vec<String>,
    pub row: usize,
    pub col: usize,
    pub dirty: bool,
    pub path: PathBuf,
}

impl Buffer {
    pub fn load(path: PathBuf) -> Self {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Buffer { lines, row: 0, col: 0, dirty: false, path }
    }

    #[cfg(test)]
    pub fn from_text(text: &str, path: PathBuf) -> Self {
        let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Buffer { lines, row: 0, col: 0, dirty: false, path }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, i: usize) -> &str {
        &self.lines[i]
    }

    fn clamp_col(&mut self) {
        let len = self.lines[self.row].chars().count();
        if self.col > len {
            self.col = len;
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let b = char_to_byte(&self.lines[self.row], self.col);
        self.lines[self.row].insert(b, c);
        self.col += 1;
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        let b = char_to_byte(&self.lines[self.row], self.col);
        let rest = self.lines[self.row].split_off(b);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            let b = char_to_byte(&self.lines[self.row], self.col - 1);
            self.lines[self.row].remove(b);
            self.col -= 1;
            self.dirty = true;
        } else if self.row > 0 {
            let prev_len = self.lines[self.row - 1].chars().count();
            let cur = self.lines.remove(self.row);
            self.row -= 1;
            self.lines[self.row].push_str(&cur);
            self.col = prev_len;
            self.dirty = true;
        }
    }

    pub fn delete_forward(&mut self) {
        let len = self.lines[self.row].chars().count();
        if self.col < len {
            let b = char_to_byte(&self.lines[self.row], self.col);
            self.lines[self.row].remove(b);
            self.dirty = true;
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
            self.dirty = true;
        }
    }

    pub fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
        }
    }

    pub fn move_right(&mut self) {
        let len = self.lines[self.row].chars().count();
        if self.col < len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.clamp_col();
        }
    }

    pub fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.clamp_col();
        }
    }

    pub fn move_home(&mut self) {
        self.col = 0;
    }

    pub fn move_end(&mut self) {
        self.col = self.lines[self.row].chars().count();
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.text())?;
        self.dirty = false;
        Ok(())
    }
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

// ============================================================================
// Key handling — plain function, no terminal dependency
// ============================================================================

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Save,
    Run,
    Quit,
}

pub fn apply_key(buf: &mut Buffer, key: KeyEvent) -> Action {
    match (key.code, key.modifiers) {
        (KeyCode::Char('s'), m) if m.contains(KeyModifiers::CONTROL) => Action::Save,
        (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => Action::Run,
        (KeyCode::Char('q'), m) if m.contains(KeyModifiers::CONTROL) => Action::Quit,
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
            buf.insert_char(c);
            Action::None
        }
        (KeyCode::Enter, _) => {
            buf.insert_newline();
            Action::None
        }
        (KeyCode::Backspace, _) => {
            buf.backspace();
            Action::None
        }
        (KeyCode::Delete, _) => {
            buf.delete_forward();
            Action::None
        }
        (KeyCode::Tab, _) => {
            for _ in 0..4 {
                buf.insert_char(' ');
            }
            Action::None
        }
        (KeyCode::Left, _) => {
            buf.move_left();
            Action::None
        }
        (KeyCode::Right, _) => {
            buf.move_right();
            Action::None
        }
        (KeyCode::Up, _) => {
            buf.move_up();
            Action::None
        }
        (KeyCode::Down, _) => {
            buf.move_down();
            Action::None
        }
        (KeyCode::Home, _) => {
            buf.move_home();
            Action::None
        }
        (KeyCode::End, _) => {
            buf.move_end();
            Action::None
        }
        _ => Action::None,
    }
}

// ============================================================================
// Diagnostics / run pipeline — plain functions, no terminal dependency
// ============================================================================

/// Runs the full lex → parse → resolve_user_modules → sema pipeline,
/// stopping at whichever stage first reports anything — mirrors
/// `kolom-lsp`'s `analyze()` and `kolom-cli`'s own `check_program`.
fn build_program(text: &str, dir: Option<&Path>) -> Result<kolom_syntax::ast::Program, Vec<kolom_lexer::Diagnostic>> {
    let (tokens, lex_errs) = kolom_lexer::lex(text);
    if !lex_errs.is_empty() {
        return Err(lex_errs);
    }
    let (mut prog, parse_errs) = kolom_syntax::parse(tokens);
    if !parse_errs.is_empty() {
        return Err(parse_errs);
    }
    if let Some(dir) = dir {
        if let Err(e) = kolom_cli::resolve_user_modules(&mut prog, dir) {
            return Err(vec![kolom_lexer::Diagnostic { line: 1, col: 1, message: e }]);
        }
    }
    let sema_errs = kolom_sema::analyze(&prog);
    if !sema_errs.is_empty() {
        return Err(sema_errs);
    }
    Ok(prog)
}

pub fn analyze_buffer(text: &str, dir: Option<&Path>) -> Vec<kolom_lexer::Diagnostic> {
    match build_program(text, dir) {
        Ok(_) => Vec::new(),
        Err(diags) => diags,
    }
}

// ============================================================================
// Terminal loop — thin glue over the pieces above
// ============================================================================

pub fn run(path: &Path) -> std::io::Result<()> {
    let mut buf = Buffer::load(path.to_path_buf());
    let dir = path.parent().map(|p| p.to_path_buf());

    terminal::enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
    let result = editor_loop(&mut buf, dir.as_deref());
    let _ = execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    result
}

fn editor_loop(buf: &mut Buffer, dir: Option<&Path>) -> std::io::Result<()> {
    let mut diags = analyze_buffer(&buf.text(), dir);
    render(buf, &diags)?;
    loop {
        if let Event::Key(key) = event::read()? {
            // Windows' conpty reports both press and release; every other
            // platform only ever sends press. Without this filter every
            // keystroke fires twice on Windows.
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match apply_key(buf, key) {
                Action::Quit => {
                    if buf.dirty && !confirm_quit()? {
                        render(buf, &diags)?;
                        continue;
                    }
                    break;
                }
                Action::Save => {
                    let _ = buf.save();
                }
                Action::Run => {
                    run_in_place(buf, dir)?;
                }
                Action::None => {}
            }
            diags = analyze_buffer(&buf.text(), dir);
            render(buf, &diags)?;
        }
    }
    Ok(())
}

fn confirm_quit() -> std::io::Result<bool> {
    let (_, rows) = terminal::size()?;
    let mut out = stdout();
    queue!(out, cursor::MoveTo(0, rows.saturating_sub(1)))?;
    queue!(out, Print("সংরক্ষণ করা হয়নি — তবু বের হবেন? (y/n) "))?;
    out.flush()?;
    loop {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(true),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => return Ok(false),
                _ => {}
            }
        }
    }
}

/// Leaves the alternate screen to run the *current buffer* (not necessarily
/// saved) through `kolom-interp` with a real, cooked terminal underneath —
/// so a program that reads `পড়ো_লাইন` from stdin works normally. Simpler
/// and lower-risk than an in-editor split pane for v1.
fn run_in_place(buf: &Buffer, dir: Option<&Path>) -> std::io::Result<()> {
    execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    println!();
    match build_program(&buf.text(), dir) {
        Ok(prog) => {
            let stdout_h = std::io::stdout();
            let mut lock = stdout_h.lock();
            if let Err(e) = kolom_interp::run(&prog, &mut lock) {
                println!("রানটাইম ত্রুটি {}:{}: {}", e.line, e.col, e.message);
            }
        }
        Err(diags) => {
            for d in &diags {
                println!("{}:{}: {}", d.line, d.col, d.message);
            }
        }
    }
    println!("\n[যেকোনো কী চাপুন পাতায় ফিরে যেতে...]");
    std::io::stdout().flush()?;
    loop {
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Release {
                break;
            }
        }
    }
    terminal::enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
    Ok(())
}

fn render(buf: &Buffer, diags: &[kolom_lexer::Diagnostic]) -> std::io::Result<()> {
    let (cols, rows) = terminal::size()?;
    let cols = cols as usize;
    let rows = rows as usize;
    let text_rows = rows.saturating_sub(1).max(1);
    let gutter = 5usize;

    let mut out = stdout();
    execute!(out, terminal::Clear(ClearType::All))?;

    let start = buf.row.saturating_sub(text_rows.saturating_sub(1));
    let error_lines: std::collections::HashSet<usize> = diags.iter().map(|d| d.line.saturating_sub(1) as usize).collect();

    for i in 0..text_rows {
        let line_idx = start + i;
        if line_idx >= buf.line_count() {
            break;
        }
        queue!(out, cursor::MoveTo(0, i as u16))?;
        let has_err = error_lines.contains(&line_idx);
        if has_err {
            queue!(out, SetForegroundColor(Color::Red))?;
        }
        queue!(out, Print(format!("{:>3}{} ", line_idx + 1, if has_err { "✗" } else { " " })))?;
        if has_err {
            queue!(out, ResetColor)?;
        }
        let max_w = cols.saturating_sub(gutter);
        let shown: String = buf.line(line_idx).chars().take(max_w).collect();
        queue!(out, Print(shown))?;
    }

    queue!(out, cursor::MoveTo(0, text_rows as u16))?;
    queue!(out, SetForegroundColor(Color::Black), SetBackgroundColor(Color::White))?;
    let status = if let Some(d) = diags.first() {
        format!(
            " {}{} — {}:{}: {}  [^S সেভ ^R চালাও ^Q প্রস্থান]",
            buf.path.display(),
            if buf.dirty { "*" } else { "" },
            d.line,
            d.col,
            d.message
        )
    } else {
        format!(
            " {}{} — ত্রুটি নেই  [^S সেভ ^R চালাও ^Q প্রস্থান]",
            buf.path.display(),
            if buf.dirty { "*" } else { "" }
        )
    };
    let shown: String = status.chars().take(cols).collect();
    let pad = " ".repeat(cols.saturating_sub(shown.chars().count()));
    queue!(out, Print(format!("{}{}", shown, pad)), ResetColor)?;

    let screen_row = buf.row.saturating_sub(start);
    let screen_col = gutter + buf.col;
    queue!(out, cursor::MoveTo(screen_col as u16, screen_row as u16))?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn insert_and_navigate() {
        let mut buf = Buffer::from_text("ab", PathBuf::from("t.ক"));
        buf.col = 1;
        buf.insert_char('X');
        assert_eq!(buf.line(0), "aXb");
        buf.move_left();
        buf.move_left();
        assert_eq!(buf.col, 0);
        buf.move_right();
        assert_eq!(buf.col, 1);
    }

    #[test]
    fn insert_handles_bengali_multibyte_correctly() {
        // Each Bengali codepoint is 3 UTF-8 bytes — a byte-offset bug here
        // would panic on a non-char-boundary insert, not just misplace text.
        let mut buf = Buffer::from_text("কলম", PathBuf::from("t.ক"));
        buf.col = 2; // between ল and ম
        buf.insert_char('X');
        assert_eq!(buf.line(0), "কলXম");
    }

    #[test]
    fn newline_splits_line_at_cursor() {
        let mut buf = Buffer::from_text("হ্যালো", PathBuf::from("t.ক"));
        buf.col = 3;
        buf.insert_newline();
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.row, 1);
        assert_eq!(buf.col, 0);
        assert_eq!(format!("{}\n{}", buf.line(0), buf.line(1)), buf.text());
    }

    #[test]
    fn backspace_at_line_start_joins_with_previous_line() {
        let mut buf = Buffer::from_text("এক\nদুই", PathBuf::from("t.ক"));
        buf.row = 1;
        buf.col = 0;
        buf.backspace();
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), "একদুই");
        assert_eq!(buf.col, 2); // codepoint count of "এক" (এ + ক), not byte count
    }

    #[test]
    fn backspace_at_document_start_is_a_no_op() {
        let mut buf = Buffer::from_text("ক", PathBuf::from("t.ক"));
        buf.backspace();
        assert_eq!(buf.line(0), "ক");
        assert_eq!(buf.row, 0);
        assert_eq!(buf.col, 0);
    }

    #[test]
    fn delete_forward_joins_next_line_at_end_of_line() {
        let mut buf = Buffer::from_text("এক\nদুই", PathBuf::from("t.ক"));
        buf.col = 3; // end of "এক"
        buf.delete_forward();
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), "একদুই");
    }

    #[test]
    fn apply_key_ctrl_s_saves_without_inserting_text() {
        let dir = std::env::temp_dir().join(format!("kolom-editor-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("save.ক");
        let mut buf = Buffer::from_text("অ্যাপ { }", path.clone());
        buf.dirty = true;
        let action = apply_key(&mut buf, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(action, Action::Save);
        assert_eq!(buf.line(0), "অ্যাপ { }", "Ctrl+S must not fall through to plain insert");
        buf.save().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "অ্যাপ { }");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_key_plain_char_inserts() {
        let mut buf = Buffer::from_text("", PathBuf::from("t.ক"));
        let action = apply_key(&mut buf, key('ক'));
        assert_eq!(action, Action::None);
        assert_eq!(buf.line(0), "ক");
    }

    #[test]
    fn apply_key_ctrl_q_requests_quit() {
        let mut buf = Buffer::from_text("", PathBuf::from("t.ক"));
        let action = apply_key(&mut buf, KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert_eq!(action, Action::Quit);
    }

    #[test]
    fn diagnostics_report_undeclared_variable() {
        let src = "অ্যাপ {\n    লেখো(অজানা)\n}\n";
        let diags = analyze_buffer(src, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("অজানা"));
    }

    #[test]
    fn diagnostics_empty_for_valid_program() {
        let src = "অ্যাপ {\n    লেখো(\"হ্যালো\")\n}\n";
        let diags = analyze_buffer(src, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn diagnostics_resolve_real_sibling_module() {
        // Same fixture kolom-lsp's own test suite uses for this — proves
        // this crate's build_program() does real module resolution too, not
        // just single-file analysis.
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/28_user_module/main.ক");
        let text = std::fs::read_to_string(&fixture).unwrap();
        let dir = fixture.parent().unwrap();
        let diags = analyze_buffer(&text, Some(dir));
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn build_program_succeeds_for_runnable_source() {
        let src = "অ্যাপ {\n    লেখো(\"হ্যালো\")\n}\n";
        assert!(build_program(src, None).is_ok());
    }
}
