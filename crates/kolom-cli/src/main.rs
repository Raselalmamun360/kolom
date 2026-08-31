use std::io::Write as _;
use std::process::ExitCode;

use kolom_lexer::{bn_num, format_error, lex};

mod editor;

const VERSION: &str = "১.০.০";

const HELP: &str = "কলম — বাংলা প্রোগ্রামিং ভাষা

ব্যবহার:
  kolom চালাও <ফাইল.ক>    প্রোগ্রাম চালাও (ইন্টারপ্রিটেড)
  kolom run <file.k>     একই কাজ (ইংরেজি)
  kolom বিল্ড <ফাইল.ক>    নেটিভ এক্সিকিউটেবল তৈরি করো
  kolom build <file.k>   একই কাজ (ইংরেজি)
  kolom নতুন <নাম>       নতুন প্রকল্প তৈরি করো
  kolom new <name>       একই কাজ (ইংরেজি)
  kolom পাতা <ফাইল.ক>    বিল্ট-ইন এডিটর (লাইভ ত্রুটি + Ctrl+R চালাও)
  kolom edit <file.k>    একই কাজ (ইংরেজি)
  kolom যোগ <নাম> <git-url> --রেফ <tag>   নির্ভরতা যোগ করো (কলম.toml-এ)
  kolom add <name> <git-url> --ref <tag>  একই কাজ (ইংরেজি)
  kolom ইনস্টল            সব নির্ভরতা ফেচ করো (কলম.lock লেখে)
  kolom install           একই কাজ (ইংরেজি)
  kolom মুছো <নাম>         নির্ভরতা সরাও
  kolom remove <name>     একই কাজ (ইংরেজি)
  kolom টার্গেট           টার্গেট প্ল্যাটফর্ম তালিকা
  kolom lex <ফাইল.ক>     টোকেন ডিবাগ আউটপুট
  kolom সংস্করণ           সংস্করণ দেখাও
  kolom সাহায্য          এই সাহায্য
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("run") | Some("চালাও") => cmd_run(args.get(1), args.get(2..).unwrap_or(&[])),
        Some("build") | Some("বিল্ড") => {
            let rest: Vec<&String> = args[1..].iter().filter(|a| !a.starts_with("--")).collect();
            cmd_build(rest.first().copied(), rest.get(1).copied())
        }
        Some("new") | Some("নতুন") => cmd_new(args.get(1)),
        Some("edit") | Some("পাতা") => cmd_edit(args.get(1)),
        Some("add") | Some("যোগ") => cmd_add(&args[1..]),
        Some("install") | Some("ইনস্টল") => cmd_install(),
        Some("remove") | Some("মুছো") => cmd_remove(args.get(1)),
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

fn cmd_run(path: Option<&String>, extra_args: &[String]) -> ExitCode {
    let (prog, _file, name) = match check_program(path) {
        Ok(x) => x,
        Err(c) => return c,
    };

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    match kolom_interp::run_with_argv(&prog, &mut lock, extra_args.to_vec()) {
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

use kolom_cli::resolve_user_modules;


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

fn cmd_build(path: Option<&String>, target: Option<&String>) -> ExitCode {
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

    // Cranelift emits machine code directly and a bundled linker produces
    // the executable — no C compiler involved at any point.
    let tgt = match kolom_codegen_cranelift::Target::from_name(target) {
        Some(t) => t,
        None => {
            eprintln!(
                "ত্রুটি: '{}' টার্গেট সমর্থিত নয় (windows, linux, android)",
                target
            );
            return ExitCode::FAILURE;
        }
    };
    let obj_bytes = match kolom_codegen_cranelift::emit_for(&prog, tgt) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ত্রুটি: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let obj_path = build_dir.join("out.obj");
    if std::fs::write(&obj_path, &obj_bytes).is_err() {
        eprintln!("ত্রুটি: অবজেক্ট ফাইল লেখা যায়নি");
        return ExitCode::FAILURE;
    }
    let exe_path = build_dir.join(format!("{}{}", exe_name, tgt.exe_suffix()));
    match kolom_codegen_cranelift::link_executable_for(tgt, &obj_path, &exe_path) {
        Ok(()) => {
            println!("{}", exe_path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ত্রুটি: {}", e);
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

    if let Err(e) = kolom_pkg::Manifest::new(&project_name).save(dir) {
        eprintln!("ত্রুটি: কলম.toml লেখা যায়নি: {}", e);
        return ExitCode::FAILURE;
    }

    println!("প্রকল্প '{}' তৈরি হয়েছে", project_name);
    println!("  {}/main.ক", project_name);
    println!("  {}/{}", project_name, kolom_pkg::MANIFEST_FILE);
    println!();
    println!("চালাতে:");
    println!("  kolom চালাও {}/main.ক", project_name);
    println!("বিল্ড করতে:");
    println!("  kolom বিল্ড {}/main.ক", project_name);
    ExitCode::SUCCESS
}

fn cmd_add(rest: &[String]) -> ExitCode {
    let mut reference: Option<String> = None;
    let mut positional: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if a == "--রেফ" || a == "--ref" {
            reference = rest.get(i + 1).cloned();
            i += 2;
            continue;
        }
        positional.push(a);
        i += 1;
    }
    let (name, url) = match (positional.first(), positional.get(1)) {
        (Some(n), Some(u)) => (n.to_string(), u.to_string()),
        _ => {
            eprintln!("ব্যবহার: kolom যোগ <নাম> <git-url> --রেফ <tag>");
            return ExitCode::FAILURE;
        }
    };
    let Some(reference) = reference else {
        eprintln!("ত্রুটি: --রেফ <tag> দিতে হবে (যেমন: --রেফ v1.0.0)");
        return ExitCode::FAILURE;
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut manifest = kolom_pkg::Manifest::load(&cwd).unwrap_or_else(|_| {
        let project_name = cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "প্রকল্প".to_string());
        kolom_pkg::Manifest::new(&project_name)
    });
    manifest.dependencies.insert(
        name.clone(),
        kolom_pkg::Dependency { git: url.clone(), reference: reference.clone() },
    );
    if let Err(e) = manifest.save(&cwd) {
        eprintln!("ত্রুটি: {}", e);
        return ExitCode::FAILURE;
    }
    println!("'{}' যোগ হয়েছে ({} @ {})", name, url, reference);
    println!("ইনস্টল করতে: kolom ইনস্টল");
    ExitCode::SUCCESS
}

fn cmd_install() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if kolom_pkg::Manifest::load(&cwd).is_err() {
        eprintln!("ত্রুটি: এই ফোল্ডারে কোনো {} নেই", kolom_pkg::MANIFEST_FILE);
        return ExitCode::FAILURE;
    }
    let existing_lock = kolom_pkg::Lockfile::load(&cwd).unwrap_or_default();
    match kolom_pkg::install(&cwd, &existing_lock) {
        Ok(lock) => {
            let count = lock.packages.len();
            if let Err(e) = lock.save(&cwd) {
                eprintln!("ত্রুটি: {}", e);
                return ExitCode::FAILURE;
            }
            println!("{}টি প্যাকেজ ইনস্টল হয়েছে", bn_num(count as u32));
            for p in &lock.packages {
                let short = &p.commit[..p.commit.len().min(12)];
                println!("  {} ({})", p.name, short);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ত্রুটি: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn cmd_remove(name: Option<&String>) -> ExitCode {
    let name = match name {
        Some(n) => n,
        None => {
            eprintln!("ব্যবহার: kolom মুছো <নাম>");
            return ExitCode::FAILURE;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut manifest = match kolom_pkg::Manifest::load(&cwd) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ত্রুটি: {}", e);
            return ExitCode::FAILURE;
        }
    };
    if manifest.dependencies.remove(name).is_none() {
        eprintln!("ত্রুটি: '{}' নির্ভরতা তালিকায় নেই", name);
        return ExitCode::FAILURE;
    }
    if let Err(e) = manifest.save(&cwd) {
        eprintln!("ত্রুটি: {}", e);
        return ExitCode::FAILURE;
    }
    let pkg_dir = kolom_pkg::packages_dir(&cwd).join(name);
    if pkg_dir.exists() {
        let _ = std::fs::remove_dir_all(&pkg_dir);
    }
    println!("'{}' সরানো হয়েছে", name);
    println!("লক ফাইল হালনাগাদ করতে: kolom ইনস্টল");
    ExitCode::SUCCESS
}

fn cmd_edit(path: Option<&String>) -> ExitCode {
    let path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            eprintln!("ব্যবহার: kolom পাতা <ফাইল.ক>");
            return ExitCode::FAILURE;
        }
    };
    match editor::run(&path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ত্রুটি: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn cmd_target() -> ExitCode {
    println!("কলম টার্গেট");
    println!();
    println!("  windows   Windows x64 — Win32/GDI UI (ডিফল্ট)");
    println!("             কোনো বাইরের টুল লাগে না।");
    println!("  linux     Linux x64 — স্ট্যাটিক musl বাইনারি, কনসোল মোড");
    println!("             কোনো বাইরের টুল লাগে না।");
    println!("  android   aarch64 (arm64-v8a) — স্ট্যাটিক বিল্ড, কনসোল মোড");
    println!("             বিল্ড-মেশিনে Android NDK লাগে (Bionic কোনো Rust");
    println!("             টুলচেইনের সাথে বান্ডলড আসে না, musl-এর মতো নয়) —");
    println!("             ANDROID_NDK_HOME (বা ANDROID_HOME/ANDROID_SDK_ROOT)");
    println!("             দিয়ে পথ দিতে হবে।");
    println!();
    println!("ব্যবহার: kolom বিল্ড <ফাইল> <টার্গেট>");
    println!("উদাহরণ:");
    println!("  kolom বিল্ড main.ক windows");
    println!("  kolom বিল্ড main.ক linux");
    println!("  ANDROID_NDK_HOME=<NDK-এর পথ> kolom বিল্ড main.ক android");
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
