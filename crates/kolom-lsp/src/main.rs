//! `kolom-lsp` — v1: diagnostics only.
//!
//! Runs the same lex → parse → resolve-imports → sema pipeline
//! `কলম চালাও`/`কলম বিল্ড` already run, and republishes whatever
//! `kolom_sema::analyze` finds as LSP diagnostics on open/change/save.
//! Nothing here is new compiler logic — this crate is a fourth consumer of
//! `kolom-lexer`/`kolom-syntax`/`kolom-sema`, translating their existing
//! positioned `Diagnostic` type into the LSP shape.
//!
//! Deliberately whole-file, no incremental re-analysis: `kolom_sema::Types`
//! is keyed by AST pointer identity (`compiler.md`), valid only against the
//! exact parse that produced it, so there is no cheaper "just this edit"
//! path today — the whole file is re-lexed/re-parsed/re-analyzed on every
//! open, change, and save. Fine for the small programs this language sees;
//! would need revisiting for large files edited at high frequency.

use lsp_server::{Connection, Message, Notification, Response};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, Position, PublishDiagnosticsParams, Range, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        ..Default::default()
    };
    // `Connection::initialize` runs the whole handshake itself — waits for
    // the client's `initialize` request, replies with our capabilities, and
    // consumes the `initialized` notification that follows. Its return
    // value is the client's `initialize` params, which v1 has no use for
    // (no workspace-root-dependent behavior yet), so it is discarded rather
    // than re-parsed into a typed struct for no reason.
    let _client_params = connection.initialize(serde_json::to_value(capabilities)?)?;

    // Moves `connection` in (rather than passing `&connection`) so it — and
    // the `Sender` it owns — is dropped the moment `main_loop` returns.
    // `io_threads.join()` waits for the writer thread to see the channel
    // close; a `connection` still alive in `main`'s own scope across that
    // join would hold the `Sender` open forever, since a shared reference
    // can't be dropped early — the server would never exit on shutdown.
    main_loop(connection)?;
    io_threads.join()?;
    Ok(())
}

fn main_loop(connection: Connection) -> Result<(), Box<dyn std::error::Error>> {
    // Last-known text per open document — so `textDocument/didSave`, which a
    // client may send with no text at all, can still re-analyze against
    // whatever `didChange` most recently reported, rather than needing the
    // save notification to carry content of its own.
    let mut docs: HashMap<Uri, String> = HashMap::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                // v1 declares no request-handled capabilities (hover,
                // completion, ...), so anything else is a method the client
                // should not have sent per our own `ServerCapabilities` —
                // answer with MethodNotFound rather than staying silent,
                // so a client bug surfaces instead of hanging.
                let resp = Response::new_err(
                    req.id,
                    lsp_server::ErrorCode::MethodNotFound as i32,
                    format!("'{}' সমর্থিত নয়", req.method),
                );
                connection.sender.send(Message::Response(resp))?;
            }
            Message::Notification(not) => {
                eprintln!("[kolom-lsp] notification: {}", not.method);
                handle_notification(&connection, &mut docs, not)?
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    docs: &mut HashMap<Uri, String>,
    not: Notification,
) -> Result<(), Box<dyn std::error::Error>> {
    match not.method.as_str() {
        "textDocument/didOpen" => {
            let p: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = p.text_document.uri;
            docs.insert(uri.clone(), p.text_document.text);
            publish(connection, docs, &uri)?;
        }
        "textDocument/didChange" => {
            let p: lsp_types::DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = p.text_document.uri;
            // FULL sync (declared in ServerCapabilities): the server never
            // asked for incremental deltas, so the last content change is
            // the entire new document text.
            if let Some(change) = p.content_changes.into_iter().last() {
                docs.insert(uri.clone(), change.text);
                publish(connection, docs, &uri)?;
            }
        }
        "textDocument/didSave" => {
            let p: lsp_types::DidSaveTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = p.text_document.uri;
            if let Some(text) = p.text {
                docs.insert(uri.clone(), text);
            }
            if docs.contains_key(&uri) {
                publish(connection, docs, &uri)?;
            }
        }
        "textDocument/didClose" => {
            let p: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
            docs.remove(&p.text_document.uri);
            // Clear diagnostics for a closed document rather than leaving
            // stale squiggles in a buffer the editor may reopen unmodified.
            let params = PublishDiagnosticsParams { uri: p.text_document.uri, diagnostics: vec![], version: None };
            connection.sender.send(Message::Notification(Notification::new(
                "textDocument/publishDiagnostics".to_string(),
                params,
            )))?;
        }
        _ => {}
    }
    Ok(())
}

fn publish(connection: &Connection, docs: &HashMap<Uri, String>, uri: &Uri) -> Result<(), Box<dyn std::error::Error>> {
    let text = docs.get(uri).map(String::as_str).unwrap_or("");
    let diagnostics = analyze(uri, text);
    let params = PublishDiagnosticsParams { uri: uri.clone(), diagnostics, version: None };
    connection
        .sender
        .send(Message::Notification(Notification::new("textDocument/publishDiagnostics".to_string(), params)))?;
    Ok(())
}

/// Runs the real pipeline and returns whatever it found, stopping at the
/// first stage that reports anything — a file with a lex error is not
/// worth parsing, and a file that fails to parse has no AST for sema to
/// check. Matches how `কলম চালাও` itself gates these stages (`kolom-cli`).
fn analyze(uri: &Uri, text: &str) -> Vec<Diagnostic> {
    let (tokens, lex_errs) = kolom_lexer::lex(text);
    if !lex_errs.is_empty() {
        return lex_errs.iter().map(to_lsp).collect();
    }

    let (mut prog, parse_errs) = kolom_syntax::parse(tokens);
    if !parse_errs.is_empty() {
        return parse_errs.iter().map(to_lsp).collect();
    }

    // Resolves sibling `ইম্পোর্ট helper`-style user modules against the
    // file's own directory, the same way `কলম চালাও`/`কলম বিল্ড` do
    // (`kolom_cli::resolve_user_modules`) — only possible when the document
    // has a real path (an unsaved, untitled buffer does not), in which case
    // user-module imports are left for sema to report as unresolved rather
    // than silently skipped.
    if let Some(dir) = uri_to_dir(uri) {
        if let Err(e) = kolom_cli::resolve_user_modules(&mut prog, &dir) {
            return vec![Diagnostic {
                range: zero_range(),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("kolom".to_string()),
                message: e,
                ..Default::default()
            }];
        }
    }

    kolom_sema::analyze(&prog).iter().map(to_lsp).collect()
}

/// Turns a `file://` URI into the directory containing it, for
/// `resolve_user_modules` to search for sibling `ইম্পোর্ট`-ed files in.
///
/// Two things this cannot skip, both found by actually opening a real
/// multi-file project through a real client rather than trusting the types
/// to be self-evidently correct:
///
/// - `Uri::path()` returns the URI's path component *as written on the
///   wire* — still percent-encoded (`EStr`, not `str`). Every real Kolom
///   file has a non-ASCII `.ক` extension, so a project living under a
///   Bengali-named directory (`examples/০১_হ্যালো/`, say) would search for
///   sibling modules using literal `%E0%A6...` bytes as if they were
///   directory names, and never find them. `.decode()` is required.
/// - On Windows, a `file:///E:/...` URI's path component is `/E:/...` — the
///   URI grammar's leading slash, not part of the real path. `std::path`
///   does not treat `/E:/foo` as `E:\foo`; it has to be stripped explicitly.
fn uri_to_dir(uri: &Uri) -> Option<std::path::PathBuf> {
    let decoded = uri.path().as_estr().decode().into_string_lossy();
    let mut s: &str = decoded.as_ref();
    if cfg!(windows) {
        let b = s.as_bytes();
        if b.first() == Some(&b'/') && b.get(1).is_some_and(u8::is_ascii_alphabetic) && b.get(2) == Some(&b':') {
            s = &s[1..];
        }
    }
    std::path::Path::new(s).parent().map(|p| p.to_path_buf())
}

/// `kolom_lexer::Diagnostic { line, col, message }` is 1-based in both
/// fields; LSP's `Position` is 0-based. `col` counts Unicode scalar values
/// (`compiler.md` §৩), which — Kolom source being Bengali/ASCII, both
/// entirely within the Basic Multilingual Plane — coincides with LSP's
/// UTF-16-code-unit convention for every character this language's own
/// keywords, identifiers, and literals can contain.
///
/// Kolom errors carry a single point, not a span, so the range collapses to
/// one character. `zero_range` is the fallback for the handful of
/// diagnostics that are program-wide (a module-resolution failure) rather
/// than tied to one token.
fn to_lsp(d: &kolom_lexer::Diagnostic) -> Diagnostic {
    let line = d.line.saturating_sub(1);
    let col = d.col.saturating_sub(1);
    Diagnostic {
        range: Range { start: Position { line, character: col }, end: Position { line, character: col + 1 } },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("kolom".to_string()),
        message: d.message.clone(),
        ..Default::default()
    }
}

fn zero_range() -> Range {
    Range { start: Position { line: 0, character: 0 }, end: Position { line: 0, character: 1 } }
}
