//! Every Kolom example in the documentation must actually compile.
//!
//! This exists because the docs had drifted badly from the language twice
//! over. The last audit found nine broken programs in `tutorial.md` alone:
//! structs declared with a keyword that was never implemented (`গঠন` rather
//! than `তথ্য`), parameters written `name: type` when Kolom wants
//! `type name`, a map helper under a name that does not exist, an
//! `ইম্পোর্ট` inside `অ্যাপ`, a module function called with a prefix it
//! cannot have, `গণিত` float functions handed whole numbers, and two
//! Devanagari letters (`न`, `ि`) sitting inside Bengali words where they are
//! invisible to a reader but not to the lexer.
//!
//! Every one of those would have been caught the moment it was written.
//!
//! **Checks, but does not run.** Parsing and type-checking find this entire
//! class of error; executing would additionally write files, create
//! directories, and open a GUI window, which a test suite has no business
//! doing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One fenced ```kolom block, with the filename the prose gave it (from a
/// `**name.ক:**` line just above) if it had one.
struct Block {
    file: String,
    index: usize,
    name: Option<String>,
    code: String,
}

fn docs_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn doc_files() -> Vec<PathBuf> {
    let root = docs_root();
    let mut out = vec![root.join("README.md")];
    let mut docs: Vec<PathBuf> = std::fs::read_dir(root.join("docs"))
        .expect("docs/ should exist")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .collect();
    docs.sort();
    out.extend(docs);
    out
}

/// Pulls every ```kolom fence out of one markdown file.
fn blocks_in(path: &Path) -> Vec<Block> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let label = path.file_name().unwrap().to_string_lossy().into_owned();

    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    // The most recent `**something.ক:**` heading, which names the block that
    // follows it — that is how the multi-file example introduces its module.
    let mut pending_name: Option<String> = None;
    let mut index = 0;

    while let Some(line) = lines.next() {
        let t = line.trim();
        if t.starts_with("**") && t.contains(".ক") && t.ends_with("**") {
            pending_name = t
                .trim_matches('*')
                .trim_end_matches(':')
                .trim()
                .strip_suffix(".ক")
                .map(|s| s.to_string());
            continue;
        }
        if t == "```kolom" {
            index += 1;
            let mut code = String::new();
            for l in lines.by_ref() {
                if l.trim() == "```" {
                    break;
                }
                code.push_str(l);
                code.push('\n');
            }
            out.push(Block { file: label.clone(), index, name: pending_name.take(), code });
        } else if !t.is_empty() {
            pending_name = None;
        }
    }
    out
}

/// Parses and type-checks `code`, returning every complaint as text.
fn check(code: &str, dir: &Path) -> Vec<String> {
    let (tokens, lex_errs) = kolom_lexer::lex(code);
    if !lex_errs.is_empty() {
        return lex_errs.iter().map(|d| format!("{}:{} {}", d.line, d.col, d.message)).collect();
    }
    let (mut prog, parse_errs) = kolom_syntax::parse(tokens);
    if !parse_errs.is_empty() {
        return parse_errs.iter().map(|d| format!("{}:{} {}", d.line, d.col, d.message)).collect();
    }
    // Resolves `ইম্পোর্ট helper` against the sibling blocks written to `dir`,
    // through the same code path `কলম চালাও` uses.
    if let Err(e) = kolom_cli::resolve_user_modules(&mut prog, dir) {
        return vec![e];
    }
    kolom_sema::analyze(&prog)
        .iter()
        .map(|d| format!("{}:{} {}", d.line, d.col, d.message))
        .collect()
}

#[test]
fn every_documented_example_compiles() {
    let dir = std::env::temp_dir().join("kolom-docs-examples");
    let _ = std::fs::remove_dir_all(&dir);

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for path in doc_files() {
        let blocks = blocks_in(&path);

        // Blocks the prose named after a file become real files first, so a
        // program block that imports one can find it.
        let scratch = dir.join(path.file_stem().unwrap());
        std::fs::create_dir_all(&scratch).unwrap();
        let named: HashMap<&str, &Block> =
            blocks.iter().filter_map(|b| b.name.as_deref().map(|n| (n, b))).collect();
        for (name, b) in &named {
            std::fs::write(scratch.join(format!("{name}.ক")), &b.code).unwrap();
        }

        for b in &blocks {
            // A fragment illustrating one construct is not a program. `অ্যাপ`
            // is what makes a block runnable, so that is the entry point —
            // and a block the prose named is checked through whoever imports
            // it, not on its own.
            if !b.code.contains("অ্যাপ") {
                continue;
            }
            checked += 1;
            let errs = check(&b.code, &scratch);
            if !errs.is_empty() {
                failures.push(format!(
                    "{} — ```kolom block #{}:\n      {}",
                    b.file,
                    b.index,
                    errs.join("\n      ")
                ));
            }
        }
    }

    assert!(checked > 20, "only {checked} example programs found — the extractor is probably broken");
    assert!(
        failures.is_empty(),
        "{} of {checked} documented examples do not compile:\n\n{}\n",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Bengali and Devanagari share shapes for several letters, so a stray
/// Devanagari codepoint inside a Bengali word is invisible when read and
/// fatal when compiled. Two had been sitting in the tutorial.
///
/// This checks the prose too, not only code: a Devanagari letter in a
/// sentence is a typo whether or not a compiler ever sees it.
#[test]
fn docs_contain_no_devanagari() {
    let mut hits = Vec::new();
    for path in doc_files() {
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let bad: Vec<char> = line
                .chars()
                .filter(|c| ('\u{0900}'..='\u{097F}').contains(c))
                // `।` and `॥` sit in the Devanagari block but are the normal
                // Bengali full stop — shared punctuation, not a stray letter.
                .filter(|c| !matches!(c, '\u{0964}' | '\u{0965}'))
                .collect();
            if !bad.is_empty() {
                hits.push(format!(
                    "{}:{} contains Devanagari {:?} — should these be Bengali?\n      {}",
                    path.file_name().unwrap().to_string_lossy(),
                    n + 1,
                    bad,
                    line.trim()
                ));
            }
        }
    }
    assert!(hits.is_empty(), "Devanagari found in documentation:\n\n{}\n", hits.join("\n"));
}

/// A keyword landing in `kolom_lexer::KEYWORDS` without also landing in the
/// reserved-keyword lists (`language.md` §১৬, `grammer.md` §৫.১) leaves a
/// program author with no way to discover it's reserved short of hitting a
/// confusing parse error — and it happened for real: এনাম/মিলাও/এক্সটার্ন all
/// shipped in the lexer while those two sections stayed exactly as they were
/// before those features existed. A substring check (not scoped to those
/// specific sections — locating them by header text is more brittle than
/// this is worth) is weak for very short keywords (`না` can appear inside
/// unrelated words), but it is exactly the check that would have caught the
/// three that actually went missing, and costs nothing to keep running.
#[test]
fn every_lexer_keyword_appears_in_reserved_keyword_docs() {
    let language_md = std::fs::read_to_string(docs_root().join("docs").join("language.md")).unwrap();
    let grammer_md = std::fs::read_to_string(docs_root().join("docs").join("grammer.md")).unwrap();

    let mut missing = Vec::new();
    for kw in kolom_lexer::KEYWORDS {
        let in_language = language_md.contains(kw);
        let in_grammer = grammer_md.contains(kw);
        if !in_language || !in_grammer {
            missing.push(format!(
                "'{kw}' — language.md: {}, grammer.md: {}",
                if in_language { "আছে" } else { "নেই" },
                if in_grammer { "আছে" } else { "নেই" },
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "keyword(s) missing from the reserved-keyword docs:\n{}\n",
        missing.join("\n")
    );
}
