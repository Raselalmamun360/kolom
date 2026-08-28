use std::process::Command;

fn find_cc() -> Option<String> {
    if let Ok(cc) = std::env::var("KLOM_CC") {
        if !cc.trim().is_empty() {
            return Some(cc);
        }
    }
    for c in ["gcc", "clang", "cc", "cl"] {
        let probe = if cfg!(windows) {
            Command::new("where").arg(c).output()
        } else {
            Command::new("which").arg(c).output()
        };
        if let Ok(out) = probe {
            if out.status.success() {
                if let Some(first) = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .map(|s| s.trim().to_string())
                {
                    if !first.is_empty() {
                        return Some(first);
                    }
                }
            }
        }
    }
    None
}

#[test]
fn native_codegen_parity() {
    let cc = match find_cc() {
        Some(c) => c,
        None => {
            eprintln!("skip: no C compiler found (gcc/clang/cc/cl, or set KLOM_CC)");
            return;
        }
    };

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden");
    let build_dir = std::env::temp_dir().join(format!("kolom-native-test-{}", std::process::id()));
    std::fs::create_dir_all(&build_dir).unwrap();

    let names = [
        "01_hello",
        "02_variables",
        "03_functions",
        "04_control_flow",
        "05_strings",
        "06_escapes",
        "07_arrays",
        "08_const_float",
        "13_ok_copy",
        "19_ok_for_each",
        "20_ok_joth",
        "21_stdlib_math",
        "22_stdlib_string",
        "23_stdlib_io",
        "24_stdlib_random",
        "25_stdlib_fs",
        "26_stdlib_json",
        "28_user_module",
    ];

    let mut failures: Vec<String> = Vec::new();

    let orig_cwd = std::env::current_dir().unwrap();
    let workroot =
        std::env::temp_dir().join(format!("kolom-native-cwd-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&workroot);

    for name in &names {
        let case_dir = root.join(name);
        let src_path = case_dir.join("main.ক");
        let exp_path = case_dir.join("expected.txt");
        let src = std::fs::read_to_string(&src_path)
            .unwrap_or_else(|e| panic!("{}: {}", src_path.display(), e));
        let expected = std::fs::read_to_string(&exp_path)
            .unwrap_or_else(|e| panic!("{}: {}", exp_path.display(), e));

        let (tokens, lex_errs) = kolom_lexer::lex(&src);
        assert!(lex_errs.is_empty(), "{}: lex errors", name);
        let (mut prog, parse_errs) = kolom_syntax::parse(tokens);
        assert!(parse_errs.is_empty(), "{}: parse errors", name);

        for imp in prog.imports.clone() {
            if kolom_sema::STDLIB_MODULES.contains(&imp.name.as_str()) {
                continue;
            }
            let mod_path = case_dir.join(format!("{}.ক", imp.name));
            if let Ok(mod_src) = std::fs::read_to_string(&mod_path) {
                let (mt, me) = kolom_lexer::lex(&mod_src);
                if me.is_empty() {
                    let (mp, pe) = kolom_syntax::parse(mt);
                    if pe.is_empty() {
                        prog.funcs.extend(mp.funcs);
                        prog.consts.extend(mp.consts);
                    }
                }
            }
        }
        prog.imports
            .retain(|i| kolom_sema::STDLIB_MODULES.contains(&i.name.as_str()));
        let sema_errs = kolom_sema::analyze(&prog);
        assert!(sema_errs.is_empty(), "{}: sema errors", name);

        let c_code = kolom_codegen::emit(&prog, name, "windows");
        let c_path = build_dir.join(format!("{}.c", name));
        std::fs::write(&c_path, c_code.as_bytes()).unwrap();

        let exe_path = build_dir.join(format!("{}{}", name, if cfg!(windows) { ".exe" } else { "" }));

        let work = workroot.join(name);
        let _ = std::fs::create_dir_all(&work);
        std::env::set_current_dir(&work).unwrap();
        let out = Command::new(&cc)
            .arg("-O2")
            .arg(&c_path)
            .arg("-o")
            .arg(&exe_path)
            .output()
            .unwrap_or_else(|e| panic!("{}: cannot run {}: {}", name, cc, e));
        assert!(
            out.status.success(),
            "{}: C compilation failed:\n{}",
            name,
            String::from_utf8_lossy(&out.stderr)
        );

        let run = Command::new(&exe_path).output().unwrap();
        std::env::set_current_dir(&orig_cwd).unwrap();
        let got = String::from_utf8_lossy(&run.stdout).trim_end().to_string();
        let want = expected.trim_end().to_string();
        if got != want {
            failures.push(format!(
                "=== {}\n--- expected bytes ---\n{:?}\n--- got bytes ---\n{:?}",
                name, want, got
            ));
        }
    }

    std::env::set_current_dir(&orig_cwd).unwrap();
    let _ = std::fs::remove_dir_all(&workroot);
    let _ = std::fs::remove_dir_all(&build_dir);

    assert!(
        failures.is_empty(),
        "{}/{} native parity tests failed:\n\n{}",
        failures.len(),
        names.len(),
        failures.join("\n\n")
    );
}

#[test]
fn native_ui_static_smoke() {
    let cc = match find_cc() {
        Some(c) => c,
        None => {
            eprintln!("skip: no C compiler found");
            return;
        }
    };

    let ui_src_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ui")
        .join("main.ক");
    if !ui_src_path.exists() {
        eprintln!("skip: no UI fixture");
        return;
    }
    let src = std::fs::read_to_string(&ui_src_path).unwrap();

    let (tokens, lex_errs) = kolom_lexer::lex(&src);
    assert!(lex_errs.is_empty(), "UI fixture: lex errors: {:?}", lex_errs);
    let (prog, parse_errs) = kolom_syntax::parse(tokens);
    assert!(parse_errs.is_empty(), "UI fixture: parse errors: {:?}", parse_errs);
    let sema_errs = kolom_sema::analyze(&prog);
    assert!(sema_errs.is_empty(), "UI fixture: sema errors: {:?}", sema_errs);

    let c_code = kolom_codegen::emit(&prog, "কলম", "windows");
    assert!(c_code.contains("kl_ui_init"), "UI runtime missing from output");

    let build_dir = std::env::temp_dir().join(format!("kolom-ui-test-{}", std::process::id()));
    std::fs::create_dir_all(&build_dir).unwrap();
    let c_path = build_dir.join("ui.c");
    std::fs::write(&c_path, c_code.as_bytes()).unwrap();
    let exe_path = build_dir.join(format!("kolom-ui{}", if cfg!(windows) { ".exe" } else { "" }));

    let mut cmd = Command::new(&cc);
    cmd.arg("-O2").arg(&c_path).arg("-o").arg(&exe_path);
    if cfg!(windows) {
        cmd.args(["-lgdi32", "-luser32", "-lws2_32"]);
    }
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {}: {}", cc, e));
    assert!(
        out.status.success(),
        "UI program failed to compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut run = Command::new(&exe_path);
    run.env("KLOM_UI_AUTOCLOSE_MS", "500");
    let res = run
        .output()
        .unwrap_or_else(|e| panic!("cannot run UI exe: {}", e));
    let _ = std::fs::remove_dir_all(&build_dir);
    assert!(
        res.status.success(),
        "UI program crashed (exit {:?}):\n{}",
        res.status.code(),
        String::from_utf8_lossy(&res.stderr)
    );
}

#[test]
fn native_ui_dynamic_counter() {
    let cc = match find_cc() {
        Some(c) => c,
        None => {
            eprintln!("skip: no C compiler found");
            return;
        }
    };

    let src_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ui")
        .join("dynamic")
        .join("main.ক");
    let src = match std::fs::read_to_string(&src_path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("skip: no dynamic UI fixture");
            return;
        }
    };

    let (tokens, lex_errs) = kolom_lexer::lex(&src);
    assert!(lex_errs.is_empty());
    let (prog, parse_errs) = kolom_syntax::parse(tokens);
    assert!(parse_errs.is_empty(), "{:?}", parse_errs);
    let sema_errs = kolom_sema::analyze(&prog);
    assert!(sema_errs.is_empty(), "{:?}", sema_errs);

    let c_code = kolom_codegen::emit(&prog, "গণনা", "windows");
    assert!(c_code.contains("kl_build_ui"), "rebuild fn missing");
    assert!(c_code.contains("kl_app_rebuild"), "rebuild hook missing");

    let build_dir =
        std::env::temp_dir().join(format!("kolom-dyn-test-{}", std::process::id()));
    std::fs::create_dir_all(&build_dir).unwrap();
    let c_path = build_dir.join("dyn.c");
    std::fs::write(&c_path, c_code.as_bytes()).unwrap();
    let exe_path = build_dir.join(format!("dyn{}", if cfg!(windows) { ".exe" } else { "" }));

    let mut cmd = Command::new(&cc);
    cmd.arg("-O2").arg(&c_path).arg("-o").arg(&exe_path);
    if cfg!(windows) {
        cmd.args(["-lgdi32", "-luser32", "-lws2_32"]);
    }
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {}: {}", cc, e));
    assert!(
        out.status.success(),
        "dynamic UI failed to compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut run = Command::new(&exe_path);
    run.env("KLOM_UI_AUTOCLOSE_MS", "1500");
    run.env("KLOM_UI_SCRIPT_CLICKS", "0,0,1");
    let res = run.output().unwrap_or_else(|e| panic!("cannot run: {}", e));
    let _ = std::fs::remove_dir_all(&build_dir);
    assert!(
        res.status.success(),
        "dynamic UI crashed (exit {:?}):\n{}",
        res.status.code(),
        String::from_utf8_lossy(&res.stderr)
    );

    let stdout = String::from_utf8_lossy(&res.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines,
        vec!["1", "2", "1"],
        "click script 0,0,1 should print 1, 2, then 1"
    );
}

#[test]
fn native_graphics_canvas_animation() {
    let cc = match find_cc() {
        Some(c) => c,
        None => {
            eprintln!("skip: no C compiler found");
            return;
        }
    };

    let src = "ইম্পোর্ট গ্রাফিক্স\n\nফাংশন প্রতি_টিক() -> ফাঁকা {\n    লেখো(\"টিক\")\n\n}\n\nঅ্যাপ গ্রাফ {\n\n    ডিসপ্লে {\n        ক্যানভাস(220, 140)\n        ছবি(\"kl_test.bmp\")\n    }\n\n    গ্রাফিক্স.রঙ(30, 90, 200)\n    গ্রাফিক্স.ভরাট_আয়ত(10, 10, 120, 60)\n    গ্রাফিক্স.রঙ(250, 210, 40)\n    গ্রাফিক্স.বৃত্ত(110, 70, 45)\n    গ্রাফিক্স.লেখা(12, 20, \"ক্যানভাস!\")\n    গ্রাফিক্স.টিক(150, প্রতি_টিক)\n\n}\n";

    // tiny 8x8 24-bit BMP written next to the exe
    let mut bmp: Vec<u8> = Vec::new();
    let (w, h) = (8i32, 8i32);
    let pad = (4 - (w * 3) % 4) % 4;
    let data_size = (w * 3 + pad) * h;
    let file_size = 54 + data_size;
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&w.to_le_bytes());
    bmp.extend_from_slice(&h.to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(data_size as u32).to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    for _ in 0..data_size {
        bmp.push(180);
    }

    let build_dir =
        std::env::temp_dir().join(format!("kolom-gfx-test-{}", std::process::id()));
    std::fs::create_dir_all(&build_dir).unwrap();
    std::fs::write(build_dir.join("kl_test.bmp"), &bmp).unwrap();

    let (tokens, lex_errs) = kolom_lexer::lex(src);
    assert!(lex_errs.is_empty(), "{:?}", lex_errs);
    let (prog, parse_errs) = kolom_syntax::parse(tokens);
    assert!(parse_errs.is_empty(), "{:?}", parse_errs);
    let sema_errs = kolom_sema::analyze(&prog);
    assert!(sema_errs.is_empty(), "{:?}", sema_errs);

    let c_code = kolom_codegen::emit(&prog, "গ্রাফ", "windows");
    assert!(c_code.contains("kl_ui_canvas"), "canvas widget missing");
    assert!(c_code.contains("kl_ui_image"), "image widget missing");
    assert!(c_code.contains("kl_g_fillrect"), "draw cmd missing");

    let c_path = build_dir.join("gfx.c");
    std::fs::write(&c_path, c_code.as_bytes()).unwrap();
    let exe_path = build_dir.join(format!("gfx{}", if cfg!(windows) { ".exe" } else { "" }));

    let mut cmd = Command::new(&cc);
    cmd.arg("-O2").arg(&c_path).arg("-o").arg(&exe_path);
    if cfg!(windows) {
        cmd.args(["-lgdi32", "-luser32", "-lws2_32"]);
    }
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {}: {}", cc, e));
    assert!(
        out.status.success(),
        "graphics program failed to compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut run = Command::new(&exe_path);
    run.env("KLOM_UI_AUTOCLOSE_MS", "900");
    run.current_dir(&build_dir);
    let res = run.output().unwrap_or_else(|e| panic!("cannot run gfx exe: {}", e));
    let _ = std::fs::remove_dir_all(&build_dir);
    assert!(
        res.status.success(),
        "graphics program crashed:\n{}",
        String::from_utf8_lossy(&res.stderr)
    );
    let stdout = String::from_utf8_lossy(&res.stdout);
    let ticks = stdout.lines().filter(|l| l.trim() == "টিক").count();
    assert!(
        ticks >= 3,
        "animation tick should fire >=3 times in 900ms at 150ms interval, got {} ({:?})",
        ticks,
        stdout
    );
}

#[test]
fn native_network_loopback() {
    let cc = match find_cc() {
        Some(c) => c,
        None => {
            eprintln!("skip: no C compiler found");
            return;
        }
    };

    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind failed");
    let port = listener.local_addr().unwrap().port();

    let server = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept failed");
        let mut buf = [0u8; 256];
        let n = sock.read(&mut buf).expect("server read failed");
        let received = String::from_utf8_lossy(&buf[..n]).to_string();
        sock.write_all("প্রীতি".as_bytes()).expect("server write failed");
        received
    });

    let src = format!(
        "ইম্পোর্ট নেটওয়ার্ক\n\nঅ্যাপ {{\n\n    ধরি s = নেটওয়ার্ক.যোগ(\"127.0.0.1\", {})\n\n    নেটওয়ার্ক.পাঠাও(s, \"শুভ সকাল\")\n\n    লেখো(নেটওয়ার্ক.নাও(s, ১০০))\n\n    নেটওয়ার্ক.বন্ধ(s)\n\n}}\n",
        port
    );

    let (tokens, lex_errs) = kolom_lexer::lex(&src);
    assert!(lex_errs.is_empty(), "{:?}", lex_errs);
    let (prog, parse_errs) = kolom_syntax::parse(tokens);
    assert!(parse_errs.is_empty(), "{:?}", parse_errs);
    let sema_errs = kolom_sema::analyze(&prog);
    assert!(sema_errs.is_empty(), "{:?}", sema_errs);

    // interpreter path
    let mut interp_out: Vec<u8> = Vec::new();
    kolom_interp::run(&prog, &mut interp_out).expect("interp net run failed");
    assert_eq!(String::from_utf8_lossy(&interp_out).trim(), "প্রীতি");
    let interp_received = server.join().expect("server thread panicked");
    assert_eq!(interp_received, "শুভ সকাল");

    // native path
    let c_code = kolom_codegen::emit(&prog, "নেট", "windows");
    let build_dir =
        std::env::temp_dir().join(format!("kolom-net-test-{}", std::process::id()));
    std::fs::create_dir_all(&build_dir).unwrap();
    let c_path = build_dir.join("net.c");
    std::fs::write(&c_path, c_code.as_bytes()).unwrap();
    let exe_path = build_dir.join(format!("net{}", if cfg!(windows) { ".exe" } else { "" }));

    let mut cmd = Command::new(&cc);
    cmd.arg("-O2").arg(&c_path).arg("-o").arg(&exe_path);
    if cfg!(windows) {
        cmd.args(["-lws2_32"]);
    }
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {}: {}", cc, e));
    assert!(
        out.status.success(),
        "net program failed to compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let listener2 = std::net::TcpListener::bind("127.0.0.1:0").expect("bind2 failed");
    let port2 = listener2.local_addr().unwrap().port();
    let src2 = src.replace(&format!("{}", port), &format!("{}", port2));
    let (tokens2, _) = kolom_lexer::lex(&src2);
    let (prog2, _) = kolom_syntax::parse(tokens2);
    let sema_errs2 = kolom_sema::analyze(&prog2);
    assert!(sema_errs2.is_empty());
let c_code2 = kolom_codegen::emit(&prog2, "নেট", "windows");
    std::fs::write(&c_path, c_code2.as_bytes()).unwrap();
    let port_str = format!("{}", port2);
    assert!(
        c_code2.contains(&port_str),
        "generated C missing port {}",
        port_str
    );
    eprintln!("dialing 127.0.0.1:{}", port_str);

    let mut cmd2 = Command::new(&cc);
    cmd2.arg("-O2").arg(&c_path).arg("-o").arg(&exe_path);
    if cfg!(windows) {
        cmd2.args(["-lws2_32"]);
    }
    let out2 = cmd2
        .output()
        .unwrap_or_else(|e| panic!("cannot run {}: {}", cc, e));
    assert!(
        out2.status.success(),
        "net program failed to compile:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );

    let server2 = std::thread::spawn(move || {
        let (mut sock, _) = listener2.accept().expect("accept2 failed");
        let mut buf = [0u8; 256];
        let n = sock.read(&mut buf).expect("server2 read failed");
        let received = String::from_utf8_lossy(&buf[..n]).to_string();
        sock.write_all("প্রীতি".as_bytes()).expect("server2 write failed");
        received
    });

    let res = Command::new(&exe_path)
        .output()
        .unwrap_or_else(|e| panic!("cannot run net exe: {}", e));
    let _ = std::fs::remove_dir_all(&build_dir);
    assert!(
        res.status.success(),
        "native net program crashed:\n{}",
        String::from_utf8_lossy(&res.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&res.stdout).trim(), "প্রীতি");
    let native_received = server2.join().expect("server2 thread panicked");
    assert_eq!(native_received, "শুভ সকাল");
}
