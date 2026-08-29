use std::io::Write as _;
use std::process::ExitCode;

use kolom_lexer::{bn_num, format_error, lex};

const VERSION: &str = "০.১.০";

const HELP: &str = "কলম — বাংলা প্রোগ্রামিং ভাষা

ব্যবহার:
  kolom চালাও <ফাইল.ক>    প্রোগ্রাম চালাও (ইন্টারপ্রিটেড)
  kolom run <file.k>     একই কাজ (ইংরেজি)
  kolom বিল্ড <ফাইল.ক>    নেটিভ এক্সিকিউটেবল তৈরি করো
  kolom build <file.k>   একই কাজ (ইংরেজি)
      --সি | --c-backend  পুরনো C ব্যাকএন্ড ব্যবহার করো (C কম্পাইলার লাগবে)
  kolom নতুন <নাম>       নতুন প্রকল্প তৈরি করো
  kolom new <name>       একই কাজ (ইংরেজি)
  kolom টার্গেট           টার্গেট প্ল্যাটফর্ম তালিকা
  kolom lex <ফাইল.ক>     টোকেন ডিবাগ আউটপুট
  kolom সংস্করণ           সংস্করণ দেখাও
  kolom সাহায্য          এই সাহায্য
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("run") | Some("চালাও") => cmd_run(args.get(1)),
        Some("build") | Some("বিল্ড") => {
            let use_c = args.iter().any(|a| a == "--সি" || a == "--c-backend");
            let rest: Vec<&String> = args[1..].iter().filter(|a| !a.starts_with("--")).collect();
            cmd_build(rest.first().copied(), rest.get(1).copied(), use_c)
        }
        Some("new") | Some("নতুন") => cmd_new(args.get(1)),
        Some("target") | Some("টার্গেট") => cmd_target(),
        Some("lex") => cmd_lex(args.get(1)),
        Some("version") | Some("--version") | Some("-V") | Some("সংস্করণ") => {
            println!("কলম {}", VERSION);
            ExitCode::SUCCESS
        }
        Some("help") | Some("--help") | Some("-h") | Some("সাহায্য") => {
            print!("{}", HELP);
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{}", HELP);
            if args.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

fn read_source(path: Option<&String>) -> Result<(String, String), ExitCode> {
    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("ত্রুটি: ফাইলের পথ দাও — উদাহরণ: kolom চালাও main.ক");
            return Err(ExitCode::FAILURE);
        }
    };
    match std::fs::read_to_string(path) {
        Ok(src) => Ok((src, path.clone())),
        Err(e) => {
            let reason = if e.kind() == std::io::ErrorKind::InvalidData {
                "ফাইলটি UTF-8 এনকোডেড নয়".to_string()
            } else {
                format!("{}", e)
            };
            eprintln!("ত্রুটি: {}: {}", path, reason);
            Err(ExitCode::FAILURE)
        }
    }
}

fn print_diags(prefix: &str, file: &str, diags: &[kolom_lexer::Diagnostic]) {
    let mut err = std::io::stderr();
    let src = std::fs::read_to_string(file).unwrap_or_default();
    let lines: Vec<&str> = src.lines().collect();
    for d in diags {
        let _ = writeln!(
            err,
            "{}",
            format_error(prefix, file, d.line, d.col, &d.message)
        );
        let li = (d.line as usize).checked_sub(1);
        if let Some(li) = li {
            if li < lines.len() {
                let source_line = lines[li];
                let _ = writeln!(err);
                let _ = writeln!(err, "  {}", source_line);
                let caret_pad = " ".repeat((d.col as usize).saturating_sub(1));
                let _ = writeln!(err, "  {}^", caret_pad);
            }
        }
    }
}

fn display_file_name(file: &str) -> String {
    std::path::Path::new(file)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string())
}

fn cmd_run(path: Option<&String>) -> ExitCode {
    let (prog, _file, name) = match check_program(path) {
        Ok(x) => x,
        Err(c) => return c,
    };

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    match kolom_interp::run(&prog, &mut lock) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr(),
                "{}",
                format_error("রানটাইম ত্রুটি", &name, e.line, e.col, &e.message)
            );
            ExitCode::FAILURE
        }
    }
}

fn resolve_user_modules(
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
            if !kolom_sema::STDLIB_MODULES.contains(&imp.name.as_str()) {
                queue.push(imp.name.clone());
            }
        }
        prog.funcs.extend(sub_prog.funcs.drain(..));
        prog.consts.extend(sub_prog.consts.drain(..));
    }
    prog.imports
        .retain(|i| kolom_sema::STDLIB_MODULES.contains(&i.name.as_str()));
    Ok(())
}

fn check_program(path: Option<&String>) -> Result<(kolom_syntax::ast::Program, String, String), ExitCode> {
    let (src, file) = match read_source(path) {
        Ok(x) => x,
        Err(c) => return Err(c),
    };
    let name = display_file_name(&file);

    let (tokens, lex_errs) = lex(&src);
    if !lex_errs.is_empty() {
        print_diags("ত্রুটি", &name, &lex_errs);
        return Err(ExitCode::FAILURE);
    }

    let (mut prog, parse_errs) = kolom_syntax::parse(tokens);
    if !parse_errs.is_empty() {
        print_diags("ত্রুটি", &name, &parse_errs);
        return Err(ExitCode::FAILURE);
    }

    let main_dir = std::path::Path::new(&file)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    if let Err(e) = resolve_user_modules(&mut prog, &main_dir) {
        eprintln!("ত্রুটি: {}", e);
        return Err(ExitCode::FAILURE);
    }

    let sema_errs = kolom_sema::analyze(&prog);
    if !sema_errs.is_empty() {
        print_diags("ত্রুটি", &name, &sema_errs);
        return Err(ExitCode::FAILURE);
    }

    Ok((prog, file, name))
}

// LEGACY FALLBACK: emits C source (kolom-codegen) and shells out to an
// external gcc/clang/cc/cl found on PATH. The Cranelift backend
// (crates/kolom-codegen-cranelift + kolom-runtime) is now the default for
// `কলম বিল্ড` and needs no C compiler at all; this path remains reachable
// via `--সি`/`--c-backend` because the Cranelift backend does not yet cover
// the নেটওয়ার্ক module or non-Windows targets. Delete it once those land.
fn find_c_compiler() -> Option<String> {
    if let Ok(cc) = std::env::var("KLOM_CC") {
        if !cc.trim().is_empty() {
            return Some(cc);
        }
    }
    let candidates = ["gcc", "clang", "cc", "cl"];
    for c in candidates {
        let probe = if cfg!(windows) {
            std::process::Command::new("where").arg(c).output()
        } else {
            std::process::Command::new("which").arg(c).output()
        };
        if let Ok(out) = probe {
            if out.status.success() {
                let first = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !first.is_empty() {
                    return Some(first);
                }
            }
        }
    }
    None
}

fn cmd_build(path: Option<&String>, target: Option<&String>, use_c_backend: bool) -> ExitCode {
    let target = target.map(|s| s.as_str()).unwrap_or("windows");
    let (prog, file, _name) = match check_program(path) {
        Ok(x) => x,
        Err(c) => return c,
    };

    let exe_name = match &prog.app {
        Some(app) => match &app.name {
            Some(n) => n.name.clone(),
            None => std::path::Path::new(&file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "app".into()),
        },
        None => "app".into(),
    };

    let build_dir = std::env::temp_dir().join(format!("kolom-build-{}", std::process::id()));
    if std::fs::create_dir_all(&build_dir).is_err() {
        eprintln!("ত্রুটি: বিল্ড ফোল্ডার তৈরি করা যায়নি");
        return ExitCode::FAILURE;
    }
    let mut exe_path = build_dir.join(format!("{}{}", exe_name, if cfg!(windows) { ".exe" } else { "" }));

    // Default path: Cranelift emits machine code directly and a bundled
    // linker produces the executable — no C compiler involved at any point.
    if !use_c_backend {
        let tgt = match kolom_codegen_cranelift::Target::from_name(target) {
            Some(t) => t,
            None => {
                eprintln!(
                    "ত্রুটি: '{}' টার্গেট Cranelift ব্যাকএন্ডে এখনো সমর্থিত নয় (windows, linux)",
                    target
                );
                eprintln!("(C ব্যাকএন্ড দিয়ে চেষ্টা করতে: কলম বিল্ড --সি <ফাইল.ক> {})", target);
                return ExitCode::FAILURE;
            }
        };
        let obj_bytes = match kolom_codegen_cranelift::emit_for(&prog, tgt) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("ত্রুটি: {}", e);
                eprintln!("(পুরনো C ব্যাকএন্ড দিয়ে চেষ্টা করতে: কলম বিল্ড --সি <ফাইল.ক>)");
                return ExitCode::FAILURE;
            }
        };
        let obj_path = build_dir.join("out.obj");
        if std::fs::write(&obj_path, &obj_bytes).is_err() {
            eprintln!("ত্রুটি: অবজেক্ট ফাইল লেখা যায়নি");
            return ExitCode::FAILURE;
        }
        exe_path = build_dir.join(format!("{}{}", exe_name, tgt.exe_suffix()));
        return match kolom_codegen_cranelift::link_executable_for(tgt, &obj_path, &exe_path) {
            Ok(()) => {
                println!("{}", exe_path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("ত্রুটি: {}", e);
                ExitCode::FAILURE
            }
        };
    }

    let c_source = kolom_codegen::emit(&prog, &exe_name, target);

    let c_path = build_dir.join("out.c");
    if std::fs::write(&c_path, c_source.as_bytes()).is_err() {
        eprintln!("ত্রুটি: C সোর্স লেখা যায়নি");
        return ExitCode::FAILURE;
    }

    let cc = match find_c_compiler() {
        Some(c) => c,
        None => {
            eprintln!(
                "ত্রুটি: কোনো C কম্পাইলার পাওয়া যায়নি — gcc/clang/cl ইনস্টল করো, অথবা KLOM_CC এনভায়রনমেন্ট ভ্যারিয়েবল দাও"
            );
            eprintln!("C সোর্স তৈরি হয়েছে: {}", c_path.display());
            return ExitCode::FAILURE;
        }
    };

    let mut cmd = std::process::Command::new(&cc);
    cmd.arg("-O2").arg(&c_path).arg("-o").arg(&exe_path);
    if cfg!(windows) && target != "linux" && target != "android" {
        cmd.args(["-lgdi32", "-luser32", "-lws2_32"]);
    } else if target == "windows" {
        cmd.args(["-lgdi32", "-luser32", "-lws2_32"]);
    }
    let status = cmd.status();

    match status {
        Ok(s) if s.success() => {
            println!("{}", exe_path.display());
            ExitCode::SUCCESS
        }
        Ok(s) => {
            eprintln!(
                "ত্রুটি: C কম্পাইলেশন ব্যর্থ ({}) — কোড {}",
                cc,
                s.code().unwrap_or(-1)
            );
            eprintln!("C সোর্স: {}", c_path.display());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("ত্রুটি: '{}' চালানো যায়নি ({})", cc, e);
            ExitCode::FAILURE
        }
    }
}

fn cmd_new(name: Option<&String>) -> ExitCode {
    let project_name = match name {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => {
            eprintln!("ব্যবহার: kolom নতুন <প্রকল্পের_নাম>");
            return ExitCode::FAILURE;
        }
    };
    let dir = std::path::Path::new(&project_name);
    if dir.exists() {
        eprintln!("ত্রুটি: '{}' আগেই আছে", project_name);
        return ExitCode::FAILURE;
    }
    if std::fs::create_dir_all(dir).is_err() {
        eprintln!("ত্রুটি: ফোল্ডার তৈরি করা যায়নি");
        return ExitCode::FAILURE;
    }

    let main_src = format!(
        "অ্যাপ {} {{\n\n    লেখো(\"হ্যালো, {}!\")\n\n}}\n",
        project_name, project_name
    );
    let main_path = dir.join("main.ক");
    if std::fs::write(&main_path, main_src.as_bytes()).is_err() {
        eprintln!("ত্রুটি: main.ক লেখা যায়নি");
        return ExitCode::FAILURE;
    }

    println!("প্রকল্প '{}' তৈরি হয়েছে", project_name);
    println!("  {}/main.ক", project_name);
    println!();
    println!("চালাতে:");
    println!("  kolom চালাও {}/main.ক", project_name);
    println!("বিল্ড করতে:");
    println!("  kolom বিল্ড {}/main.ক", project_name);
    ExitCode::SUCCESS
}

fn cmd_target() -> ExitCode {
    println!("কলম টার্গেট");
    println!();
    println!("  windows   Windows x64 — Win32/GDI UI (ডিফল্ট)");
    println!("  linux     Linux x64 — স্ট্যাটিক musl বাইনারি, কনসোল মোড");
    println!("  android   Android NDK — শুধু `--সি` ব্যাকএন্ডে");
    println!();
    println!("ব্যবহার: kolom বিল্ড <ফাইল> <টার্গেট>");
    println!("উদাহরণ:");
    println!("  kolom বিল্ড main.ক windows");
    println!("  kolom বিল্ড main.ক linux");
    println!("  kolom বিল্ড main.ক android");
    ExitCode::SUCCESS
}

fn cmd_lex(path: Option<&String>) -> ExitCode {
    let (src, _file) = match read_source(path) {
        Ok(x) => x,
        Err(c) => return c,
    };
    let (tokens, errs) = lex(&src);
    for t in &tokens {
        println!("{:>4}:{:<4} {:?}", t.line, bn_num(t.col), t.kind);
    }
    for e in &errs {
        println!("{:>4}:{:<4} ত্রুটি: {}", e.line, bn_num(e.col), e.message);
    }
    if errs.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
