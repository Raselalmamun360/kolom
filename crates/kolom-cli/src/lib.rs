//! Shared pieces of the Kolom CLI that tests also need.
//!
//! `resolve_user_modules` lives here rather than in `main.rs` because the
//! golden-test harness needs the *same* implementation. It previously had
//! its own copy, which silently missed two fixes (struct export and stdlib
//! import propagation) — exactly the drift this module exists to prevent.

use kolom_lexer::lex;

struct QueueItem {
    /// The import name as written at the use site (`ইম্পোর্ট <this>`).
    mod_name: String,
    /// Directory this module's *own* sibling-file imports resolve against —
    /// the project root for a top-level file, or a package's own directory
    /// once resolution has entered its subtree.
    base_dir: std::path::PathBuf,
    /// `None` for the project's own code (flat merge, unqualified names,
    /// unchanged from before packages existed). `Some(package_name)` for
    /// anything reached while resolving a package's dependency tree — every
    /// file in that tree, however many sibling-file hops deep, merges under
    /// the *same* top-level package name, so a package's own internal
    /// helper files don't leak into the global namespace as bare names.
    namespace: Option<String>,
}

/// Resolves everything `prog.imports` needs beyond the standard library:
/// same-directory sibling `.ক` files (as before) and, new, KPM packages —
/// git dependencies fetched by `kolom install` into `কলম_প্যাকেজ/<নাম>/`
/// (see `kolom_pkg`). A name is checked as a package first (its own
/// directory under `কলম_প্যাকেজ/` relative to `main_file_dir`, which is
/// always the project root — packages are fetched flat, not nested, even
/// for transitive dependencies), then falls back to a sibling file.
///
/// Package functions are merged in under a mangled `"প্যাকেজ::ফাংশন"` key
/// (see `Types` in each backend for where that gets un-mangled back into a
/// real call) — collision-safe by construction, unlike the flat merge
/// sibling files still get. Package-declared `তথ্য`/`ধ্রুবক` are **not**
/// yet namespaced (still flat) — Kolom's type-expression syntax has no
/// qualified form (`প্যাকেজ.বিন্দু`) to reference a namespaced struct type
/// with, so namespacing them usefully needs that parser/sema work too. A
/// package sticking to function exports (the common case) is unaffected;
/// one that also declares structs/consts can still collide on those, same
/// as sibling files always could — a known, deliberate v1 gap.
pub fn resolve_user_modules(
    prog: &mut kolom_syntax::ast::Program,
    main_file_dir: &std::path::Path,
) -> Result<(), String> {
    let packages_root = kolom_pkg::packages_dir(main_file_dir);
    let mut resolved: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: Vec<QueueItem> = prog
        .imports
        .iter()
        .filter(|i| !kolom_sema::STDLIB_MODULES.contains(&i.name.as_str()))
        .map(|i| QueueItem { mod_name: i.name.clone(), base_dir: main_file_dir.to_path_buf(), namespace: None })
        .collect();

    while let Some(QueueItem { mod_name, base_dir, namespace }) = queue.pop() {
        // A package always claims its name globally (fetched flat into
        // `কলম_প্যাকেজ/<নাম>/`, never nested per-dependent) — checked before
        // the sibling-file fallback so a package can never be shadowed by a
        // same-named file sitting next to the importer.
        let as_package = packages_root.join(&mod_name).join(format!("{}.ক", mod_name));
        let (mod_path, effective_namespace, child_base_dir, child_namespace) = if as_package.exists() {
            (as_package, Some(mod_name.clone()), packages_root.join(&mod_name), Some(mod_name.clone()))
        } else {
            (base_dir.join(format!("{}.ক", mod_name)), namespace.clone(), base_dir.clone(), namespace.clone())
        };

        let dedup_key = mod_path.to_string_lossy().into_owned();
        if !resolved.insert(dedup_key) {
            continue;
        }

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
                queue.push(QueueItem {
                    mod_name: imp.name.clone(),
                    base_dir: child_base_dir.clone(),
                    namespace: child_namespace.clone(),
                });
            }
        }
        // `তথ্য` declarations are exported like functions and constants.
        // Without this a module could define a struct but nobody could name
        // its type, so `ফাংশন বানাও() -> বিন্দু` failed to resolve. Left
        // flat even inside a package's namespace — see the doc comment.
        for sd in sub_prog.structs.drain(..) {
            if let Some(existing) = prog.structs.iter().find(|e| e.name.name == sd.name.name) {
                return Err(format!(
                    "'{}' তথ্য দুইবার ঘোষিত — একবার মডিউল '{}'-এ, আরেকবার {}:{}-এ",
                    sd.name.name, mod_name, existing.name.pos.line, existing.name.pos.col
                ));
            }
            prog.structs.push(sd);
        }
        match &effective_namespace {
            None => {
                prog.funcs.extend(sub_prog.funcs.drain(..));
            }
            Some(ns) => {
                for f in sub_prog.funcs.drain(..) {
                    // `FuncDecl` sits behind an `Rc` (shared with the
                    // interpreter/codegen, which clone it freely) — no
                    // in-place rename, so unwrap-or-clone to get an owned
                    // copy, rename it, and re-wrap. This `Rc` was just
                    // created by parsing `sub_prog` with nothing else
                    // referencing it yet, so `try_unwrap` always succeeds in
                    // practice; the `unwrap_or_else` fallback is just for
                    // safety, not the expected path.
                    let mut owned = std::rc::Rc::try_unwrap(f).unwrap_or_else(|rc| (*rc).clone());
                    owned.name.name = format!("{}::{}", ns, owned.name.name);
                    prog.funcs.push(std::rc::Rc::new(owned));
                }
            }
        }
        prog.consts.extend(sub_prog.consts.drain(..));
    }
    // The user's own imports stay in the list even though their declarations
    // have just been merged in. Nothing downstream is confused by them — sema
    // and the codegen backends both select on STDLIB_MODULES — and keeping
    // them is what lets sema tell `helper.foo()` (a real module, used with a
    // prefix it does not have) apart from a module that was never imported
    // at all, and (new) what lets `call_stdlib`'s package fallback recognize
    // `প্যাকেজ.ফাংশন()` as intentional rather than a typo'd stdlib call.
    Ok(())
}
