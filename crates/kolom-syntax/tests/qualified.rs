use kolom_lexer::lex;
use kolom_syntax::ast::*;
use kolom_syntax::parse;

fn lower_first_init(prog: &Program) -> Option<Expr> {
    let app = prog.app.as_ref()?;
    for st in &app.body.stmts {
        if let Stmt::Var(v) = st {
            return Some(v.init.clone());
        }
    }
    None
}

#[test]
fn qualified_call_parses() {
    let src = "ইম্পোর্ট গণিত\n\nঅ্যাপ {\n\n    ধরি x = গণিত.পরম(-৫)\n\n}\n";
    let (tokens, lex_errs) = lex(src);
    assert!(lex_errs.is_empty(), "{:?}", lex_errs);
    let (prog, diags) = parse(tokens);
    assert!(diags.is_empty(), "parse diags: {:?}", diags);

    let init = lower_first_init(&prog).expect("no var decl found");
    match init.kind {
        ExprKind::Postfix(base, sfx) => {
            assert!(
                matches!(&base.kind, ExprKind::Qualified { module, .. } if module.name == "গণিত"),
                "base should be qualified, got {:?}",
                base.kind
            );
            assert_eq!(sfx.len(), 1, "expected one call suffix");
        }
        other => panic!("unexpected init expr: {:?}", other),
    }
}
