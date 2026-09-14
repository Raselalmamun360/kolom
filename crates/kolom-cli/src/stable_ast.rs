use kolom_syntax::ast::*;

fn esc(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ch if (ch as u32) < 32 || ch == '\u{7f}' => {
                out.push_str(&format!("\\u{{{:x}}}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out
}

fn record(out: &mut String, kind: &str, pos: Option<Pos>, payload: &str) {
    let (line, col) = pos.map(|p| (p.line, p.col)).unwrap_or((0, 0));
    out.push_str("NODE\t");
    out.push_str(kind);
    out.push('\t');
    out.push_str(&line.to_string());
    out.push('\t');
    out.push_str(&col.to_string());
    out.push('\t');
    out.push_str(&esc(payload));
    out.push('\n');
}

fn begin(out: &mut String, kind: &str, pos: Option<Pos>) {
    record(out, &format!("begin_{}", kind), pos, "");
}

fn end(out: &mut String, kind: &str) {
    record(out, &format!("end_{}", kind), None, "");
}

fn ident(out: &mut String, id: &Ident) {
    record(out, "ident", Some(id.pos), &id.name);
}

fn ty(out: &mut String, value: &TypeExpr) {
    match value {
        TypeExpr::Named(id) => {
            record(out, "type_named", Some(id.pos), "");
            ident(out, id);
        }
        TypeExpr::Array(inner) => {
            begin(out, "type_array", None);
            ty(out, inner);
            end(out, "type_array");
        }
        TypeExpr::Shared(inner) => {
            begin(out, "type_shared", None);
            ty(out, inner);
            end(out, "type_shared");
        }
        TypeExpr::Map(key, value) => {
            begin(out, "type_map", None);
            ty(out, key);
            ty(out, value);
            end(out, "type_map");
        }
        TypeExpr::Func(params, ret) => {
            begin(out, "type_func", None);
            for param in params { ty(out, param); }
            ty(out, ret);
            end(out, "type_func");
        }
        TypeExpr::Generic(id, args) => {
            begin(out, "type_generic", Some(id.pos));
            ident(out, id);
            for arg in args { ty(out, arg); }
            end(out, "type_generic");
        }
    }
}

fn block(out: &mut String, value: &Block) {
    begin(out, "block", None);
    for item in &value.stmts { stmt(out, item); }
    end(out, "block");
}

fn lit(out: &mut String, value: &Lit) {
    match value {
        Lit::Int(v) => record(out, "lit_int", None, &v.to_string()),
        Lit::Float(v) => record(out, "lit_float", None, &v.to_string()),
        Lit::Str(v) => record(out, "lit_str", None, v),
        Lit::Char(v) => record(out, "lit_char", None, &v.to_string()),
        Lit::Bool(v) => record(out, "lit_bool", None, if *v { "true" } else { "false" }),
        Lit::Null => record(out, "lit_null", None, ""),
        Lit::Array(items) => {
            begin(out, "lit_array", None);
            for item in items { expr(out, item); }
            end(out, "lit_array");
        }
    }
}

fn suffix(out: &mut String, value: &Suffix) {
    match value {
        Suffix::Call(args, pos) => {
            begin(out, "suffix_call", Some(*pos));
            for arg in args { expr(out, arg); }
            end(out, "suffix_call");
        }
        Suffix::Index(index, pos) => {
            begin(out, "suffix_index", Some(*pos));
            expr(out, index);
            end(out, "suffix_index");
        }
        Suffix::Field(id) => {
            begin(out, "suffix_field", Some(id.pos));
            ident(out, id);
            end(out, "suffix_field");
        }
    }
}

fn op_name(value: BinOp) -> &'static str {
    match value { BinOp::Add => "add", BinOp::Sub => "sub", BinOp::Mul => "mul", BinOp::Div => "div", BinOp::Mod => "mod", BinOp::Eq => "eq", BinOp::Neq => "neq", BinOp::Lt => "lt", BinOp::Gt => "gt", BinOp::Le => "le", BinOp::Ge => "ge", BinOp::And => "and", BinOp::Or => "or" }
}

fn expr(out: &mut String, value: &Expr) {
    match &value.kind {
        ExprKind::Lit(literal) => { begin(out, "expr_lit", Some(value.pos)); lit(out, literal); end(out, "expr_lit"); }
        ExprKind::Ident(id) => { begin(out, "expr_ident", Some(value.pos)); ident(out, id); end(out, "expr_ident"); }
        ExprKind::Qualified { module, name } => { begin(out, "expr_qualified", Some(value.pos)); ident(out, module); ident(out, name); end(out, "expr_qualified"); }
        ExprKind::Unary(op, inner) => { begin(out, "expr_unary", Some(value.pos)); record(out, "operator", Some(value.pos), if matches!(op, UnaryOp::Neg) { "neg" } else { "not" }); expr(out, inner); end(out, "expr_unary"); }
        ExprKind::Postfix(inner, suffixes) => { begin(out, "expr_postfix", Some(value.pos)); expr(out, inner); for item in suffixes { suffix(out, item); } end(out, "expr_postfix"); }
        ExprKind::Binary(op, left, right) => { begin(out, "expr_binary", Some(value.pos)); record(out, "operator", Some(value.pos), op_name(*op)); expr(out, left); expr(out, right); end(out, "expr_binary"); }
        ExprKind::Assign(target, rhs) => { begin(out, "expr_assign", Some(value.pos)); lvalue(out, target); expr(out, rhs); end(out, "expr_assign"); }
        ExprKind::FieldAssign(base, field, rhs) => { begin(out, "expr_field_assign", Some(value.pos)); ident(out, base); ident(out, field); expr(out, rhs); end(out, "expr_field_assign"); }
        ExprKind::Match(value) => { begin(out, "expr_match", Some(value.pos)); expr(out, &value.scrutinee); for arm in &value.arms { pattern(out, &arm.pattern); expr(out, &arm.body); } end(out, "expr_match"); }
    }
}

fn lvalue(out: &mut String, value: &LValue) {
    begin(out, "lvalue", Some(value.base.pos));
    ident(out, &value.base);
    for item in &value.idx { expr(out, item); }
    if let Some(field) = &value.field { ident(out, field); }
    end(out, "lvalue");
}

fn pattern(out: &mut String, value: &Pattern) {
    match value {
        Pattern::Wildcard(pos) => record(out, "pattern_wildcard", Some(*pos), ""),
        Pattern::Variant { name, binds, pos } => {
            begin(out, "pattern_variant", Some(*pos));
            ident(out, name);
            for bind in binds { ident(out, bind); }
            end(out, "pattern_variant");
        }
    }
}

fn stmt(out: &mut String, value: &Stmt) {
    match value {
        Stmt::Var(v) => { begin(out, "stmt_var", Some(v.name.pos)); ident(out, &v.name); if let Some(t) = &v.ty { ty(out, t); } expr(out, &v.init); end(out, "stmt_var"); }
        Stmt::Const(v) => { begin(out, "stmt_const", Some(v.name.pos)); ident(out, &v.name); ty(out, &v.ty); expr(out, &v.init); end(out, "stmt_const"); }
        Stmt::If(v) => { begin(out, "stmt_if", Some(v.pos)); expr(out, &v.cond); block(out, &v.then); if let Some(e) = &v.els { match e { ElseBranch::If(i) => stmt(out, &Stmt::If((**i).clone())), ElseBranch::Block(b) => block(out, b) } } end(out, "stmt_if"); }
        Stmt::Loop(v) => { begin(out, "stmt_loop", Some(v.pos)); expr(out, &v.count); block(out, &v.body); end(out, "stmt_loop"); }
        Stmt::While(v) => { begin(out, "stmt_while", Some(v.pos)); expr(out, &v.cond); block(out, &v.body); end(out, "stmt_while"); }
        Stmt::ForEach(v) => { begin(out, "stmt_foreach", Some(v.pos)); ident(out, &v.var); expr(out, &v.iter); block(out, &v.body); end(out, "stmt_foreach"); }
        Stmt::Return(v) => { begin(out, "stmt_return", Some(v.pos)); if let Some(e) = &v.value { expr(out, e); } end(out, "stmt_return"); }
        Stmt::Break(pos) => record(out, "stmt_break", Some(*pos), ""),
        Stmt::Continue(pos) => record(out, "stmt_continue", Some(*pos), ""),
        Stmt::Expr(e) => { begin(out, "stmt_expr", Some(e.pos)); expr(out, e); end(out, "stmt_expr"); }
        Stmt::Nested(b) => { begin(out, "stmt_nested", None); block(out, b); end(out, "stmt_nested"); }
        Stmt::TryCatch(v) => { begin(out, "stmt_try", Some(v.err_var.pos)); block(out, &v.body); ident(out, &v.err_var); block(out, &v.handler); end(out, "stmt_try"); }
        Stmt::Widget(v) => { begin(out, "stmt_widget", Some(v.pos)); record(out, "widget_name", Some(v.pos), &v.kw); for e in &v.args { expr(out, e); } if let Some(b) = &v.body { block(out, b); } end(out, "stmt_widget"); }
        Stmt::Display(b) => { begin(out, "stmt_display", None); block(out, b); end(out, "stmt_display"); }
    }
}

pub fn dump(program: &Program) -> String {
    let mut out = String::new();
    begin(&mut out, "program", None);
    for id in &program.imports { ident(&mut out, id); }
    for decl in &program.structs { begin(&mut out, "struct", Some(decl.name.pos)); ident(&mut out, &decl.name); for p in &decl.type_params { ident(&mut out, p); } for (name, ty_value) in &decl.fields { ident(&mut out, name); ty(&mut out, ty_value); } end(&mut out, "struct"); }
    for decl in &program.enums { begin(&mut out, "enum", Some(decl.name.pos)); ident(&mut out, &decl.name); for p in &decl.type_params { ident(&mut out, p); } for (name, payload) in &decl.variants { begin(&mut out, "variant", Some(name.pos)); ident(&mut out, name); for ty_value in payload { ty(&mut out, ty_value); } end(&mut out, "variant"); } end(&mut out, "enum"); }
    for decl in &program.funcs { begin(&mut out, "function", Some(decl.name.pos)); ident(&mut out, &decl.name); for p in &decl.type_params { ident(&mut out, p); } for param in &decl.params { ty(&mut out, &param.ty); ident(&mut out, &param.name); } ty(&mut out, &decl.ret); block(&mut out, &decl.body); end(&mut out, "function"); }
    for decl in &program.externs { begin(&mut out, "extern", Some(decl.pos)); record(&mut out, "abi", Some(decl.pos), &decl.abi); for func in &decl.funcs { begin(&mut out, "extern_function", Some(func.name.pos)); ident(&mut out, &func.name); for param in &func.params { ty(&mut out, &param.ty); ident(&mut out, &param.name); } ty(&mut out, &func.ret); end(&mut out, "extern_function"); } end(&mut out, "extern"); }
    for decl in &program.consts { begin(&mut out, "const", Some(decl.name.pos)); ident(&mut out, &decl.name); ty(&mut out, &decl.ty); expr(&mut out, &decl.init); end(&mut out, "const"); }
    if let Some(app) = &program.app { begin(&mut out, "app", app.name.as_ref().map(|n| n.pos)); if let Some(name) = &app.name { ident(&mut out, name); } block(&mut out, &app.body); end(&mut out, "app"); }
    end(&mut out, "program");
    out
}

pub fn dump_errors(phase: &str, errors: &[kolom_lexer::Diagnostic]) -> String {
    let mut out = String::new();
    for error in errors {
        out.push_str("ERR\t");
        out.push_str(phase);
        out.push('\t');
        out.push_str(&error.line.to_string());
        out.push('\t');
        out.push_str(&error.col.to_string());
        out.push('\t');
        out.push_str(&esc(&error.message));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{dump, dump_errors};

    #[test]
    fn stable_dump_is_deterministic_and_not_debug_output() {
        let (tokens, lex_errors) = kolom_lexer::lex("ফাংশন যোগ(সংখ্যা x) -> সংখ্যা { রিটার্ন x + ১ }\n");
        assert!(lex_errors.is_empty());
        let (program, parse_errors) = kolom_syntax::parse(tokens);
        assert!(parse_errors.is_empty());
        let first = dump(&program);
        let second = dump(&program);
        assert_eq!(first, second);
        assert!(first.starts_with("NODE\tbegin_program\t0\t0\t\n"));
        assert!(first.contains("NODE\tbegin_function\t"));
        assert!(!first.contains("FuncDecl"));
    }

    #[test]
    fn stable_errors_have_a_comparable_shape() {
        let errors = vec![kolom_lexer::Diagnostic {
            line: 2,
            col: 4,
            message: "ভুল\nবার্তা".to_string(),
        }];
        assert_eq!(dump_errors("parse", &errors), "ERR\tparse\t2\t4\tভুল\\nবার্তা\n");
    }

    #[test]
    fn stable_dump_covers_golden_sources_that_parse() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("golden");
        let mut checked = 0;
        for entry in std::fs::read_dir(root).expect("golden directory") {
            let dir = entry.expect("golden entry").path();
            let source_path = dir.join("main.ক");
            if !source_path.is_file() {
                continue;
            }
            let source = std::fs::read_to_string(source_path).expect("golden source");
            let (tokens, lex_errors) = kolom_lexer::lex(&source);
            if !lex_errors.is_empty() {
                continue;
            }
            let (program, parse_errors) = kolom_syntax::parse(tokens);
            if !parse_errors.is_empty() {
                continue;
            }
            let first = dump(&program);
            assert_eq!(first, dump(&program));
            assert!(!first.contains("StructDecl"));
            checked += 1;
        }
        assert!(checked >= 50, "too few golden AST fixtures checked: {checked}");
    }
}
