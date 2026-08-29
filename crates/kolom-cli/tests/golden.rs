use std::fs;
use std::path::Path;

fn run_src(src: &str, base_dir: Option<&Path>) -> String {
    let mut out: Vec<u8> = Vec::new();

    let (tokens, lex_errs) = kolom_lexer::lex(src);
    if !lex_errs.is_empty() {
        return blocks(
            lex_errs
                .iter()
                .map(|d| kolom_lexer::format_error("ত্রুটি", "main.ক", d.line, d.col, &d.message))
                .collect(),
        );
    }

    let (mut prog, parse_errs) = kolom_syntax::parse(tokens);
    if !parse_errs.is_empty() {
        return blocks(
            parse_errs
                .iter()
                .map(|d| kolom_lexer::format_error("ত্রুটি", "main.ক", d.line, d.col, &d.message))
                .collect(),
        );
    }

    // Uses the CLI's own module resolution rather than a private copy:
    // an earlier duplicate here silently missed struct export and stdlib
    // import propagation, so the harness disagreed with the real compiler.
    if let Some(dir) = base_dir {
        if let Err(e) = kolom_cli::resolve_user_modules(&mut prog, dir) {
            return blocks(vec![format!("ত্রুটি: {}", e)]);
        }
    }

    let sema_errs = kolom_sema::analyze(&prog);
    if !sema_errs.is_empty() {
        return blocks(
            sema_errs
                .iter()
                .map(|d| kolom_lexer::format_error("ত্রুটি", "main.ক", d.line, d.col, &d.message))
                .collect(),
        );
    }

    match kolom_interp::run(&prog, &mut out) {
        Ok(()) => String::from_utf8_lossy(&out).into_owned(),
        Err(e) => format!(
            "{}\n",
            kolom_lexer::format_error("রানটাইম ত্রুটি", "main.ক", e.line, e.col, &e.message)
        ),
    }
}

fn blocks(v: Vec<String>) -> String {
    if v.is_empty() {
        return String::new();
    }
    let mut s = v.join("\n\n");
    s.push('\n');
    s
}

#[test]
fn golden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden");
    let mut failures: Vec<String> = Vec::new();
    let mut count = 0usize;

    let mut dirs: Vec<_> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("golden dir missing: {} ({})", root.display(), e))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let orig_cwd = std::env::current_dir().unwrap();
    let workroot = std::env::temp_dir().join(format!("kolom-golden-cwd-{}", std::process::id()));
    let _ = fs::create_dir_all(&workroot);

    for dir in dirs {
        count += 1;
        let src = fs::read_to_string(dir.join("main.ক"))
            .unwrap_or_else(|e| panic!("{}: missing main.ক ({})", dir.display(), e));

        let work = workroot.join(dir.file_name().unwrap().to_string_lossy().to_string());
        let _ = fs::create_dir_all(&work);
        std::env::set_current_dir(&work).unwrap();

        let got = run_src(&src, Some(&dir));

        std::env::set_current_dir(&orig_cwd).unwrap();

        let expected_path = dir.join("expected.txt");
        if !expected_path.exists() {
            fs::write(&expected_path, &got)
                .unwrap_or_else(|e| panic!("{}: cannot write expected.txt ({})", expected_path.display(), e));
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap();
        // Compare on normalized newlines. `.gitattributes` pins these files
        // to LF, but a clone made before that landed — or with a local
        // core.autocrlf — still yields CRLF, and the resulting failure
        // ("expected 5, got 5") gives no hint of the real cause.
        let norm = |t: &str| t.replace("\r\n", "\n");
        if norm(&got) != norm(&expected) {
            failures.push(format!(
                "=== {}\n--- expected ---\n{}--- got ---\n{}",
                dir.display(),
                expected,
                got
            ));
        }
    }

    let _ = fs::remove_dir_all(&workroot);
    assert!(count > 0, "no golden fixtures found");
    assert!(
        failures.is_empty(),
        "{}/{} golden tests failed:\n\n{}",
        failures.len(),
        count,
        failures.join("\n\n")
    );
}
