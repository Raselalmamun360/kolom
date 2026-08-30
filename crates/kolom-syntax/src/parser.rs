use kolom_lexer::{NumTok, Token, TokenKind};
use std::rc::Rc;

use crate::ast::*;

const PRIMITIVE_TYPES: &[&str] = &[
    "সংখ্যা",
    "দশমিক",
    "লেখা",
    "বুলিয়ান",
    "অক্ষর",
    "ফাঁকা",
];

const WIDGETS: &[&str] = &[
    "টেক্সট", "বাটন", "ইনপুট", "ছবি", "সারি", "কলাম", "কার্ড", "ডায়ালগ", "স্ক্রল", "ক্যানভাস",
];

const CONTAINER_WIDGETS: &[&str] = &["সারি", "কলাম", "কার্ড", "ডায়ালগ", "স্ক্রল"];

const SYNC_KEYWORDS: &[&str] = &["অ্যাপ", "ফাংশন", "ধরি", "ধ্রুবক", "ইম্পোর্ট"];

struct P {
    t: Vec<Token>,
    p: usize,
    diags: Vec<Diagnostic>,
    fn_depth: u32,
    loop_depth: u32,
}

pub type Diagnostic = kolom_lexer::Diagnostic;

impl P {
    fn at_eof(&self) -> bool {
        self.p >= self.t.len()
    }

    fn kind(&self) -> Option<&TokenKind> {
        self.t.get(self.p).map(|t| &t.kind)
    }

    fn pos(&self) -> Pos {
        match self.t.get(self.p) {
            Some(t) => Pos {
                line: t.line,
                col: t.col,
            },
            None => self
                .t
                .last()
                .map(|t| Pos {
                    line: t.line,
                    col: t.col,
                })
                .unwrap_or(Pos { line: 1, col: 1 }),
        }
    }

    fn bump(&mut self) {
        if !self.at_eof() {
            self.p += 1;
        }
    }

    fn at_op(&self, s: &str) -> bool {
        matches!(self.kind(), Some(TokenKind::Op(o)) if *o == s)
    }

    fn at_kw(&self, s: &str) -> bool {
        matches!(self.kind(), Some(TokenKind::Kw(k)) if *k == s)
    }

    fn at_nl(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::Newline))
    }

    fn eat_op(&mut self, s: &str) -> bool {
        if self.at_op(s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, s: &str) -> bool {
        if self.at_kw(s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn diag_here(&mut self, msg: String) {
        let pos = self.pos();
        self.diags.push(Diagnostic {
            line: pos.line,
            col: pos.col,
            message: msg,
        });
    }

    fn expect_op(&mut self, s: &str) -> bool {
        if self.eat_op(s) {
            true
        } else {
            self.diag_here(format!("'{}' প্রত্যাশিত", s));
            false
        }
    }

    /// Like `expect_ident`, but also accepts a primitive-type keyword, so a
    /// stdlib module may share a type's name (`ইম্পোর্ট লেখা`).
    fn expect_module_name(&mut self, what: &str) -> Option<Ident> {
        if let Some(TokenKind::Kw(k)) = self.kind() {
            if PRIMITIVE_TYPES.contains(k) {
                let name = k.to_string();
                let pos = self.pos();
                self.bump();
                return Some(Ident { name, pos });
            }
        }
        self.expect_ident(what)
    }

    fn expect_ident(&mut self, what: &str) -> Option<Ident> {
        if let Some(TokenKind::Ident(name)) = self.kind() {
            let name = name.clone();
            let pos = self.pos();
            self.bump();
            Some(Ident { name, pos })
        } else {
            self.diag_here(format!("{} প্রত্যাশিত", what));
            None
        }
    }

    fn skip_nl(&mut self) {
        while self.at_nl() {
            self.bump();
        }
    }

    fn recover_stmt(&mut self) {
        loop {
            if self.at_eof() || self.at_op("}") {
                return;
            }
            if self.at_nl() {
                self.bump();
                return;
            }
            if matches!(self.kind(), Some(TokenKind::Kw(k)) if SYNC_KEYWORDS.contains(k)) {
                return;
            }
            self.bump();
        }
    }

    fn recover_top(&mut self) {
        loop {
            if self.at_eof() {
                return;
            }
            if matches!(self.kind(), Some(TokenKind::Kw(k)) if SYNC_KEYWORDS.contains(k)) {
                return;
            }
            self.bump();
        }
    }

    fn end_stmt(&mut self) {
        if self.eat_nl_or_close() {
            return;
        }
        self.diag_here("স্টেটমেন্ট শেষে নতুন লাইন প্রত্যাশিত".to_string());
        self.recover_stmt();
    }

    fn eat_nl_or_close(&mut self) -> bool {
        if self.at_nl() {
            self.bump();
            true
        } else {
            self.at_eof() || self.at_op("}")
        }
    }

    fn parse_program(&mut self) -> Program {
        let mut prog = Program {
            imports: Vec::new(),
            structs: Vec::new(),
            funcs: Vec::new(),
            consts: Vec::new(),
            app: None,
        };
        self.skip_nl();
        while !self.at_eof() {
            if self.at_kw("ইম্পোর্ট") {
                self.bump();
                if let Some(id) = self.expect_module_name("'ইম্পোর্ট'-এর পরে মডিউলের নাম") {
                    prog.imports.push(id);
                }
                self.end_stmt();
                self.skip_nl();
            } else if self.at_kw("অ্যাপ") {
                if prog.app.is_some() {
                    self.diag_here("একাধিক 'অ্যাপ' ডিক্লারেশন".to_string());
                }
                prog.app = Some(self.parse_app());
                self.skip_nl();
                if !self.at_eof() {
                    self.diag_here("'অ্যাপ'-এর পরে শুধু নতুন লাইন থাকতে পারে".to_string());
                    self.recover_top();
                    self.skip_nl();
                }
            } else if self.at_kw("তথ্য") {
                if let Some(s) = self.parse_struct_decl() {
                    prog.structs.push(s);
                }
                self.skip_nl();
            } else if self.at_kw("ফাংশন") {
                let f = self.parse_fn();
                prog.funcs.push(Rc::new(f));
                self.skip_nl();
            } else if self.at_kw("ধ্রুবক") {
                self.bump();
                if let Some(c) = self.parse_const_tail() {
                    prog.consts.push(c);
                }
                self.end_stmt();
                self.skip_nl();
            } else {
                self.diag_here(
                    "শীর্ষ-স্তরে শুধু 'ইম্পোর্ট', 'ফাংশন', 'ধ্রুবক' বা 'অ্যাপ' থাকতে পারে".to_string(),
                );
                self.recover_top();
                self.skip_nl();
            }
        }
        prog
    }

    fn parse_app(&mut self) -> AppDecl {
        self.bump();
        let name = if matches!(self.kind(), Some(TokenKind::Ident(_))) {
            self.expect_ident("'অ্যাপ'-এর নাম")
        } else {
            None
        };
        let body = self.parse_app_block();
        AppDecl { name, body }
    }

    fn parse_app_block(&mut self) -> Block {
        let mut stmts = Vec::new();
        if !self.expect_op("{") {
            return Block { stmts };
        }
        loop {
            self.skip_nl();
            if self.at_op("}") {
                self.bump();
                break;
            }
            if self.at_eof() {
                self.diag_here("ব্লক বন্ধ হয়নি — '}' পাওয়া যায়নি".to_string());
                break;
            }
            if self.at_kw("ডিসপ্লে") {
                self.bump();
                let blk = self.parse_block();
                stmts.push(Stmt::Display(blk));
                continue;
            }
            let s = self.parse_stmt();
            stmts.push(s);
        }
        Block { stmts }
    }

    fn parse_struct_decl(&mut self) -> Option<StructDecl> {
        self.bump();
        let name = self.expect_ident("'তথ্য'-এর পরে নাম")?;
        if !self.expect_op("{") {
            return None;
        }
        let mut fields = Vec::new();
        loop {
            self.skip_nl();
            if self.at_op("}") {
                self.bump();
                break;
            }
            if self.at_eof() {
                self.diag_here("তথ্য বন্ধ হয়নি — '}' পাওয়া যায়নি".to_string());
                break;
            }
            let fname = self.expect_ident("ফিল্ডের নাম")?;
            if !self.eat_op(":") {
                self.diag_here("':' প্রত্যাশিত — ফিল্ড টাইপ আবশ্যক".to_string());
                return None;
            }
            let fty = self.parse_type();
            fields.push((fname, fty));
            if !self.eat_op(",") {
                self.skip_nl();
                if self.at_op("}") {
                    self.bump();
                    break;
                }
            }
        }
        Some(StructDecl { name, fields })
    }

    fn parse_fn(&mut self) -> FuncDecl {
        self.bump();
        let name = self
            .expect_ident("'ফাংশন'-এর পরে নাম")
            .unwrap_or_else(|| Ident {
                name: "ত্রুটি".to_string(),
                pos: self.pos(),
            });
        let mut params = Vec::new();
        if self.expect_op("(") {
            if !self.at_op(")") {
                loop {
                    let ty = self.parse_type();
                    let pname = self
                        .expect_ident("প্যারামিটারের নাম")
                        .unwrap_or_else(|| Ident {
                            name: "ত্রুটি".to_string(),
                            pos: self.pos(),
                        });
                    params.push(Param { ty, name: pname });
                    if !self.eat_op(",") {
                        break;
                    }
                }
            }
            self.expect_op(")");
        }
        if !self.at_op("->") && !self.at_op("{") {
            self.diag_here("'->' প্রত্যাশিত — ফাংশনের রিটার্ন টাইপ আবশ্যক".to_string());
        }
        self.eat_op("->");
        let ret = self.parse_type();
        self.fn_depth += 1;
        let body = self.parse_block();
        self.fn_depth -= 1;
        FuncDecl {
            name,
            params,
            ret,
            body,
        }
    }

    fn parse_const_tail(&mut self) -> Option<ConstDecl> {
        let name = self.expect_ident("'ধ্রুবক'-এর পরে নাম")?;
        if !self.eat_op(":") {
            self.diag_here("':' প্রত্যাশিত — ধ্রুবকে টাইপ অ্যানোটেশন আবশ্যক".to_string());
            while !self.eat_nl_or_close() && !self.at_eof() {
                self.bump();
            }
            return None;
        }
        let ty = self.parse_type();
        if !self.eat_op("=") {
            self.diag_here("'=' প্রত্যাশিত".to_string());
            while !self.eat_nl_or_close() && !self.at_eof() {
                self.bump();
            }
            return None;
        }
        let init = self.parse_expr();
        Some(ConstDecl { name, ty, init })
    }

    fn parse_block(&mut self) -> Block {
        let mut stmts = Vec::new();
        if !self.expect_op("{") {
            return Block { stmts };
        }
        loop {
            self.skip_nl();
            if self.at_op("}") {
                self.bump();
                break;
            }
            if self.at_eof() {
                self.diag_here("ব্লক বন্ধ হয়নি — '}' পাওয়া যায়নি".to_string());
                break;
            }
            let s = self.parse_stmt();
            stmts.push(s);
        }
        Block { stmts }
    }

    fn parse_stmt(&mut self) -> Stmt {
        let out = self.parse_stmt_inner();
        out
    }

    fn parse_stmt_inner(&mut self) -> Stmt {
        if self.at_kw("ধরি") {
            self.bump();
            let name = match self.expect_ident("'ধরি'-এর পরে ভ্যারিয়েবলের নাম") {
                Some(n) => n,
                None => {
                    self.recover_stmt();
                    return Stmt::Nested(Block { stmts: vec![] });
                }
            };
            let ty = if self.eat_op(":") {
                Some(self.parse_type())
            } else {
                None
            };
            if !self.eat_op("=") {
                self.diag_here("'=' প্রত্যাশিত — 'ধরি'-তে ইনিশিয়ালাইজার আবশ্যক".to_string());
                self.recover_stmt();
                return Stmt::Nested(Block { stmts: vec![] });
            }
            let init = self.parse_expr();
            self.end_stmt();
            return Stmt::Var(VarDecl { name, ty, init });
        }
        if self.at_kw("ধ্রুবক") {
            self.bump();
            match self.parse_const_tail() {
                Some(c) => {
                    self.end_stmt();
                    Stmt::Const(c)
                }
                None => Stmt::Nested(Block { stmts: vec![] }),
            }
        } else if self.at_kw("যদি") {
            let s = self.parse_if();
            Stmt::If(s)
        } else if self.at_kw("লুপ") {
            let pos = self.pos();
            self.bump();
            self.expect_op("(");
            let count = self.parse_expr();
            self.expect_op(")");
            self.loop_depth += 1;
            let body = self.parse_block();
            self.loop_depth -= 1;
            self.end_stmt();
            Stmt::Loop(LoopStmt { pos, count, body })
        } else if self.at_kw("যতক্ষণ") {
            let pos = self.pos();
            self.bump();
            self.expect_op("(");
            let cond = self.parse_expr();
            self.expect_op(")");
            self.loop_depth += 1;
            let body = self.parse_block();
            self.loop_depth -= 1;
            self.end_stmt();
            Stmt::While(WhileStmt { pos, cond, body })
        } else if self.at_kw("প্রতি") {
            let pos = self.pos();
            self.bump();
            self.expect_op("(");
            let var = self
                .expect_ident("'প্রতি'-এর পরে লুপ ভ্যারিয়েবল")
                .unwrap_or_else(|| Ident {
                    name: "ত্রুটি".to_string(),
                    pos: self.pos(),
                });
            if !self.eat_op(":") {
                self.diag_here("':' প্রত্যাশিত — 'প্রতি (ভ্যারিয়েবল : তালিকা)'".to_string());
            }
            let iter = self.parse_expr();
            self.expect_op(")");
            self.loop_depth += 1;
            let body = self.parse_block();
            self.loop_depth -= 1;
            self.end_stmt();
            Stmt::ForEach(ForEachStmt {
                pos,
                var,
                iter,
                body,
            })
        } else if self.at_kw("রিটার্ন") {
            let pos = self.pos();
            if self.fn_depth == 0 {
                self.diag_here("'রিটার্ন' শুধু ফাংশনের ভেতরে বৈধ".to_string());
            }
            self.bump();
            let value = if self.starts_expr() {
                Some(self.parse_expr())
            } else {
                None
            };
            self.end_stmt();
            Stmt::Return(ReturnStmt { pos, value })
        } else if self.at_kw("বিরতি") {
            let pos = self.pos();
            if self.loop_depth == 0 {
                self.diag_here("'বিরতি' শুধু লুপের ভেতরে বৈধ".to_string());
            }
            self.bump();
            self.end_stmt();
            Stmt::Break(pos)
        } else if self.at_kw("চলবে") {
            let pos = self.pos();
            if self.loop_depth == 0 {
                self.diag_here("'চলবে' শুধু লুপের ভেতরে বৈধ".to_string());
            }
            self.bump();
            self.end_stmt();
            Stmt::Continue(pos)
        } else if self.at_kw("চেষ্টা") {
            self.bump();
            let body = self.parse_block();
            if !self.eat_kw("ধরো") {
                self.diag_here("'চেষ্টা'-এর পরে 'ধরো' আবশ্যক".to_string());
                return Stmt::Nested(Block { stmts: vec![] });
            }
            self.expect_op("(");
            let err_var = match self.expect_ident("ত্রুটি ভ্যারিয়েবলের নাম") {
                Some(n) => n,
                None => return Stmt::Nested(Block { stmts: vec![] }),
            };
            self.expect_op(")");
            let handler = self.parse_block();
            self.end_stmt();
            return Stmt::TryCatch(TryCatchStmt { body, err_var, handler });
        } else if self.at_op("{") {
            Stmt::Nested(self.parse_block())
        } else if let Some(TokenKind::Kw(k)) = self.kind().cloned() {
            if WIDGETS.contains(&k) {
                let w = self.parse_widget(k);
                return Stmt::Widget(w);
            }
            let e = self.parse_expr();
            self.end_stmt();
            Stmt::Expr(e)
        } else {
            let e = self.parse_expr();
            self.end_stmt();
            Stmt::Expr(e)
        }
    }

    fn parse_widget(&mut self, kw: &'static str) -> WidgetNode {
        let pos = self.pos();
        self.bump();
        let mut args = Vec::new();
        if self.expect_op("(") {
            if !self.at_op(")") {
                loop {
                    args.push(self.parse_expr());
                    if !self.eat_op(",") {
                        break;
                    }
                }
            }
            self.expect_op(")");
        }
        let body = if CONTAINER_WIDGETS.contains(&kw) {
            Some(self.parse_block())
        } else {
            None
        };
        self.end_stmt();
        WidgetNode {
            kw: kw.to_string(),
            pos,
            args,
            body,
        }
    }

    fn parse_if(&mut self) -> IfStmt {
        let pos = self.pos();
        self.bump();
        self.expect_op("(");
        let cond = self.parse_expr();
        self.expect_op(")");
        let then = self.parse_block();
        let els = if self.eat_kw("নাহলে") {
            if self.at_kw("যদি") {
                Some(ElseBranch::If(Box::new(self.parse_if())))
        } else if self.at_op("{") {
                Some(ElseBranch::Block(self.parse_block()))
            } else {
                self.diag_here("'নাহলে'-এর পরে 'যদি' বা '{' প্রত্যাশিত".to_string());
                None
            }
        } else {
            None
        };
        self.end_stmt();
        IfStmt {
            pos,
            cond,
            then,
            els,
        }
    }

    fn starts_expr(&self) -> bool {
        match self.kind() {
            Some(TokenKind::Ident(_))
            | Some(TokenKind::Num(_))
            | Some(TokenKind::Str(_))
            | Some(TokenKind::Chr(_)) => true,
            Some(TokenKind::Kw(k)) => matches!(*k, "সত্য" | "মিথ্যা" | "ফাঁকা" | "না"),
            Some(TokenKind::Op(o)) => matches!(*o, "(" | "[" | "-"),
            _ => false,
        }
    }

    fn parse_expr(&mut self) -> Expr {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Expr {
        if matches!(self.kind(), Some(TokenKind::Ident(_))) {
            let save = self.p;
            let base = self
                .expect_ident("ভ্যারিয়েবল")
                .unwrap_or_else(|| Ident {
                    name: "ত্রুটি".to_string(),
                    pos: self.pos(),
                });
            let mut idx = Vec::new();
            let mut field: Option<Ident> = None;
            while self.at_op("[") {
                self.bump();
                idx.push(self.parse_expr());
                self.expect_op("]");
            }
            if self.at_op(".") {
                self.bump();
                if let Some(TokenKind::Ident(fname)) = self.kind().cloned() {
                    let fpos = self.pos();
                    self.bump();
                    field = Some(Ident { name: fname, pos: fpos });
                }
            }
            if self.at_op("=") {
                let pos = base.pos;
                self.bump();
                let rhs = self.parse_assign();
                return Expr {
                    kind: ExprKind::Assign(LValue { base, idx, field }, Box::new(rhs)),
                    pos,
                };
            }
            for (op_str, bin_op) in [
                ("+=", BinOp::Add),
                ("-=", BinOp::Sub),
                ("*=", BinOp::Mul),
                ("/=", BinOp::Div),
            ] {
                if self.at_op(op_str) {
                    let pos = base.pos;
                    self.bump();
                    let rhs = self.parse_assign();
                    // desugar: x += rhs → x = x + rhs
                    let cur = Expr {
                        kind: ExprKind::Postfix(
                            Box::new(Expr {
                                kind: ExprKind::Ident(base.clone()),
                                pos,
                            }),
                            idx.iter()
                                .map(|e| Suffix::Index(Box::new(e.clone()), pos))
                                .collect(),
                        ),
                        pos,
                    };
                    let combined = Expr {
                        kind: ExprKind::Binary(bin_op, Box::new(cur), Box::new(rhs)),
                        pos,
                    };
                    return Expr {
                        kind: ExprKind::Assign(LValue { base, idx: Vec::new(), field: None }, Box::new(combined)),
                        pos,
                    };
                }
            }
            // Struct field assignment: p.field = value
            self.p = save;
            if matches!(self.kind(), Some(TokenKind::Ident(_))) {
                let base = self.expect_ident("ভ্যারিয়েবল").unwrap();
                if self.at_op(".") {
                    self.bump();
                    if let Some(TokenKind::Ident(fname_str)) = self.kind().cloned() {
                        let fpos = self.pos();
                        let fname = Ident { name: fname_str, pos: fpos };
                        self.bump();
                        if self.at_op("=") {
                            self.bump();
                            let rhs = self.parse_assign();
                            return Expr {
                                kind: ExprKind::FieldAssign(base.clone(), fname, Box::new(rhs)),
                                pos: base.pos,
                            };
                        }
                    }
                }
                self.p = save;
            }
        }
        self.parse_or()
    }

    fn binary_ladder(
        &mut self,
        lower: fn(&mut P) -> Expr,
        ops: &[(&'static str, BinOp)],
        kw_ops: &[(&'static str, BinOp)],
    ) -> Expr {
        let mut lhs = lower(self);
        loop {
            let mut found: Option<(BinOp, Pos)> = None;
            if let Some(TokenKind::Op(o)) = self.kind() {
                let o = *o;
                if let Some((_, bop)) = ops.iter().find(|(s, _)| *s == o) {
                    found = Some((*bop, self.pos()));
                }
            } else if let Some(TokenKind::Kw(k)) = self.kind() {
                let k = *k;
                if let Some((_, bop)) = kw_ops.iter().find(|(s, _)| *s == k) {
                    found = Some((*bop, self.pos()));
                }
            }
            match found {
                Some((bop, pos)) => {
                    self.bump();
                    let rhs = lower(self);
                    lhs = Expr {
                        kind: ExprKind::Binary(bop, Box::new(lhs), Box::new(rhs)),
                        pos,
                    };
                }
                None => break,
            }
        }
        lhs
    }

    fn parse_or(&mut self) -> Expr {
        self.binary_ladder(P::parse_and, &[], &[("অথবা", BinOp::Or)])
    }

    fn parse_and(&mut self) -> Expr {
        self.binary_ladder(P::parse_eq, &[], &[("এবং", BinOp::And)])
    }

    fn parse_eq(&mut self) -> Expr {
        self.binary_ladder(
            P::parse_rel,
            &[("==", BinOp::Eq), ("!=", BinOp::Neq)],
            &[],
        )
    }

    fn parse_rel(&mut self) -> Expr {
        self.binary_ladder(
            P::parse_add,
            &[
                ("<", BinOp::Lt),
                (">", BinOp::Gt),
                ("<=", BinOp::Le),
                (">=", BinOp::Ge),
            ],
            &[],
        )
    }

    fn parse_add(&mut self) -> Expr {
        self.binary_ladder(P::parse_mul, &[("+", BinOp::Add), ("-", BinOp::Sub)], &[])
    }

    fn parse_mul(&mut self) -> Expr {
        self.binary_ladder(
            P::parse_unary,
            &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Mod)],
            &[],
        )
    }

    fn parse_unary(&mut self) -> Expr {
        if self.at_op("-") {
            let pos = self.pos();
            self.bump();
            let e = self.parse_unary();
            return Expr {
                kind: ExprKind::Unary(UnaryOp::Neg, Box::new(e)),
                pos,
            };
        }
        if self.at_kw("না") {
            let pos = self.pos();
            self.bump();
            let e = self.parse_unary();
            return Expr {
                kind: ExprKind::Unary(UnaryOp::Not, Box::new(e)),
                pos,
            };
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Expr {
        let base = self.parse_primary();
        let mut sfx = Vec::new();
        loop {
            if self.at_op("(") {
                let pos = self.pos();
                self.bump();
                let mut args = Vec::new();
                if !self.at_op(")") {
                    loop {
                        args.push(self.parse_expr());
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                }
                self.expect_op(")");
                sfx.push(Suffix::Call(args, pos));
            } else if self.at_op("[") {
                let pos = self.pos();
                self.bump();
                let ix = self.parse_expr();
                self.expect_op("]");
                sfx.push(Suffix::Index(Box::new(ix), pos));
            } else if self.at_op(".") {
                self.bump();
                if let Some(TokenKind::Ident(fname)) = self.kind().cloned() {
                    let fpos = self.pos();
                    self.bump();
                    sfx.push(Suffix::Field(Ident { name: fname, pos: fpos }));
                } else {
                    self.diag_here("'.'-এর পরে ফিল্ডের নাম প্রত্যাশিত".to_string());
                    break;
                }
            } else {
                break;
            }
        }
        if sfx.is_empty() {
            base
        } else {
            let pos = base.pos;
            Expr {
                kind: ExprKind::Postfix(Box::new(base), sfx),
                pos,
            }
        }
    }

    fn parse_primary(&mut self) -> Expr {
        let pos = self.pos();
        match self.kind().cloned() {
            Some(TokenKind::Num(NumTok::Int(v))) => {
                self.bump();
                self.lit(Lit::Int(v), pos)
            }
            Some(TokenKind::Num(NumTok::Float(v))) => {
                self.bump();
                self.lit(Lit::Float(v), pos)
            }
            Some(TokenKind::Str(s)) => {
                self.bump();
                self.lit(Lit::Str(s), pos)
            }
            Some(TokenKind::Chr(c)) => {
                self.bump();
                self.lit(Lit::Char(c), pos)
            }
            Some(TokenKind::Kw("সত্য")) => {
                self.bump();
                self.lit(Lit::Bool(true), pos)
            }
            Some(TokenKind::Kw("মিথ্যা")) => {
                self.bump();
                self.lit(Lit::Bool(false), pos)
            }
            Some(TokenKind::Kw("ফাঁকা")) => {
                self.bump();
                self.lit(Lit::Null, pos)
            }
            Some(TokenKind::Ident(name)) => {
                self.bump();
                if self.at_op(".") {
                    self.bump();
                    let member = match self.kind().cloned() {
                        Some(TokenKind::Ident(n)) => Some((n, self.pos())),
                        Some(TokenKind::Kw(k)) if PRIMITIVE_TYPES.contains(&k) => {
                            Some((k.to_string(), self.pos()))
                        }
                        _ => None,
                    };
                    if let Some((name2, pos2)) = member {
                        self.bump();
                        return Expr {
                            kind: ExprKind::Qualified {
                                module: Ident { name, pos },
                                name: Ident {
                                    name: name2,
                                    pos: pos2,
                                },
                            },
                            pos,
                        };
                    }
                    self.diag_here("'.'-এর পরে নাম প্রত্যাশিত".to_string());
                    if !self.at_eof() {
                        self.bump();
                    }
                }
                Expr {
                    kind: ExprKind::Ident(Ident { name, pos }),
                    pos,
                }
            }
            // A primitive-type keyword in expression position is only ever
            // meaningful as a module qualifier — types are not values in
            // Kolom, so a bare `লেখা` can never be an expression on its own.
            // That makes `লেখা.বড়হাতের(...)` unambiguous, and lets the text
            // module share the name of the type it operates on.
            Some(TokenKind::Kw(k)) if PRIMITIVE_TYPES.contains(&k) => {
                self.bump();
                if self.at_op(".") {
                    self.bump();
                    let member = match self.kind().cloned() {
                        Some(TokenKind::Ident(n)) => Some((n, self.pos())),
                        Some(TokenKind::Kw(k2)) if PRIMITIVE_TYPES.contains(&k2) => {
                            Some((k2.to_string(), self.pos()))
                        }
                        _ => None,
                    };
                    if let Some((name2, pos2)) = member {
                        self.bump();
                        return Expr {
                            kind: ExprKind::Qualified {
                                module: Ident { name: k.to_string(), pos },
                                name: Ident { name: name2, pos: pos2 },
                            },
                            pos,
                        };
                    }
                    self.diag_here("'.'-এর পরে নাম প্রত্যাশিত".to_string());
                    if !self.at_eof() {
                        self.bump();
                    }
                } else {
                    self.diag_here(format!("'{}' একটি টাইপের নাম — এখানে মান প্রত্যাশিত", k));
                }
                self.lit(Lit::Null, pos)
            }
            Some(TokenKind::Op("(")) => {
                self.bump();
                let e = self.parse_expr();
                self.expect_op(")");
                e
            }
            Some(TokenKind::Op("[")) => {
                self.bump();
                let mut items = Vec::new();
                if !self.at_op("]") {
                    loop {
                        items.push(self.parse_expr());
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                }
                self.expect_op("]");
                self.lit(Lit::Array(items), pos)
            }
            _ => {
                self.diag_here("এক্সপ্রেশন প্রত্যাশিত".to_string());
                if !self.at_eof() {
                    self.bump();
                }
                self.lit(Lit::Null, pos)
            }
        }
    }

    fn lit(&mut self, l: Lit, pos: Pos) -> Expr {
        Expr {
            kind: ExprKind::Lit(l),
            pos,
        }
    }

    fn parse_type(&mut self) -> TypeExpr {
        let base = if self.at_kw("শেয়ার") {
            self.bump();
            TypeExpr::Shared(Box::new(self.parse_type_base()))
        } else {
            self.parse_type_base()
        };
        let mut ty = base;
        while self.at_op("[") {
            if matches!(self.t.get(self.p + 1).map(|t| &t.kind), Some(TokenKind::Op("]"))) {
                self.bump();
                self.bump();
                ty = TypeExpr::Array(Box::new(ty));
            } else {
                break;
            }
        }
        ty
    }

    fn parse_type_base(&mut self) -> TypeExpr {
        let pos = self.pos();
        match self.kind().cloned() {
            Some(TokenKind::Kw(k)) if k == "ম্যাপ" => {
                self.bump();
                if !self.expect_op("[") {
                    return TypeExpr::Named(Ident { name: "ম্যাপ".into(), pos });
                }
                let key_ty = self.parse_type();
                if !self.expect_op(",") {
                    return TypeExpr::Named(Ident { name: "ম্যাপ".into(), pos });
                }
                let val_ty = self.parse_type();
                self.expect_op("]");
                TypeExpr::Map(Box::new(key_ty), Box::new(val_ty))
            }
            Some(TokenKind::Kw(k)) if PRIMITIVE_TYPES.contains(&k) => {
                self.bump();
                TypeExpr::Named(Ident { name: k.to_string(), pos })
            }
            Some(TokenKind::Ident(name)) => {
                self.bump();
                TypeExpr::Named(Ident { name, pos })
            }
            _ => {
                self.diag_here("টাইপ প্রত্যাশিত".to_string());
                if !self.at_eof() {
                    self.bump();
                }
                TypeExpr::Named(Ident { name: "ত্রুটি".into(), pos })
            }
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> (Program, Vec<Diagnostic>) {
    if tokens.is_empty() {
        return (
            Program {
                imports: Vec::new(),
                structs: Vec::new(),
                funcs: Vec::new(),
                consts: Vec::new(),
                app: None,
            },
            Vec::new(),
        );
    }
    let mut p = P {
        t: tokens,
        p: 0,
        diags: Vec::new(),
        fn_depth: 0,
        loop_depth: 0,
    };
    let prog = p.parse_program();
    (prog, p.diags)
}
