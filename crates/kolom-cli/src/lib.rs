//! Shared pieces of the Kolom CLI that tests also need.
//!
//! `resolve_user_modules` lives here rather than in `main.rs` because the
//! golden-test harness needs the *same* implementation. It previously had
//! its own copy, which silently missed two fixes (struct export and stdlib
//! import propagation) — exactly the drift this module exists to prevent.

use kolom_lexer::lex;

pub fn resolve_user_modules(
    prog: &mut kolom_syntax::ast::Program,
    main_file_dir: &std::path::Path,
) -> Result<(), String> {
    let mut resolved = std::collections::HashSet::new();
    let mut queue: Vec<String> = prog
        .imports
        .iter()
        .filter(|i| !kolom_sema::STDLIB_MODULES.contains(&i.name.as_str()))
        .map(|i| i.name.clone())
        .collect();

    while let Some(mod_name) = queue.pop() {
        if !resolved.insert(mod_name.clone()) {
            continue;
        }
        let mod_path = main_file_dir.join(format!("{}.ক", mod_name));
        let src = std::fs::read_to_string(&mod_path).map_err(|_| {
            format!(
                "মডিউল '{}' পাওয়া যায়নি — '{}' খোঁজা হয়েছে",
                mod_name,
                mod_path.display()
            )
        })?;
        let (tokens, lex_errs) = lex(&src);
        if !lex_errs.is_empty() {
            return Err(format!(
                "মডিউল '{}'-এ লেক্সিক্যাল ত্রুটি: {}",
                mod_name,
                lex_errs
                    .iter()
                    .map(|d| format!("{}:{}: {}", mod_path.display(), d.line, d.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        let (mut sub_prog, parse_errs) = kolom_syntax::parse(tokens);
        if !parse_errs.is_empty() {
            return Err(format!(
                "মডিউল '{}'-এ পার্স ত্রুটি: {}",
                mod_name,
                parse_errs
                    .iter()
                    .map(|d| format!("{}:{}: {}", mod_path.display(), d.line, d.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if sub_prog.app.is_some() {
            return Err(format!(
                "মডিউল '{}'-এ 'অ্যাপ' ডিক্লারেশন থাকতে পারে না",
                mod_name
            ));
        }
        for imp in &sub_prog.imports {
            if kolom_sema::STDLIB_MODULES.contains(&imp.name.as_str()) {
                // A module's own stdlib imports must reach the merged
                // program: its function bodies are about to live there, and
                // sema checks `গণিত.বর্গমূল` against the *merged* import
                // list. Dropping them made a module unable to use the
                // standard library at all.
                if !prog.imports.iter().any(|e| e.name == imp.name) {
                    prog.imports.push(imp.clone());
                }
            } else {
                queue.push(imp.name.clone());
            }
        }
        // `ডাটা` declarations are exported like functions and constants.
        // Without this a module could define a struct but nobody could name
        // its type, so `ফাংশন বানাও() -> বিন্দু` failed to resolve.
        for sd in sub_prog.structs.drain(..) {
            if let Some(existing) = prog.structs.iter().find(|e| e.name.name == sd.name.name) {
                return Err(format!(
                    "'{}' ডাটা দুইবার ঘোষিত — একবার মডিউল '{}'-এ, আরেকবার {}:{}-এ",
                    sd.name.name, mod_name, existing.name.pos.line, existing.name.pos.col
                ));
            }
            prog.structs.push(sd);
        }
        prog.funcs.extend(sub_prog.funcs.drain(..));
        prog.consts.extend(sub_prog.consts.drain(..));
    }
    prog.imports
        .retain(|i| kolom_sema::STDLIB_MODULES.contains(&i.name.as_str()));
    Ok(())
}
