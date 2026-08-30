//! End-to-end regression tests for `kolom-lsp`: spawns the real built
//! binary and speaks actual `Content-Length`-framed JSON-RPC over its
//! stdio, exactly as an editor would — not just unit-testing `analyze()`
//! in isolation. Rust port of the ad-hoc Node.js scripts used to bring
//! the server up originally, so this is exercised by `cargo test` rather
//! than living only as scratch scripts.

use serde_json::{json, Value};
use std::io::{BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

struct Client {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    next_id: i64,
}

impl Client {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kolom-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn kolom-lsp");
        let stdin = child.stdin.take().unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        Client { child, stdin, reader, next_id: 1 }
    }

    fn send(&mut self, msg: &Value) {
        let body = serde_json::to_vec(msg).unwrap();
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.stdin.write_all(&body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Reads exactly one `Content-Length`-framed message off stdout.
    fn read_message(&mut self) -> Value {
        let mut header = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            self.reader.read_exact(&mut byte).expect("read header byte");
            header.push(byte[0]);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let header_str = String::from_utf8_lossy(&header);
        let len: usize = header_str
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .expect("Content-Length header")
            .trim()
            .parse()
            .unwrap();
        let mut body = vec![0u8; len];
        self.reader.read_exact(&mut body).expect("read body");
        serde_json::from_slice(&body).expect("parse json")
    }

    /// Reads messages until a `textDocument/publishDiagnostics` notification
    /// for `uri` shows up, ignoring anything else (responses to other
    /// requests, unrelated notifications) in between.
    fn wait_for_diagnostics(&mut self, uri: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "timed out waiting for diagnostics for {uri}");
            let msg = self.read_message();
            if msg.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
                && msg["params"]["uri"] == uri
            {
                return msg["params"]["diagnostics"].clone();
            }
        }
    }

    fn initialize(&mut self) {
        let id = self.request("initialize", json!({ "processId": null, "rootUri": null, "capabilities": {} }));
        loop {
            let msg = self.read_message();
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                break;
            }
        }
        self.notify("initialized", json!({}));
    }

    fn shutdown(mut self) {
        let id = self.request("shutdown", Value::Null);
        loop {
            let msg = self.read_message();
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                break;
            }
        }
        self.notify("exit", Value::Null);
        let _ = self.child.wait();
    }
}

/// Percent-encodes a filesystem path into a `file://` URI the way a real
/// editor does: non-ASCII bytes (every Kolom source file has a non-ASCII
/// `.ক` extension) get `%XX`-encoded, `/` and the drive-letter `:` do not.
fn file_uri(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[test]
fn undeclared_variable_reported_and_cleared_on_fix() {
    let mut c = Client::spawn();
    c.initialize();

    let uri = "file:///C:/kolom-lsp-test/bad.k";
    let bad_src = "অ্যাপ {\n    ধরি ক = ১\n    লেখো(অজানা_ভ্যারিয়েবল)\n}\n";
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "kolom", "version": 1, "text": bad_src } }),
    );
    let diags = c.wait_for_diagnostics(uri);
    let arr = diags.as_array().unwrap();
    assert_eq!(arr.len(), 1, "expected exactly one diagnostic, got {diags:?}");
    assert!(arr[0]["message"].as_str().unwrap().contains("অজানা_ভ্যারিয়েবল"));

    // didChange (full sync) to valid content clears the error.
    let good_src = "অ্যাপ {\n    ধরি ক = ১\n    লেখো(ক)\n}\n";
    c.notify(
        "textDocument/didChange",
        json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": good_src }] }),
    );
    let diags = c.wait_for_diagnostics(uri);
    assert_eq!(diags.as_array().unwrap().len(), 0, "expected diagnostics cleared, got {diags:?}");

    // didClose republishes empty diagnostics too.
    c.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }));
    let diags = c.wait_for_diagnostics(uri);
    assert_eq!(diags.as_array().unwrap().len(), 0);

    c.shutdown();
}

#[test]
fn valid_program_has_no_diagnostics() {
    let mut c = Client::spawn();
    c.initialize();

    let uri = "file:///C:/kolom-lsp-test/good.k";
    let src = "অ্যাপ {\n    লেখো(\"হ্যালো\")\n}\n";
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "kolom", "version": 1, "text": src } }),
    );
    let diags = c.wait_for_diagnostics(uri);
    assert_eq!(diags.as_array().unwrap().len(), 0, "expected no diagnostics, got {diags:?}");

    c.shutdown();
}

/// Real project, real Bengali-extension filename, real Windows drive-letter
/// path — the exact combination that exposed both the percent-decoding bug
/// and the `/E:/...`-leading-slash bug in `uri_to_dir`. Regression coverage
/// for both fixes together.
#[test]
fn module_resolution_against_real_windows_path() {
    // `std::fs::canonicalize` on Windows returns a `\\?\`-prefixed
    // extended-length path — never what a real editor sends in a `file://`
    // URI — so build the URI from the plain absolute path instead.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kolom-cli/tests/golden/28_user_module/main.ক");
    assert!(fixture.exists(), "golden fixture must exist: {fixture:?}");
    let uri = file_uri(&fixture);
    let src = std::fs::read_to_string(&fixture).unwrap();

    let mut c = Client::spawn();
    c.initialize();
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "kolom", "version": 1, "text": src } }),
    );
    let diags = c.wait_for_diagnostics(&uri);
    assert_eq!(
        diags.as_array().unwrap().len(),
        0,
        "expected sibling module `helper` to resolve with no diagnostics, got {diags:?}"
    );

    c.shutdown();
}
