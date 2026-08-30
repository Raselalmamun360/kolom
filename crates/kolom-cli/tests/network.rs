//! The `নেটওয়ার্ক` module on the native (Cranelift) backend.
//!
//! The test stands up a real loopback echo server in a background thread,
//! then drives it from a natively-compiled Kolom program — a genuine TCP
//! round trip rather than a mock.
//!
//! Both the interpreter and the native binary are checked against the same
//! expected output, so the two cannot drift apart.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;

fn kolom_exe() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    dir.pop();
    dir.join(format!("kolom{}", std::env::consts::EXE_SUFFIX))
}

/// Binds an ephemeral port and echoes one connection's payload back with a
/// prefix. Returns the port so the Kolom program can be pointed at it.
///
/// Port 0 lets the OS choose a free port: a fixed port would make these
/// tests collide with each other and with anything else on the machine.
fn spawn_echo_server() -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("cannot bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            if let Ok(n) = sock.read(&mut buf) {
                let mut reply = b"echo: ".to_vec();
                reply.extend_from_slice(&buf[..n]);
                let _ = sock.write_all(&reply);
            }
            let _ = sock.shutdown(std::net::Shutdown::Both);
        }
    });
    (port, handle)
}

/// A Kolom client that connects, sends Bengali text, prints the reply.
fn client_source(port: u16) -> String {
    format!(
        r#"ইম্পোর্ট নেটওয়ার্ক

অ্যাপ {{
    ধরি স = নেটওয়ার্ক.কানেক্ট("127.0.0.1", {port})
    নেটওয়ার্ক.সেন্ড(স, "হ্যালো সার্ভার")
    ধরি উত্তর = নেটওয়ার্ক.রিসিভ(স, ১০২৪)
    লেখো(উত্তর)
    নেটওয়ার্ক.ক্লোজ(স)
}}
"#
    )
}

const EXPECTED: &str = "echo: হ্যালো সার্ভার\n";

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("kolom-network-test").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Confirms loopback TCP is usable at all, so a sandboxed environment
/// reports "skipped" rather than a misleading failure.
fn loopback_available() -> bool {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => {
            let port = l.local_addr().unwrap().port();
            std::thread::spawn(move || {
                let _ = l.accept();
            });
            TcpStream::connect(("127.0.0.1", port)).is_ok()
        }
        Err(_) => false,
    }
}

#[test]
fn network_module_interpreted() {
    if !loopback_available() {
        eprintln!("skip: loopback TCP unavailable in this environment");
        return;
    }
    let dir = workdir("interp");
    let (port, server) = spawn_echo_server();
    let src = dir.join("net.ক");
    std::fs::write(&src, client_source(port)).unwrap();

    let out = Command::new(kolom_exe())
        .arg("চালাও")
        .arg(&src)
        .output()
        .expect("failed to run kolom");
    let _ = server.join();

    assert!(
        out.status.success(),
        "কলম চালাও failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"), EXPECTED);
}

#[test]
fn network_module_native() {
    if !loopback_available() {
        eprintln!("skip: loopback TCP unavailable in this environment");
        return;
    }
    let dir = workdir("native");
    let src = dir.join("net.ক");

    // Compile first, then start the server: the build takes long enough
    // that a server waiting on accept() could otherwise time out.
    let (port, server) = spawn_echo_server();
    std::fs::write(&src, client_source(port)).unwrap();
    let build = Command::new(kolom_exe())
        .arg("বিল্ড")
        .arg(&src)
        .output()
        .expect("failed to run kolom");
    assert!(
        build.status.success(),
        "কলম বিল্ড failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let exe = String::from_utf8_lossy(&build.stdout).trim().to_string();

    let run = Command::new(&exe).output().expect("failed to run produced exe");
    let _ = server.join();

    assert!(
        run.status.success(),
        "produced exe exited {:?}:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"), EXPECTED);
}
