use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;

use kolom_syntax::ast::*;

use kolom_lexer::bn_num;

#[derive(Debug)]
pub struct InterpError {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

fn err(pos: Pos, message: impl Into<String>) -> InterpError {
    InterpError {
        line: pos.line,
        col: pos.col,
        message: message.into(),
    }
}

#[derive(Clone)]
pub enum Value {
    Num(i64),
    Dec(f64),
    Txt(String),
    Bool(bool),
    Ch(char),
    Null,
    Arr(Rc<RefCell<Vec<Value>>>),
    Shared(Rc<RefCell<Value>>),
    Map(Rc<RefCell<HashMap<String, Value>>>),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Num(n) => write!(f, "{}", n),
            Value::Dec(d) => write!(f, "{}", d),
            Value::Txt(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", if *b { "সত্য" } else { "মিথ্যা" }),
            Value::Ch(c) => write!(f, "{}", c),
            Value::Null => write!(f, "ফাঁকা"),
            Value::Arr(a) => {
                write!(f, "[")?;
                let v = a.borrow();
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, "]")
            }
            Value::Shared(s) => write!(f, "{}", s.borrow()),
            Value::Map(m) => {
                write!(f, "{{")?;
                let v = m.borrow();
                for (i, (k, val)) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\": {}", k, val)?;
                }
                write!(f, "}}")
            }
        }
    }
}

enum CF {
    Normal,
    Break,
    Continue,
    Ret(Value),
}

struct Scope {
    vars: HashMap<String, Value>,
    consts: HashSet<String>,
}

impl Scope {
    fn new() -> Scope {
        Scope {
            vars: HashMap::new(),
            consts: HashSet::new(),
        }
    }
}

struct Interp<'o> {
    funcs: HashMap<String, Rc<FuncDecl>>,
    scopes: Vec<Scope>,
    out: &'o mut dyn Write,
    rng: u64,
    net: HashMap<u32, std::net::TcpStream>,
    net_next: u32,
    structs: HashMap<String, Vec<String>>,
    argv: Vec<String>,
}

pub fn run(prog: &Program, out: &mut dyn Write) -> Result<(), InterpError> {
    run_with_argv(prog, out, Vec::new())
}

/// As `run`, but with `সিস্টেম.আর্গুমেন্ট()`'s value supplied explicitly —
/// used by `কলম চালাও file.ক arg1 arg2`, where `argv` is everything after
/// the script path. Every other caller (the `পাতা` editor's run-in-place,
/// the golden-test harness) has no such arguments, hence the plain `run`
/// wrapper defaulting to an empty vector.
pub fn run_with_argv(prog: &Program, out: &mut dyn Write, argv: Vec<String>) -> Result<(), InterpError> {
    let seed_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0x9E3779B97F4A7C15);
    let mut it = Interp {
        funcs: HashMap::new(),
        scopes: vec![Scope::new()],
        out,
        rng: 0x2545F4914F6CDD1D ^ seed_ms,
        net: HashMap::new(),
        net_next: 1,
        structs: HashMap::new(),
        argv,
    };
    for s in &prog.structs {
        let field_names: Vec<String> = s.fields.iter().map(|(n, _)| n.name.clone()).collect();
        it.structs.insert(s.name.name.clone(), field_names);
    }
    for f in &prog.funcs {
        if it
            .funcs
            .insert(f.name.name.clone(), Rc::clone(f))
            .is_some()
        {
            return Err(err(
                f.name.pos,
                format!("দুটি ফাংশনের একই নাম — '{}'", f.name.name),
            ));
        }
    }
    for c in &prog.consts {
        let v = it.eval(&c.init)?;
        it.define(&c.name.name, v, true, c.name.pos)?;
    }
    let app = match &prog.app {
        Some(a) => a,
        None => {
            return Err(err(
                Pos { line: 1, col: 1 },
                "কোনো 'অ্যাপ' ডিক্লারেশন পাওয়া যায়নি",
            ))
        }
    };
    it.scopes.push(Scope::new());
    let cf = it.exec_block(&app.body)?;
    it.scopes.pop();
    match cf {
        CF::Ret(_) => Err(err(Pos { line: 1, col: 1 }, "ফাংশনের বাইরে 'রিটার্ন'")),
        _ => Ok(()),
    }
}

impl<'o> Interp<'o> {
    fn scope(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }

    fn define(&mut self, name: &str, val: Value, is_const: bool, pos: Pos) -> Result<(), InterpError> {
        let s = self.scope();
        if s.vars.contains_key(name) {
            return Err(err(pos, format!("'{}' এই স্কোপে আগেই ঘোষিত", name)));
        }
        if is_const {
            s.consts.insert(name.to_string());
        }
        s.vars.insert(name.to_string(), val);
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        for s in self.scopes.iter().rev() {
            if let Some(v) = s.vars.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn assign_var(&mut self, name: &str, val: Value, pos: Pos) -> Result<(), InterpError> {
        for s in self.scopes.iter_mut().rev() {
            if s.vars.contains_key(name) {
                if s.consts.contains(name) {
                    return Err(err(pos, format!("ধ্রুবক '{}'-এ মান নির্ধারণ করা যাবে না", name)));
                }
                s.vars.insert(name.to_string(), val);
                return Ok(());
            }
        }
        Err(err(pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", name)))
    }

    fn exec_block(&mut self, b: &Block) -> Result<CF, InterpError> {
        self.scopes.push(Scope::new());
        for st in &b.stmts {
            let cf = self.exec_stmt(st)?;
            match cf {
                CF::Normal => {}
                other => {
                    self.scopes.pop();
                    return Ok(other);
                }
            }
        }
        self.scopes.pop();
        Ok(CF::Normal)
    }

    fn exec_stmt(&mut self, s: &Stmt) -> Result<CF, InterpError> {
        match s {
            Stmt::Var(v) => {
                let val = self.eval(&v.init)?;
                self.define(&v.name.name, val, false, v.name.pos)?;
                Ok(CF::Normal)
            }
            Stmt::Const(c) => {
                let val = self.eval(&c.init)?;
                self.define(&c.name.name, val, true, c.name.pos)?;
                Ok(CF::Normal)
            }
            Stmt::If(i) => {
                let cond = self.eval(&i.cond)?;
                let b = match cond {
                    Value::Bool(b) => b,
                    _ => {
                        return Err(err(
                            i.pos,
                            "'যদি'-এর শর্ত 'বুলিয়ান' টাইপের হতে হবে",
                        ))
                    }
                };
                if b {
                    self.exec_block(&i.then)
                } else {
                    match &i.els {
                        Some(ElseBranch::If(inner)) => self.exec_stmt(&Stmt::If((**inner).clone())),
                        Some(ElseBranch::Block(blk)) => self.exec_block(blk),
                        None => Ok(CF::Normal),
                    }
                }
            }
            Stmt::Loop(l) => {
                let n = match self.eval(&l.count)? {
                    Value::Num(n) => n,
                    _ => {
                        return Err(err(
                            l.pos,
                            "'লুপ'-এর সংখ্যা 'সংখ্যা' টাইপের হতে হবে",
                        ))
                    }
                };
                if n < 0 {
                    return Err(err(l.pos, "'লুপ'-এর সংখ্যা ঋণাত্মক হতে পারে না"));
                }
                for _ in 0..n {
                    let cf = self.exec_block(&l.body)?;
                    match cf {
                        CF::Break => break,
                        CF::Continue => continue,
                        CF::Ret(v) => return Ok(CF::Ret(v)),
                        CF::Normal => {}
                    }
                }
                Ok(CF::Normal)
            }
            Stmt::While(w) => {
                loop {
                    let cond = self.eval(&w.cond)?;
                    match cond {
                        Value::Bool(true) => {}
                        Value::Bool(false) => break,
                        _ => {
                            return Err(err(
                                w.pos,
                                "'যতক্ষণ'-এর শর্ত 'বুলিয়ান' টাইপের হতে হবে",
                            ))
                        }
                    }
                    let cf = self.exec_block(&w.body)?;
                    match cf {
                        CF::Break => break,
                        CF::Continue => continue,
                        CF::Ret(v) => return Ok(CF::Ret(v)),
                        CF::Normal => {}
                    }
                }
                Ok(CF::Normal)
            }
            Stmt::ForEach(fe) => {
                let iter_val = self.eval(&fe.iter)?;
                let arr = match iter_val {
                    Value::Arr(a) => a,
                    _ => return Err(err(fe.pos, "'প্রতি'-তে শুধু অ্যারে চলবে")),
                };
                let items: Vec<Value> = arr.borrow().clone();
                for v in items {
                    let mut sc = Scope::new();
                    sc.vars.insert(fe.var.name.clone(), v);
                    self.scopes.push(sc);
                    let cf = self.exec_block(&fe.body)?;
                    self.scopes.pop();
                    match cf {
                        CF::Break => break,
                        CF::Continue => continue,
                        CF::Ret(rv) => return Ok(CF::Ret(rv)),
                        CF::Normal => {}
                    }
                }
                Ok(CF::Normal)
            }
            Stmt::Return(r) => {
                let v = match &r.value {
                    Some(e) => self.eval(e)?,
                    None => Value::Null,
                };
                Ok(CF::Ret(v))
            }
            Stmt::Break(_) => Ok(CF::Break),
            Stmt::Continue(_) => Ok(CF::Continue),
            Stmt::Expr(e) => {
                self.eval(e)?;
                Ok(CF::Normal)
            }
            Stmt::Nested(b) => self.exec_block(b),
            Stmt::Widget(w) => {
                match w.kw.as_str() {
                    "ইনপুট" | "ক্যানভাস" | "ছবি" => Ok(CF::Normal),
                    _ => {
                        for a in &w.args {
                            let _ = self.eval(a)?;
                        }
                        if let Some(b) = &w.body {
                            self.exec_block(b)?;
                        }
                        Ok(CF::Normal)
                    }
                }
            }
            Stmt::TryCatch(tc) => {
                let saved = self.scopes.len();
                match self.exec_block(&tc.body) {
                    Ok(cf) => {
                        self.scopes.truncate(saved);
                        Ok(cf)
                    }
                    Err(e) => {
                        self.scopes.truncate(saved);
                        let mut sc = Scope::new();
                        sc.vars.insert(
                            tc.err_var.name.clone(),
                            Value::Txt(e.message.clone()),
                        );
                        self.scopes.push(sc);
                        let result = self.exec_block(&tc.handler);
                        self.scopes.pop();
                        result
                    }
                }
            }
            Stmt::Display(_) => Ok(CF::Normal),
        }
    }

    fn eval(&mut self, e: &Expr) -> Result<Value, InterpError> {
        match &e.kind {
            ExprKind::Lit(l) => Ok(match l {
                Lit::Int(v) => Value::Num(*v),
                Lit::Float(v) => Value::Dec(*v),
                Lit::Str(s) => Value::Txt(s.clone()),
                Lit::Char(c) => Value::Ch(*c),
                Lit::Bool(b) => Value::Bool(*b),
                Lit::Null => Value::Null,
                Lit::Array(items) => {
                    let mut vals = Vec::with_capacity(items.len());
                    for it in items {
                        vals.push(self.eval(it)?);
                    }
                    Value::Arr(Rc::new(RefCell::new(vals)))
                }
            }),
            ExprKind::Ident(id) => self
                .lookup(&id.name)
                .ok_or_else(|| err(id.pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", id.name))),
            ExprKind::Qualified { module, name } => {
                // Check if module is actually a local struct variable → field access
                if let Some(v) = self.lookup(&module.name) {
                    match v {
                        Value::Map(m) => {
                            let b = m.borrow();
                            match b.get(&name.name) {
                                Some(val) => return Ok(val.clone()),
                                None => return Err(err(
                                    name.pos,
                                    format!("'{}' ফিল্ড নেই", name.name),
                                )),
                            }
                        }
                        _ => {}
                    }
                }
                match (module.name.as_str(), name.name.as_str()) {
                    ("গণিত", "পাই") => Ok(Value::Dec(std::f64::consts::PI)),
                    ("গণিত", "ই") => Ok(Value::Dec(std::f64::consts::E)),
                    _ => Err(err(
                        name.pos,
                        format!(
                            "'{}.{}' একটি ফাংশন — কল করে ব্যবহার করুন",
                            module.name, name.name
                        ),
                    )),
                }
            }
            ExprKind::Unary(op, inner) => {
                let v = self.eval(inner)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Num(n) => n
                            .checked_neg()
                            .map(Value::Num)
                            .ok_or_else(|| err(e.pos, "পূর্ণসংখ্যা ওভারফ্লো")),
                        Value::Dec(d) => Ok(Value::Dec(-d)),
                        _ => Err(err(e.pos, "ইউনারি '-'-এর অপারেন্ড সংখ্যা হতে হবে")),
                    },
                    UnaryOp::Not => match v {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        _ => Err(err(
                            e.pos,
                            "'না'-এর অপারেন্ড 'বুলিয়ান' টাইপের হতে হবে",
                        )),
                    },
                }
            }
            ExprKind::Binary(op, l, r) => {
                if *op == BinOp::And || *op == BinOp::Or {
                    let lv = self.eval(l)?;
                    let lb = match lv {
                        Value::Bool(b) => b,
                        _ => {
                            return Err(err(
                                e.pos,
                                "লজিক্যাল অপারেটরের অপারেন্ড 'বুলিয়ান' টাইপের হতে হবে",
                            ))
                        }
                    };
                    if *op == BinOp::And && !lb {
                        return Ok(Value::Bool(false));
                    }
                    if *op == BinOp::Or && lb {
                        return Ok(Value::Bool(true));
                    }
                    let rv = self.eval(r)?;
                    return match rv {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        _ => Err(err(
                            e.pos,
                            "লজিক্যাল অপারেটরের অপারেন্ড 'বুলিয়ান' টাইপের হতে হবে",
                        )),
                    };
                }
                let lv = self.eval(l)?;
                let rv = self.eval(r)?;
                self.binary(*op, lv, rv, e.pos)
            }
            ExprKind::Assign(target, rhs) => {
                let v = self.eval(rhs)?;
                self.set_lvalue(target, v)?;
                Ok(Value::Null)
            }
            ExprKind::FieldAssign(base, field, rhs) => {
                let v = self.eval(rhs)?;
                let name = &base.name;
                match self.lookup(name) {
                    Some(Value::Map(m)) => {
                        m.borrow_mut().insert(field.name.clone(), v);
                        Ok(Value::Null)
                    }
                    Some(_) => Err(err(base.pos, format!("'{}' তথ্য নয়", name))),
                    None => Err(err(base.pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", name))),
                }
            }
            ExprKind::Postfix(base, sfx) => {
                let mut name: Option<(String, Pos)> = match &base.kind {
                    ExprKind::Ident(id) => Some((id.name.clone(), id.pos)),
                    ExprKind::Qualified { module, name } => {
                        Some((format!("{}::{}", module.name, name.name), module.pos))
                    }
                    _ => None,
                };
                let mut val: Option<Value> = None;
                // `a.b` is a module item only when `a` is not a local struct
                // variable; otherwise it is a field read that further
                // suffixes chain onto (`ব.ভি.মান`). Evaluate it as a value
                // and clear the callable name so the loop treats it as one.
                if let ExprKind::Qualified { module, name: fname } = &base.kind {
                    if let Some(Value::Map(m)) = self.lookup(&module.name) {
                        if let Some(v) = m.borrow().get(&fname.name) {
                            val = Some(v.clone());
                            name = None;
                        }
                    }
                }
                for s in sfx {
                    match s {
                        Suffix::Call(args, cpos) => {
                            let (nm, npos) = match name.take() {
                                Some(x) => x,
                                None => {
                                    return Err(err(*cpos, "এটি কলযোগ্য ফাংশন নয়"))
                                }
                            };
                            val = Some(self.call(&nm, npos, args)?);
                        }
                        Suffix::Index(ix, ipos) => {
                            let cur = match val.take() {
                                Some(v) => v,
                                None => {
                                    let (nm, _) = name
                                        .take()
                                        .ok_or_else(|| err(*ipos, "এটি ইনডেক্সযোগ্য নয়"))?;
                                    self.lookup(&nm).ok_or_else(|| {
                                        err(*ipos, format!("অঘোষিত ভ্যারিয়েবল '{}'", nm))
                                    })?
                                }
                            };
                            let iv = self.eval(ix)?;
                            val = Some(self.index_one(cur, iv, *ipos)?);
                        }
                        Suffix::Field(fname) => {
                            let cur = match val.take() {
                                Some(v) => v,
                                None => {
                                    let (nm, _) = name
                                        .take()
                                        .ok_or_else(|| err(fname.pos, "ফিল্ড অ্যাক্সেস অবৈধ"))?;
                                    self.lookup(&nm).ok_or_else(|| {
                                        err(fname.pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", nm))
                                    })?
                                }
                            };
                            match cur {
                                Value::Map(m) => {
                                    let b = m.borrow();
                                    match b.get(&fname.name) {
                                        Some(v) => val = Some(v.clone()),
                                        None => return Err(err(
                                            fname.pos,
                                            format!("'{}' ফিল্ড নেই", fname.name),
                                        )),
                                    }
                                }
                                _ => return Err(err(
                                    fname.pos,
                                    format!("'{}' টাইপের উপর ফিল্ড অ্যাক্সেস করা যায় না", cur),
                                )),
                            }
                        }
                    }
                }
                if let Some((nm, npos)) = name {
                    if val.is_none() {
                        return self.lookup(&nm).ok_or_else(|| {
                            err(npos, format!("অঘোষিত ভ্যারিয়েবল '{}'", nm))
                        });
                    }
                }
                Ok(val.unwrap_or(Value::Null))
            }
        }
    }

    fn binary(&mut self, op: BinOp, l: Value, r: Value, pos: Pos) -> Result<Value, InterpError> {
        use BinOp::*;
        match op {
            Add => {
                if let (Value::Txt(a), Value::Txt(b)) = (&l, &r) {
                    return Ok(Value::Txt(format!("{}{}", a, b)));
                }
                if let (Value::Arr(a), Value::Arr(b)) = (&l, &r) {
                    let nv: Vec<Value> = a
                        .borrow()
                        .iter()
                        .cloned()
                        .chain(b.borrow().iter().cloned())
                        .collect();
                    return Ok(Value::Arr(Rc::new(RefCell::new(nv))));
                }
                arith(op, l, r, pos)
            }
            Sub | Mul | Div | Mod => arith(op, l, r, pos),
            Eq => Ok(Value::Bool(values_eq(&l, &r))),
            Neq => Ok(Value::Bool(!values_eq(&l, &r))),
            Lt | Gt | Le | Ge => {
                if let (Value::Txt(a), Value::Txt(b)) = (&l, &r) {
                    return Ok(Value::Bool(match op {
                        BinOp::Lt => a < b,
                        BinOp::Gt => a > b,
                        BinOp::Le => a <= b,
                        BinOp::Ge => a >= b,
                        _ => unreachable!(),
                    }));
                }
                let (a, b) = numeric_pair(l, r, pos)?;
                Ok(Value::Bool(match op {
                    BinOp::Lt => a < b,
                    BinOp::Gt => a > b,
                    BinOp::Le => a <= b,
                    BinOp::Ge => a >= b,
                    _ => unreachable!(),
                }))
            }
            And | Or => unreachable!(),
        }
    }

    fn index_one(&mut self, base: Value, ix: Value, pos: Pos) -> Result<Value, InterpError> {
        match base {
            Value::Arr(a) => {
                let i = match ix {
                    Value::Num(n) => n,
                    _ => return Err(err(pos, "ইনডেক্স 'সংখ্যা' টাইপের হতে হবে")),
                };
                let b = a.borrow();
                let len = b.len() as i64;
                if i < 0 || i >= len {
                    return Err(err(
                        pos,
                        format!("ইনডেক্স {} সীমার বাইরে (দৈর্ঘ্য {})", bn_num(i as u32), bn_num(len as u32)),
                    ));
                }
                Ok(b[i as usize].clone())
            }
            Value::Txt(s) => {
                let i = match ix {
                    Value::Num(n) => n,
                    _ => return Err(err(pos, "ইনডেক্স 'সংখ্যা' টাইপের হতে হবে")),
                };
                let chars: Vec<char> = s.chars().collect();
                if i < 0 || i >= chars.len() as i64 {
                    return Err(err(
                        pos,
                        format!(
                            "ইনডেক্স {} সীমার বাইরে (দৈর্ঘ্য {})",
                            bn_num(i as u32),
                            bn_num(chars.len() as u32)
                        ),
                    ));
                }
                Ok(Value::Ch(chars[i as usize]))
            }
            Value::Map(m) => {
                let key = match ix {
                    Value::Txt(k) => k,
                    Value::Num(n) => format!("{}", n),
                    _ => return Err(err(pos, "ম্যাপ key হতে হবে 'লেখা' বা 'সংখ্যা'")),
                };
                let b = m.borrow();
                match b.get(&key) {
                    Some(v) => Ok(v.clone()),
                    None => Err(err(pos, format!("কী '{}' ম্যাপে নেই", key))),
                }
            }
            _ => Err(err(pos, "শুধু অ্যারে বা লেখা ইনডেক্স করা যায়")),
        }
    }

    fn set_lvalue(&mut self, target: &LValue, v: Value) -> Result<(), InterpError> {
        if let Some(field) = &target.field {
            // Struct field assignment: p.field = value
            let name = &target.base.name;
            let m = match self.lookup(name) {
                Some(Value::Map(m)) => m,
                Some(_) => {
                    return Err(err(
                        target.base.pos,
                        format!("'{}' তথ্য নয়", name),
                    ))
                }
                None => {
                    return Err(err(
                        target.base.pos,
                        format!("অঘোষিত ভ্যারিয়েবল '{}'", name),
                    ))
                }
            };
            m.borrow_mut().insert(field.name.clone(), v);
            return Ok(());
        }
        if target.idx.is_empty() {
            return self.assign_var(&target.base.name, v, target.base.pos);
        }
        // Map index assignment
        if let Some(Value::Map(m)) = self.lookup(&target.base.name) {
            let key_expr = target.idx.first().unwrap();
            let kv = self.eval(key_expr)?;
            let key = match kv {
                Value::Txt(k) => k,
                Value::Num(n) => format!("{}", n),
                _ => return Err(err(target.base.pos, "ম্যাপ key হতে হবে 'লেখা' বা 'সংখ্যা'")),
            };
            m.borrow_mut().insert(key, v);
            return Ok(());
        }
        // Array index assignment
        let arr_rc = match self.lookup(&target.base.name) {
            Some(Value::Arr(a)) => a,
            Some(_) => {
                return Err(err(
                    target.base.pos,
                    format!("'{}' অ্যারে নয়", target.base.name),
                ));
            }
            None => {
                return Err(err(
                    target.base.pos,
                    format!("অঘোষিত ভ্যারিয়েবল '{}'", target.base.name),
                ));
            }
        };
        let n = target.idx.len();
        let mut rc = arr_rc;
        for (k, ie) in target.idx.iter().enumerate() {
            let iv = self.eval(ie)?;
            let i = match iv {
                Value::Num(i) => i,
                _ => return Err(err(target.base.pos, "ইনডেক্স 'সংখ্যা' টাইপের হতে হবে")),
            };
            let mut b = rc.borrow_mut();
            let len = b.len() as i64;
            if i < 0 || i >= len {
                return Err(err(
                    target.base.pos,
                    format!(
                        "ইনডেক্স {} সীমার বাইরে (দৈর্ঘ্য {})",
                        bn_num(i as u32),
                        bn_num(len as u32)
                    ),
                ));
            }
            if k + 1 == n {
                b[i as usize] = v;
                return Ok(());
            }
            let nxt = match &b[i as usize] {
                Value::Arr(a) => Rc::clone(a),
                _ => {
                    return Err(err(target.base.pos, "নেস্টেড ইনডেক্সে অ্যারে প্রত্যাশিত"));
                }
            };
            drop(b);
            rc = nxt;
        }
        Ok(())
    }

    fn rng_next(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    fn next_net_handle(&mut self) -> i64 {
        let h = self.net_next;
        self.net_next += 1;
        h as i64
    }

    fn call_stdlib(
        &mut self,
        module: &str,
        item: &str,
        pos: Pos,
        args: &[Expr],
    ) -> Result<Value, InterpError> {
        if module == "গ্রাফিক্স" && item == "টিক" {
            let mt = self.eval(&args[0])?;
            let _ = mt;
            if let Some(ExprKind::Ident(h)) = args.get(1).map(|a| &a.kind) {
                let key = h.name.clone();
                let hpos = h.pos;
                if self.funcs.contains_key(&key) {
                    return self.call(&key, hpos, &[]);
                }
                return Err(err(hpos, format!("অজানা হ্যান্ডলার ফাংশন '{}'", key)));
            }
            return Err(err(pos, "'টিক'-এর দ্বিতীয় আর্গুমেন্ট হ্যান্ডলার ফাংশনের নাম"));
        }
        // Not necessarily stdlib — a package's functions were merged into
        // `self.funcs` under a mangled `"প্যাকেজ::ফাংশন"` key (see
        // `resolve_user_modules`), and a package call reaches here via
        // `call`'s `"::"` split looking exactly like a stdlib one. Checked
        // before evaluating `args` below: `call_named_fn` evaluates them
        // itself while binding parameters, so evaluating twice here would
        // double any side effects in argument expressions.
        let mangled = format!("{module}::{item}");
        if self.funcs.contains_key(&mangled) {
            return self.call_named_fn(&mangled, pos, args);
        }
        let mut vals: Vec<Value> = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.eval(a)?);
        }
        match (module, item, vals.as_slice()) {
            // গণিত
            ("গণিত", "পরম_মান", [Value::Num(x)]) => Ok(Value::Num(x.abs())),
            ("গণিত", "পরম_মান", [Value::Dec(x)]) => Ok(Value::Dec(x.abs())),
            ("গণিত", "বর্গমূল", [Value::Dec(x)]) => {
                if *x < 0.0 {
                    Err(err(pos, "বর্গমূলে ঋণাত্মক সংখ্যা নেয় না"))
                } else {
                    Ok(Value::Dec(x.sqrt()))
                }
            }
            ("গণিত", "বর্গমূল", [Value::Num(x)]) => {
                if *x < 0 {
                    Err(err(pos, "বর্গমূলে ঋণাত্মক সংখ্যা নেয় না"))
                } else {
                    Ok(Value::Dec((*x as f64).sqrt()))
                }
            }
            ("গণিত", "ঘাত", [a, b]) => Ok(Value::Dec(num_f(a)?.powf(num_f(b)?))),
            ("গণিত", "ফ্লোর", [v]) => Ok(Value::Num(num_f(v)?.floor() as i64)),
            ("গণিত", "সিলিং", [v]) => Ok(Value::Num(num_f(v)?.ceil() as i64)),
            ("গণিত", "রাউন্ডঅফ", [v]) => Ok(Value::Num(num_f(v)?.round() as i64)),
            ("গণিত", "সাইন", [v]) => Ok(Value::Dec(num_f(v)?.sin())),
            ("গণিত", "কোসাইন", [v]) => Ok(Value::Dec(num_f(v)?.cos())),
            ("গণিত", "ট্যান", [v]) => Ok(Value::Dec(num_f(v)?.tan())),
            ("গণিত", "লগ", [v]) => Ok(Value::Dec(num_f(v)?.log10())),
            ("গণিত", "লন", [v]) => Ok(Value::Dec(num_f(v)?.ln())),
            ("গণিত", "সর্বনিম্ন", [Value::Num(a), Value::Num(b)]) => {
                Ok(Value::Num(*a.min(b)))
            }
            ("গণিত", "সর্বোচ্চ", [Value::Num(a), Value::Num(b)]) => {
                Ok(Value::Num(*a.max(b)))
            }
            ("গণিত", "সর্বনিম্ন", [a, b]) => {
                let (x, y) = (num_f(a)?, num_f(b)?);
                Ok(Value::Dec(x.min(y)))
            }
            ("গণিত", "সর্বোচ্চ", [a, b]) => {
                let (x, y) = (num_f(a)?, num_f(b)?);
                Ok(Value::Dec(x.max(y)))
            }

            // লেখা
            ("লেখা", "বড়হাতের", [Value::Txt(s)]) => Ok(Value::Txt(
                s.chars().map(|c| c.to_ascii_uppercase()).collect(),
            )),
            ("লেখা", "ছোটহাতের", [Value::Txt(s)]) => Ok(Value::Txt(
                s.chars().map(|c| c.to_ascii_lowercase()).collect(),
            )),
            ("লেখা", "ছাঁটো", [Value::Txt(s)]) => Ok(Value::Txt(s.trim().to_string())),
            ("লেখা", "স্প্লিট", [Value::Txt(s), Value::Txt(sep)]) => {
                if sep.is_empty() {
                    return Err(err(pos, "খালি বিভাজক দেওয়া যাবে না"));
                }
                let parts: Vec<Value> = s
                    .split(sep.as_str())
                    .map(|p| Value::Txt(p.to_string()))
                    .collect();
                Ok(Value::Arr(Rc::new(RefCell::new(parts))))
            }
            ("লেখা", "জুড়াও", [Value::Arr(arr), Value::Txt(sep)]) => {
                let mut out = String::new();
                for (i, v) in arr.borrow().iter().enumerate() {
                    if i > 0 {
                        out.push_str(sep);
                    }
                    match v {
                        Value::Txt(t) => out.push_str(t),
                        other => {
                            return Err(err(
                                pos,
                                format!("'জুড়াও'-তে 'লেখা' অ্যারে লাগে, '{}' পেয়েছে", other),
                            ))
                        }
                    }
                }
                Ok(Value::Txt(out))
            }
            ("লেখা", "বদলাও", [Value::Txt(s), Value::Txt(from), Value::Txt(to)]) => {
                if from.is_empty() {
                    return Err(err(pos, "খালি অনুসন্ধান-লেখা দেওয়া যাবে না"));
                }
                Ok(Value::Txt(s.replace(from.as_str(), to)))
            }
            ("লেখা", "খুঁজো", [Value::Txt(s), Value::Txt(sub)]) => {
                Ok(Value::Num(match s.find(sub.as_str()) {
                    Some(bi) => s[..bi].chars().count() as i64,
                    None => -1,
                }))
            }
            ("লেখা", "স্লাইস", [Value::Txt(s), Value::Num(st), Value::Num(ln)]) => {
                let chars: Vec<char> = s.chars().collect();
                let start = (*st).clamp(0, chars.len() as i64) as usize;
                let end = (start + (*ln).max(0) as usize).min(chars.len());
                Ok(Value::Txt(chars[start..end].iter().collect()))
            }
            ("লেখা", "শুরুতে_আছে", [Value::Txt(s), Value::Txt(p)]) => {
                Ok(Value::Bool(s.starts_with(p.as_str())))
            }
            ("লেখা", "শেষে_আছে", [Value::Txt(s), Value::Txt(p)]) => {
                Ok(Value::Bool(s.ends_with(p.as_str())))
            }

            // ফাইল  (পড়ো_লাইন is a global builtin, not a member — it reads
            // from standard input rather than from a file)
            ("ফাইল", "পড়ো", [Value::Txt(path)]) => match std::fs::read_to_string(path) {
                Ok(content) => Ok(Value::Txt(content)),
                Err(e) => Err(err(pos, format!("ফাইল পড়া যায়নি '{}': {}", path, e))),
            },
            ("ফাইল", "লেখো", [Value::Txt(path), Value::Txt(content)]) => {
                match std::fs::write(path, content.as_bytes()) {
                    Ok(()) => Ok(Value::Null),
                    Err(e) => Err(err(pos, format!("ফাইল লেখা যায়নি '{}': {}", path, e))),
                }
            }
            ("ফাইল", "এপেন্ড", [Value::Txt(path), Value::Txt(content)]) => {
                use std::io::Write as _;
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut f| f.write_all(content.as_bytes()))
                {
                    Ok(()) => Ok(Value::Null),
                    Err(e) => Err(err(pos, format!("ফাইলে এপেন্ড করা যায়নি '{}': {}", path, e))),
                }
            }
            ("ফাইল", "লাইন_তালিকা", [Value::Txt(path)]) => match std::fs::read_to_string(path) {
                Ok(content) => {
                    let lines: Vec<Value> = content.lines().map(|l| Value::Txt(l.to_string())).collect();
                    Ok(Value::Arr(Rc::new(RefCell::new(lines))))
                }
                Err(e) => Err(err(pos, format!("ফাইল পড়া যায়নি '{}': {}", path, e))),
            },

            // সময়
            ("সময়", "এখন_মিলিসেকেন্ড", []) => {
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                Ok(Value::Num(ms))
            }
            ("সময়", "সেকেন্ড", []) => Ok(Value::Dec(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0),
            )),
            ("সময়", "বছর", []) => Ok(Value::Num(now_civil().0)),
            ("সময়", "মাস", []) => Ok(Value::Num(now_civil().1 as i64)),
            ("সময়", "দিন", []) => Ok(Value::Num(now_civil().2 as i64)),
            ("সময়", "ঘণ্টা", []) => Ok(Value::Num(now_time_of_day().0 as i64)),
            ("সময়", "মিনিট", []) => Ok(Value::Num(now_time_of_day().1 as i64)),
            ("সময়", "সেকেন্ড_অংশ", []) => Ok(Value::Num(now_time_of_day().2 as i64)),
            ("সময়", "বর্তমান_তারিখ_লেখা", []) => {
                let (y, mo, d) = now_civil();
                let (h, mi, s) = now_time_of_day();
                Ok(Value::Txt(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")))
            }

            // র‍্যান্ডম
            ("র‍্যান্ডম", "বীজ", [Value::Num(s)]) => {
                self.rng = *s as u64 ^ 0x2545F4914F6CDD1D;
                Ok(Value::Null)
            }
            ("র‍্যান্ডম", "সংখ্যা", []) => Ok(Value::Num(self.rng_next() as i64)),
            ("র‍্যান্ডম", "মধ্যে", [Value::Num(lo), Value::Num(hi)]) => {
                if lo > hi {
                    return Err(err(pos, "'মধ্যে'-তে নিম্নসীমা উচ্চসীমার বেশি"));
                }
                let span = (hi - lo + 1) as u64;
                Ok(Value::Num(lo + (self.rng_next() % span) as i64))
            }
            ("র‍্যান্ডম", "দশমিক", []) => {
                Ok(Value::Dec((self.rng_next() % 1_000_000) as f64 / 1_000_000.0))
            }

            // ফাইলসিস্টেম
            ("ফাইলসিস্টেম", "ফাইল_আছে", [Value::Txt(p)]) => {
                Ok(Value::Bool(std::path::Path::new(p).is_file()))
            }
            ("ফাইলসিস্টেম", "ডিরেক্টরি_আছে", [Value::Txt(p)]) => {
                Ok(Value::Bool(std::path::Path::new(p).is_dir()))
            }
            ("ফাইলসিস্টেম", "ডিরেক্টরি_বানাও", [Value::Txt(p)]) => {
                match std::fs::create_dir_all(p) {
                    Ok(()) => Ok(Value::Null),
                    Err(e) => Err(err(pos, format!("ডিরেক্টরি তৈরি ব্যর্থ '{}': {}", p, e))),
                }
            }
            ("ফাইলসিস্টেম", "মুছো", [Value::Txt(p)]) => match std::fs::remove_file(p) {
                Ok(()) => Ok(Value::Null),
                Err(e) => Err(err(pos, format!("মুছতে ব্যর্থ '{}': {}", p, e))),
            },
            ("ফাইলসিস্টেম", "ডিরেক্টরি_মুছো", [Value::Txt(p)]) => match std::fs::remove_dir_all(p) {
                Ok(()) => Ok(Value::Null),
                Err(e) => Err(err(pos, format!("ডিরেক্টরি মুছতে ব্যর্থ '{}': {}", p, e))),
            },
            ("ফাইলসিস্টেম", "তালিকা", [Value::Txt(p)]) => {
                let mut names: Vec<Value> = Vec::new();
                match std::fs::read_dir(p) {
                    Ok(rd) => {
                        for e in rd.flatten() {
                            names.push(Value::Txt(e.file_name().to_string_lossy().into_owned()));
                        }
                        Ok(Value::Arr(Rc::new(RefCell::new(names))))
                    }
                    Err(e) => Err(err(pos, format!("তালিকা পড়া যায়নি '{}': {}", p, e))),
                }
            }
            ("ফাইলসিস্টেম", "কপি", [Value::Txt(a), Value::Txt(b)]) => {
                match std::fs::copy(a, b) {
                    Ok(_) => Ok(Value::Null),
                    Err(e) => Err(err(pos, format!("কপি ব্যর্থ '{}'-('{}'): {}", a, b, e))),
                }
            }
            ("ফাইলসিস্টেম", "ডিরেক্টরি_কপি", [Value::Txt(a), Value::Txt(b)]) => {
                match copy_dir_recursive(std::path::Path::new(a), std::path::Path::new(b)) {
                    Ok(()) => Ok(Value::Null),
                    Err(e) => Err(err(pos, format!("ডিরেক্টরি কপি ব্যর্থ '{}'-('{}'): {}", a, b, e))),
                }
            }
            ("ফাইলসিস্টেম", "সরাও", [Value::Txt(a), Value::Txt(b)]) => {
                match std::fs::rename(a, b) {
                    Ok(()) => Ok(Value::Null),
                    Err(e) => Err(err(pos, format!("সরানো ব্যর্থ '{}'-('{}'): {}", a, b, e))),
                }
            }
            ("ফাইলসিস্টেম", "আকার", [Value::Txt(p)]) => match std::fs::metadata(p) {
                Ok(m) => Ok(Value::Num(m.len() as i64)),
                Err(e) => Err(err(pos, format!("আকার পড়া যায়নি '{}': {}", p, e))),
            },
            ("ফাইলসিস্টেম", "পরিবর্তনের_সময়", [Value::Txt(p)]) => match std::fs::metadata(p).and_then(|m| m.modified()) {
                Ok(t) => {
                    let ms = t
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    Ok(Value::Num(ms))
                }
                Err(e) => Err(err(pos, format!("পরিবর্তনের সময় পড়া যায়নি '{}': {}", p, e))),
            },
            ("ফাইলসিস্টেম", "বর্তমান_ডিরেক্টরি", []) => match std::env::current_dir() {
                Ok(p) => Ok(Value::Txt(p.to_string_lossy().into_owned())),
                Err(e) => Err(err(pos, format!("বর্তমান ডিরেক্টরি পড়া যায়নি: {}", e))),
            },

            // পাথ — লেক্সিক্যাল পাথ ম্যানিপুলেশন, ডিস্কে কিছু ছোঁয় না
            ("পাথ", "জোড়ো", [Value::Txt(a), Value::Txt(b)]) => {
                Ok(Value::Txt(std::path::Path::new(a).join(b).to_string_lossy().into_owned()))
            }
            ("পাথ", "ফাইলনাম", [Value::Txt(p)]) => Ok(Value::Txt(
                std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            )),
            ("পাথ", "ডিরেক্টরিনাম", [Value::Txt(p)]) => Ok(Value::Txt(
                std::path::Path::new(p).parent().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            )),
            ("পাথ", "এক্সটেনশন", [Value::Txt(p)]) => Ok(Value::Txt(
                std::path::Path::new(p).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default(),
            )),
            ("পাথ", "পরম_পাথ", [Value::Txt(p)]) => {
                let path = std::path::Path::new(p);
                let abs = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    std::env::current_dir().unwrap_or_default().join(path)
                };
                Ok(Value::Txt(abs.to_string_lossy().into_owned()))
            }

            // জেসন
            ("জেসন", "বৈধ", [Value::Txt(t)]) => Ok(Value::Bool(json_valid(t))),
            ("জেসন", "বের_হও", [Value::Txt(s)]) => Ok(Value::Txt(json_escape(s))),
            ("জেসন", "লেখা_বের_করো", [Value::Txt(t), Value::Txt(key)]) => {
                Ok(Value::Txt(json_string_field(t, key).unwrap_or_default()))
            }
            ("জেসন", "সংখ্যা_বের_করো", [Value::Txt(t), Value::Txt(key)]) => {
                Ok(Value::Num(
                    json_num_field(t, key).ok_or_else(|| err(pos, format!("'{}' কী-তে কোনো পূর্ণসংখ্যা নেই", key)))?,
                ))
            }

            // নেটওয়ার্ক
            ("নেটওয়ার্ক", "কানেক্ট", [Value::Txt(host), Value::Num(port)]) => {
                use std::net::TcpStream;
                match TcpStream::connect((host.as_str(), *port as u16)) {
                    Ok(stream) => {
                        let handle = self.next_net_handle();
                        self.net.insert(handle as u32, stream);
                        Ok(Value::Num(handle))
                    }
                    Err(e) => Err(err(pos, format!("সংযোগ ব্যর্থ '{}:{}': {}", host, port, e))),
                }
            }
            ("নেটওয়ার্ক", "সেন্ড", [Value::Num(h), Value::Txt(data)]) => {
                use std::io::Write as _;
                match self.net.get_mut(&((*h) as u32)) {
                    Some(s) => s.write_all(data.as_bytes()).map_err(|e| {
                        err(pos, format!("পাঠাতে ব্যর্থ: {}", e))
                    })?,
                    None => return Err(err(pos, format!("অবৈধ সংযোগ #{}", h))),
                }
                Ok(Value::Null)
            }
            ("নেটওয়ার্ক", "রিসিভ", [Value::Num(h), Value::Num(max)]) => {
                use std::io::Read as _;
                let handle = (*h) as u32;
                match self.net.get_mut(&handle) {
                    Some(s) => {
                        let mut buf = vec![0u8; (*max).clamp(1, 1 << 20) as usize];
                        let n = s.read(&mut buf).map_err(|e| err(pos, format!("পড়তে ব্যর্থ: {}", e)))?;
                        Ok(Value::Txt(String::from_utf8_lossy(&buf[..n]).into_owned()))
                    }
                    None => Err(err(pos, format!("অবৈধ সংযোগ #{}", h))),
                }
            }
            ("নেটওয়ার্ক", "ক্লোজ", [Value::Num(h)]) => {
                self.net.remove(&((*h) as u32));
                Ok(Value::Null)
            }

            // ম্যাট্রিক্স — ভেক্টর = দশমিক[], ম্যাট্রিক্স = দশমিক[][]
            ("ম্যাট্রিক্স", "ভেক্টর_যোগ", [a, b]) => {
                let (a, b) = (to_vecf(a)?, to_vecf(b)?);
                if a.len() != b.len() {
                    return Err(err(pos, "ভেক্টর_যোগ: দুই ভেক্টরের দৈর্ঘ্য সমান হতে হবে"));
                }
                Ok(vecf_val(a.iter().zip(&b).map(|(x, y)| x + y).collect()))
            }
            ("ম্যাট্রিক্স", "ভেক্টর_বিয়োগ", [a, b]) => {
                let (a, b) = (to_vecf(a)?, to_vecf(b)?);
                if a.len() != b.len() {
                    return Err(err(pos, "ভেক্টর_বিয়োগ: দুই ভেক্টরের দৈর্ঘ্য সমান হতে হবে"));
                }
                Ok(vecf_val(a.iter().zip(&b).map(|(x, y)| x - y).collect()))
            }
            ("ম্যাট্রিক্স", "ভেক্টর_স্কেল", [v, k]) => {
                let (v, k) = (to_vecf(v)?, num_f(k)?);
                Ok(vecf_val(v.iter().map(|x| x * k).collect()))
            }
            ("ম্যাট্রিক্স", "ডট", [a, b]) => {
                let (a, b) = (to_vecf(a)?, to_vecf(b)?);
                if a.len() != b.len() {
                    return Err(err(pos, "ডট: দুই ভেক্টরের দৈর্ঘ্য সমান হতে হবে"));
                }
                Ok(Value::Dec(a.iter().zip(&b).map(|(x, y)| x * y).sum()))
            }
            ("ম্যাট্রিক্স", "ক্রস", [a, b]) => {
                let (a, b) = (to_vecf(a)?, to_vecf(b)?);
                if a.len() != 3 || b.len() != 3 {
                    return Err(err(pos, "ক্রস: শুধু ৩-মাত্রিক ভেক্টরের জন্য সংজ্ঞায়িত"));
                }
                Ok(vecf_val(vec![
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ]))
            }
            ("ম্যাট্রিক্স", "নর্ম", [v]) => {
                let v = to_vecf(v)?;
                Ok(Value::Dec(v.iter().map(|x| x * x).sum::<f64>().sqrt()))
            }
            ("ম্যাট্রিক্স", "যোগ", [a, b]) => {
                let (a, b) = (to_matf(a)?, to_matf(b)?);
                let (ra, ca) = mat_shape(&a, pos)?;
                let (rb, cb) = mat_shape(&b, pos)?;
                if (ra, ca) != (rb, cb) {
                    return Err(err(pos, "যোগ: দুই ম্যাট্রিক্সের মাত্রা সমান হতে হবে"));
                }
                Ok(matf_val(a.iter().zip(&b).map(|(ra, rb)| ra.iter().zip(rb).map(|(x, y)| x + y).collect()).collect()))
            }
            ("ম্যাট্রিক্স", "বিয়োগ", [a, b]) => {
                let (a, b) = (to_matf(a)?, to_matf(b)?);
                let (ra, ca) = mat_shape(&a, pos)?;
                let (rb, cb) = mat_shape(&b, pos)?;
                if (ra, ca) != (rb, cb) {
                    return Err(err(pos, "বিয়োগ: দুই ম্যাট্রিক্সের মাত্রা সমান হতে হবে"));
                }
                Ok(matf_val(a.iter().zip(&b).map(|(ra, rb)| ra.iter().zip(rb).map(|(x, y)| x - y).collect()).collect()))
            }
            ("ম্যাট্রিক্স", "স্কেল", [m, k]) => {
                let (m, k) = (to_matf(m)?, num_f(k)?);
                mat_shape(&m, pos)?;
                Ok(matf_val(m.into_iter().map(|row| row.into_iter().map(|x| x * k).collect()).collect()))
            }
            ("ম্যাট্রিক্স", "গুণ", [a, b]) => {
                let (a, b) = (to_matf(a)?, to_matf(b)?);
                let (ra, ca) = mat_shape(&a, pos)?;
                let (rb, cb) = mat_shape(&b, pos)?;
                if ca != rb {
                    return Err(err(pos, format!("গুণ: প্রথম ম্যাট্রিক্সের কলাম ({}) দ্বিতীয়টির সারির ({}) সমান হতে হবে", ca, rb)));
                }
                let mut out = vec![vec![0.0; cb]; ra];
                for (i, row) in out.iter_mut().enumerate() {
                    for (j, cell) in row.iter_mut().enumerate() {
                        *cell = (0..ca).map(|k| a[i][k] * b[k][j]).sum();
                    }
                }
                Ok(matf_val(out))
            }
            ("ম্যাট্রিক্স", "ট্রান্সপোজ", [m]) => {
                let m = to_matf(m)?;
                let (rows, cols) = mat_shape(&m, pos)?;
                let mut out = vec![vec![0.0; rows]; cols];
                for (i, row) in m.iter().enumerate() {
                    for (j, v) in row.iter().enumerate() {
                        out[j][i] = *v;
                    }
                }
                Ok(matf_val(out))
            }
            ("ম্যাট্রিক্স", "নির্ণায়ক", [m]) => {
                let m = to_matf(m)?;
                let (rows, cols) = mat_shape(&m, pos)?;
                if rows != cols {
                    return Err(err(pos, "নির্ণায়ক: শুধু বর্গ ম্যাট্রিক্সের জন্য সংজ্ঞায়িত"));
                }
                Ok(Value::Dec(mat_det(m)))
            }
            ("ম্যাট্রিক্স", "বিপরীত", [m]) => {
                let m = to_matf(m)?;
                let (rows, cols) = mat_shape(&m, pos)?;
                if rows != cols {
                    return Err(err(pos, "বিপরীত: শুধু বর্গ ম্যাট্রিক্সের জন্য সংজ্ঞায়িত"));
                }
                match mat_inv(m) {
                    Some(inv) => Ok(matf_val(inv)),
                    None => Err(err(pos, "বিপরীত: ম্যাট্রিক্সটি ইনভার্টিবল নয় (নির্ণায়ক শূন্য)")),
                }
            }
            ("ম্যাট্রিক্স", "অভেদক", [Value::Num(n)]) => {
                if *n < 1 {
                    return Err(err(pos, "অভেদক: আকার কমপক্ষে ১ হতে হবে"));
                }
                let n = *n as usize;
                Ok(matf_val((0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect()))
            }
            ("ম্যাট্রিক্স", "শূন্য_ম্যাট্রিক্স", [Value::Num(rows), Value::Num(cols)]) => {
                if *rows < 1 || *cols < 1 {
                    return Err(err(pos, "শূন্য_ম্যাট্রিক্স: সারি ও কলাম কমপক্ষে ১ হতে হবে"));
                }
                Ok(matf_val(vec![vec![0.0; *cols as usize]; *rows as usize]))
            }

            // জ্যামিতি — বিন্দু = [x, y] (দশমিক[]), বহুভুজ = বিন্দুর তালিকা
            // (দশমিক[][]) — ম্যাট্রিক্সের ভেক্টর/ম্যাট্রিক্স কনভেনশন অনুসরণ করে।
            ("জ্যামিতি", "দূরত্ব", [x1, y1, x2, y2]) => {
                let (x1, y1, x2, y2) = (num_f(x1)?, num_f(y1)?, num_f(x2)?, num_f(y2)?);
                Ok(Value::Dec(((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt()))
            }
            ("জ্যামিতি", "কোণ", [x1, y1, x2, y2]) => {
                let (x1, y1, x2, y2) = (num_f(x1)?, num_f(y1)?, num_f(x2)?, num_f(y2)?);
                Ok(Value::Dec((y2 - y1).atan2(x2 - x1)))
            }
            ("জ্যামিতি", "ঘোরানো", [x, y, cx, cy, angle]) => {
                let (x, y, cx, cy, angle) = (num_f(x)?, num_f(y)?, num_f(cx)?, num_f(cy)?, num_f(angle)?);
                let (dx, dy) = (x - cx, y - cy);
                let (sin, cos) = angle.sin_cos();
                Ok(vecf_val(vec![cx + dx * cos - dy * sin, cy + dx * sin + dy * cos]))
            }
            ("জ্যামিতি", "বৃত্তের_ক্ষেত্রফল", [r]) => Ok(Value::Dec(std::f64::consts::PI * num_f(r)?.powi(2))),
            ("জ্যামিতি", "বৃত্তের_পরিধি", [r]) => Ok(Value::Dec(2.0 * std::f64::consts::PI * num_f(r)?)),
            ("জ্যামিতি", "উপবৃত্তের_ক্ষেত্রফল", [rx, ry]) => {
                Ok(Value::Dec(std::f64::consts::PI * num_f(rx)? * num_f(ry)?))
            }
            // রামানুজনের আসন্নীকরণ — উপবৃত্তের পরিধির কোনো প্রাথমিক সূত্র নেই
            // (উপবৃত্তীয় ইন্টিগ্রাল লাগে), কিন্তু এটা সব rx/ry অনুপাতে
            // ব্যবহারিক নির্ভুলতার মধ্যে থাকে।
            ("জ্যামিতি", "উপবৃত্তের_পরিধি", [rx, ry]) => {
                let (rx, ry) = (num_f(rx)?, num_f(ry)?);
                let h = ((rx - ry) / (rx + ry)).powi(2);
                Ok(Value::Dec(std::f64::consts::PI * (rx + ry) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()))))
            }
            ("জ্যামিতি", "ত্রিভুজের_ক্ষেত্রফল", [x1, y1, x2, y2, x3, y3]) => {
                let (x1, y1, x2, y2, x3, y3) = (num_f(x1)?, num_f(y1)?, num_f(x2)?, num_f(y2)?, num_f(x3)?, num_f(y3)?);
                Ok(Value::Dec((x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2)).abs() / 2.0))
            }
            ("জ্যামিতি", "বহুভুজের_ক্ষেত্রফল", [points]) => {
                let pts = to_matf(points)?;
                if pts.len() < 3 || pts.iter().any(|p| p.len() != 2) {
                    return Err(err(pos, "বহুভুজের_ক্ষেত্রফল: কমপক্ষে ৩টি [x, y] বিন্দু দরকার"));
                }
                Ok(Value::Dec(poly_area(&pts)))
            }
            ("জ্যামিতি", "গোলকের_আয়তন", [r]) => {
                Ok(Value::Dec(4.0 / 3.0 * std::f64::consts::PI * num_f(r)?.powi(3)))
            }
            ("জ্যামিতি", "গোলকের_পৃষ্ঠফল", [r]) => Ok(Value::Dec(4.0 * std::f64::consts::PI * num_f(r)?.powi(2))),
            ("জ্যামিতি", "শঙ্কুর_আয়তন", [r, h]) => {
                Ok(Value::Dec(std::f64::consts::PI * num_f(r)?.powi(2) * num_f(h)? / 3.0))
            }
            ("জ্যামিতি", "শঙ্কুর_পৃষ্ঠফল", [r, h]) => {
                let (r, h) = (num_f(r)?, num_f(h)?);
                Ok(Value::Dec(std::f64::consts::PI * r * (r + (r * r + h * h).sqrt())))
            }
            ("জ্যামিতি", "সিলিন্ডারের_আয়তন", [r, h]) => {
                Ok(Value::Dec(std::f64::consts::PI * num_f(r)?.powi(2) * num_f(h)?))
            }
            ("জ্যামিতি", "সিলিন্ডারের_পৃষ্ঠফল", [r, h]) => {
                let (r, h) = (num_f(r)?, num_f(h)?);
                Ok(Value::Dec(2.0 * std::f64::consts::PI * r * (r + h)))
            }
            ("জ্যামিতি", "নিয়মিত_বহুভুজ", [cx, cy, r, Value::Num(n)]) => {
                if *n < 3 {
                    return Err(err(pos, "নিয়মিত_বহুভুজ: কমপক্ষে ৩ বাহু দরকার"));
                }
                let (cx, cy, r) = (num_f(cx)?, num_f(cy)?, num_f(r)?);
                Ok(matf_val(regular_polygon(cx, cy, r, r, *n)))
            }
            ("জ্যামিতি", "উপবৃত্ত_বিন্দু", [cx, cy, rx, ry, Value::Num(n)]) => {
                if *n < 3 {
                    return Err(err(pos, "উপবৃত্ত_বিন্দু: কমপক্ষে ৩টি বিন্দু দরকার"));
                }
                let (cx, cy, rx, ry) = (num_f(cx)?, num_f(cy)?, num_f(rx)?, num_f(ry)?);
                Ok(matf_val(regular_polygon(cx, cy, rx, ry, *n)))
            }
            ("জ্যামিতি", "রেখার_ছেদ", [x1, y1, x2, y2, x3, y3, x4, y4]) => {
                let (x1, y1, x2, y2) = (num_f(x1)?, num_f(y1)?, num_f(x2)?, num_f(y2)?);
                let (x3, y3, x4, y4) = (num_f(x3)?, num_f(y3)?, num_f(x4)?, num_f(y4)?);
                let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
                if denom.abs() < 1e-12 {
                    return Err(err(pos, "রেখার_ছেদ: রেখা দুটি সমান্তরাল, কোনো ছেদবিন্দু নেই"));
                }
                let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
                Ok(vecf_val(vec![x1 + t * (x2 - x1), y1 + t * (y2 - y1)]))
            }

            // পরিসংখ্যান
            ("পরিসংখ্যান", "সমষ্টি", [v]) => {
                let v = to_vecf(v)?;
                if v.is_empty() {
                    return Err(err(pos, "সমষ্টি: খালি ভেক্টরে সংজ্ঞায়িত নয়"));
                }
                Ok(Value::Dec(v.iter().sum()))
            }
            ("পরিসংখ্যান", "গড়", [v]) => {
                let v = to_vecf(v)?;
                if v.is_empty() {
                    return Err(err(pos, "গড়: খালি ভেক্টরে সংজ্ঞায়িত নয়"));
                }
                Ok(Value::Dec(stat_mean(&v)))
            }
            ("পরিসংখ্যান", "মধ্যক", [v]) => {
                let v = to_vecf(v)?;
                if v.is_empty() {
                    return Err(err(pos, "মধ্যক: খালি ভেক্টরে সংজ্ঞায়িত নয়"));
                }
                Ok(Value::Dec(stat_median(&v)))
            }
            ("পরিসংখ্যান", "প্রচুরক", [v]) => {
                let v = to_vecf(v)?;
                if v.is_empty() {
                    return Err(err(pos, "প্রচুরক: খালি ভেক্টরে সংজ্ঞায়িত নয়"));
                }
                Ok(Value::Dec(stat_mode(&v)))
            }
            ("পরিসংখ্যান", "ভেদাংক", [v]) => {
                let v = to_vecf(v)?;
                if v.is_empty() {
                    return Err(err(pos, "ভেদাংক: খালি ভেক্টরে সংজ্ঞায়িত নয়"));
                }
                Ok(Value::Dec(stat_variance(&v)))
            }
            ("পরিসংখ্যান", "আদর্শ_বিচ্যুতি", [v]) => {
                let v = to_vecf(v)?;
                if v.is_empty() {
                    return Err(err(pos, "আদর্শ_বিচ্যুতি: খালি ভেক্টরে সংজ্ঞায়িত নয়"));
                }
                Ok(Value::Dec(stat_variance(&v).sqrt()))
            }
            ("পরিসংখ্যান", "সহভেদাংক", [a, b]) => {
                let (a, b) = (to_vecf(a)?, to_vecf(b)?);
                if a.len() != b.len() || a.len() < 2 {
                    return Err(err(pos, "সহভেদাংক: দুই ভেক্টরের দৈর্ঘ্য সমান ও কমপক্ষে ২ হতে হবে"));
                }
                Ok(Value::Dec(stat_covariance(&a, &b)))
            }
            ("পরিসংখ্যান", "সহসম্পর্ক", [a, b]) => {
                let (a, b) = (to_vecf(a)?, to_vecf(b)?);
                if a.len() != b.len() || a.len() < 2 {
                    return Err(err(pos, "সহসম্পর্ক: দুই ভেক্টরের দৈর্ঘ্য সমান ও কমপক্ষে ২ হতে হবে"));
                }
                let (sa, sb) = (stat_variance(&a).sqrt(), stat_variance(&b).sqrt());
                if sa == 0.0 || sb == 0.0 {
                    return Err(err(pos, "সহসম্পর্ক: একটি ভেক্টরের আদর্শ-বিচ্যুতি শূন্য (সব মান সমান)"));
                }
                Ok(Value::Dec(stat_covariance(&a, &b) / (sa * sb)))
            }
            ("পরিসংখ্যান", "রৈখিক_রিগ্রেশন", [x, y]) => {
                let (x, y) = (to_vecf(x)?, to_vecf(y)?);
                if x.len() != y.len() || x.len() < 2 {
                    return Err(err(pos, "রৈখিক_রিগ্রেশন: দুই ভেক্টরের দৈর্ঘ্য সমান ও কমপক্ষে ২ হতে হবে"));
                }
                let (mx, my) = (stat_mean(&x), stat_mean(&y));
                let cov: f64 = x.iter().zip(&y).map(|(xi, yi)| (xi - mx) * (yi - my)).sum();
                let varx: f64 = x.iter().map(|xi| (xi - mx).powi(2)).sum();
                if varx == 0.0 {
                    return Err(err(pos, "রৈখিক_রিগ্রেশন: x-এর সব মান সমান, ঢাল অসংজ্ঞায়িত"));
                }
                let slope = cov / varx;
                let intercept = my - slope * mx;
                Ok(vecf_val(vec![slope, intercept]))
            }

            // সিস্টেম
            ("সিস্টেম", "আর্গুমেন্ট", []) => Ok(Value::Arr(Rc::new(RefCell::new(
                self.argv.iter().map(|s| Value::Txt(s.clone())).collect(),
            )))),
            ("সিস্টেম", "পরিবেশ", [Value::Txt(name)]) => {
                Ok(Value::Txt(std::env::var(name).unwrap_or_default()))
            }

            // গ্রাফিক্স — ইন্টারপ্রেটেড মোডে আঁকা no-op
            ("গ্রাফিক্স", _, _) => Ok(Value::Null),

            _ => Err(err(
                pos,
                format!("'{}.{}': ভুল আর্গুমেন্ট টাইপ বা অজানা ফাংশন", module, item),
            )),
        }
    }

    fn call(&mut self, name: &str, pos: Pos, args: &[Expr]) -> Result<Value, InterpError> {
        if let Some((module, item)) = name.split_once("::") {
            return self.call_stdlib(module, item, pos, args);
        }
        if name == "লেখো" {
            for a in args {
                let v = self.eval(a)?;
                writeln!(self.out, "{}", v).map_err(|_| {
                    err(pos, "আউটপুট লেখা যায়নি")
                })?;
            }
            return Ok(Value::Null);
        }
        if name == "দৈর্ঘ্য" {
            if args.len() != 1 {
                return Err(err(
                    pos,
                    format!(
                        "'দৈর্ঘ্য' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                        bn_num(args.len() as u32)
                    ),
                ));
            }
            let v = self.eval(&args[0])?;
            return match v {
                Value::Txt(s) => Ok(Value::Num(s.chars().count() as i64)),
                Value::Arr(a) => Ok(Value::Num(a.borrow().len() as i64)),
                Value::Map(m) => Ok(Value::Num(m.borrow().len() as i64)),
                _ => Err(err(pos, "'দৈর্ঘ্য' 'লেখা', অ্যারে বা ম্যাপ নেয়")),
            };
        }
        if name == "পড়ো_লাইন" {
            if !args.is_empty() {
                return Err(err(pos, "'পড়ো_লাইন' কোনো আর্গুমেন্ট নেয় না"));
            }
            let mut line = String::new();
            return match std::io::stdin().read_line(&mut line) {
                Ok(_) => Ok(Value::Txt(line.trim_end_matches(['\n', '\r']).to_string())),
                Err(_) => Err(err(pos, "ইনপুট পড়া যায়নি")),
            };
        }
        if name == "কপি" {
            if args.len() != 1 {
                return Err(err(
                    pos,
                    format!(
                        "'কপি' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                        bn_num(args.len() as u32)
                    ),
                ));
            }
            let v = self.eval(&args[0])?;
            return Ok(deep_copy(&v));
        }
        if name == "সাজাও" {
            if args.len() != 1 {
                return Err(err(
                    pos,
                    format!(
                        "'সাজাও' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                        bn_num(args.len() as u32)
                    ),
                ));
            }
            let v = self.eval(&args[0])?;
            let arr = match v {
                Value::Arr(a) => a,
                _ => return Err(err(pos, "'সাজাও' সংখ্যা[]/দশমিক[]/লেখা[] নেয়")),
            };
            let mut items = arr.borrow().clone();
            items.sort_by(|a, b| match (a, b) {
                (Value::Num(x), Value::Num(y)) => x.cmp(y),
                (Value::Dec(x), Value::Dec(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                (Value::Txt(x), Value::Txt(y)) => x.cmp(y),
                _ => std::cmp::Ordering::Equal,
            });
            return Ok(Value::Arr(Rc::new(RefCell::new(items))));
        }
        if name == "শেয়ার_করো" {
            if args.len() != 1 {
                return Err(err(
                    pos,
                    format!(
                        "'শেয়ার_করো' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                        bn_num(args.len() as u32)
                    ),
                ));
            }
            let v = self.eval(&args[0])?;
            return Ok(Value::Shared(Rc::new(RefCell::new(v))));
        }
        if name == "মান" {
            if args.len() != 1 {
                return Err(err(
                    pos,
                    format!("'মান' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে", bn_num(args.len() as u32)),
                ));
            }
            let v = self.eval(&args[0])?;
            return match v {
                Value::Shared(s) => Ok(s.borrow().clone()),
                _ => Err(err(pos, "'মান' 'শেয়ার' মান নেয়")),
            };
        }
        if name == "বসাও" {
            if args.len() != 2 {
                return Err(err(
                    pos,
                    format!(
                        "'বসাও' ২টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                        bn_num(args.len() as u32)
                    ),
                ));
            }
            let cell = self.eval(&args[0])?;
            let v = self.eval(&args[1])?;
            return match cell {
                Value::Shared(s) => {
                    *s.borrow_mut() = v;
                    Ok(Value::Null)
                }
                _ => Err(err(pos, "'বসাও'-এর প্রথম আর্গুমেন্ট 'শেয়ার' হতে হবে")),
            };
        }
        if name == "লেখায়" {
            if args.len() != 1 {
                return Err(err(
                    pos,
                    format!(
                        "'লেখায়' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                        bn_num(args.len() as u32)
                    ),
                ));
            }
            let v = self.eval(&args[0])?;
            return match v {
                Value::Shared(_) => Err(err(pos, "'লেখায়'-এর আগে 'মান(x)' দিন")),
                other => Ok(Value::Txt(format!("{}", other))),
            };
        }
        if name == "ম্যাপ_তৈরি" {
            return Ok(Value::Map(Rc::new(RefCell::new(HashMap::new()))));
        }
        if self.structs.contains_key(name) {
            let field_names = self.structs[name].clone();
            let mut m = HashMap::new();
            for (i, fname) in field_names.iter().enumerate() {
                if i < args.len() {
                    let v = self.eval(&args[i])?;
                    m.insert(fname.clone(), v);
                }
            }
            return Ok(Value::Map(Rc::new(RefCell::new(m))));
        }
        if name == "চাবি_গুলো" {
            if args.len() != 1 {
                return Err(err(pos, format!("'চাবি_গুলো' ১টি আর্গুমেন্ট নেয়")));
            }
            let v = self.eval(&args[0])?;
            return match v {
                Value::Map(m) => {
                    let keys: Vec<Value> =
                        m.borrow().keys().map(|k| Value::Txt(k.clone())).collect();
                    Ok(Value::Arr(Rc::new(RefCell::new(keys))))
                }
                _ => Err(err(pos, "'চাবি_গুলো' ম্যাপ নেয়")),
            };
        }
        if name == "আছে_কি" {
            if args.len() != 2 {
                return Err(err(pos, format!("'আছে_কি' ২টি আর্গুমেন্ট নেয়")));
            }
            let m = self.eval(&args[0])?;
            let k = self.eval(&args[1])?;
            return match (m, k) {
                (Value::Map(m), Value::Txt(k)) => {
                    Ok(Value::Bool(m.borrow().contains_key(&k)))
                }
                (Value::Map(m), Value::Num(n)) => {
                    Ok(Value::Bool(m.borrow().contains_key(&n.to_string())))
                }
                _ => Err(err(pos, "'আছে_কি' ম্যাপ ও key নেয়")),
            };
        }
        if name == "চাবি_মুছো" {
            if args.len() != 2 {
                return Err(err(pos, format!("'মুছো_কী' ২টি আর্গুমেন্ট নেয়")));
            }
            let m = self.eval(&args[0])?;
            let k = self.eval(&args[1])?;
            return match (m, k) {
                (Value::Map(m), Value::Txt(k)) => {
                    m.borrow_mut().remove(&k);
                    Ok(Value::Null)
                }
                (Value::Map(m), Value::Num(n)) => {
                    m.borrow_mut().remove(&n.to_string());
                    Ok(Value::Null)
                }
                _ => Err(err(pos, "'মুছো_কী' ম্যাপ ও key নেয়")),
            };
        }
        self.call_named_fn(name, pos, args)
    }

    /// Runs a call against `self.funcs` by its exact key — shared by
    /// ordinary bare calls (`যোগ(a, b)`, key `"যোগ"`) and package calls
    /// (`প্যাকেজ.ফাংশন(...)`, key `"প্যাকেজ::ফাংশন"` — package functions are
    /// genuine Kolom code merged in under that mangled key by
    /// `resolve_user_modules`). Split out so `call_stdlib` can reach it
    /// directly without re-entering `call` and re-triggering its `"::"`
    /// split (which would just call `call_stdlib` again — infinite loop).
    fn call_named_fn(&mut self, name: &str, pos: Pos, args: &[Expr]) -> Result<Value, InterpError> {
        let f = match self.funcs.get(name) {
            Some(f) => Rc::clone(f),
            None => return Err(err(pos, format!("অজানা ফাংশন '{}'", name))),
        };
        if f.params.len() != args.len() {
            return Err(err(
                pos,
                format!(
                    "'{}' {}টি প্যারামিটার নেয়, {}টি পেয়েছে",
                    name,
                    bn_num(f.params.len() as u32),
                    bn_num(args.len() as u32)
                ),
            ));
        }
        let mut bound = Scope::new();
        for (p, a) in f.params.iter().zip(args.iter()) {
            let v = self.eval(a)?;
            bound.vars.insert(p.name.name.clone(), v);
        }
        self.scopes.push(bound);
        let cf = self.exec_block(&f.body)?;
        self.scopes.pop();
        Ok(match cf {
            CF::Ret(v) => v,
            _ => Value::Null,
        })
    }
}

struct JsonP<'a> {
    c: Vec<char>,
    i: usize,
    _src: &'a str,
}

impl<'a> JsonP<'a> {
    fn new(s: &'a str) -> Self {
        JsonP {
            c: s.chars().collect(),
            i: 0,
            _src: s,
        }
    }
    fn ws(&mut self) {
        while self.i < self.c.len() && self.c[self.i].is_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn value(&mut self) -> bool {
        self.ws();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => self.string().is_some(),
            Some('t') => self.lit("true"),
            Some('f') => self.lit("false"),
            Some('n') => self.lit("null"),
            Some(_) => self.number(),
            None => false,
        }
    }
    fn lit(&mut self, word: &str) -> bool {
        let w: Vec<char> = word.chars().collect();
        if self.i + w.len() <= self.c.len() && self.c[self.i..self.i + w.len()] == w[..] {
            self.i += w.len();
            true
        } else {
            false
        }
    }
    fn object(&mut self) -> bool {
        self.i += 1;
        self.ws();
        if self.peek() == Some('}') {
            self.i += 1;
            return true;
        }
        loop {
            self.ws();
            if self.string().is_none() {
                return false;
            }
            self.ws();
            if self.peek() != Some(':') {
                return false;
            }
            self.i += 1;
            if !self.value() {
                return false;
            }
            self.ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some('}') => {
                    self.i += 1;
                    return true;
                }
                _ => return false,
            }
        }
    }
    fn array(&mut self) -> bool {
        self.i += 1;
        self.ws();
        if self.peek() == Some(']') {
            self.i += 1;
            return true;
        }
        loop {
            if !self.value() {
                return false;
            }
            self.ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some(']') => {
                    self.i += 1;
                    return true;
                }
                _ => return false,
            }
        }
    }
    fn string(&mut self) -> Option<String> {
        if self.peek() != Some('"') {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            self.i += 1;
            match ch {
                '"' => return Some(out),
                '\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' => {
                            let mut v = 0u32;
                            for _ in 0..4 {
                                let h = self.peek()?;
                                v = v * 16 + h.to_digit(16)?;
                                self.i += 1;
                            }
                            out.push(char::from_u32(v)?);
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
        None
    }
    fn number(&mut self) -> bool {
        let start = self.i;
        if self.peek() == Some('-') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
                self.i += 1;
            } else {
                break;
            }
        }
        self.i > start
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

fn json_valid(text: &str) -> bool {
    let mut p = JsonP::new(text);
    p.ws();
    if !p.value() {
        return false;
    }
    p.ws();
    p.i == p.c.len()
}

fn json_escape(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_find_key(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{}\"", key);
    let key_bytes = text.find(&needle)?;
    let after_key = key_bytes + needle.len();
    let rest = &text[after_key..];
    let colon_rel = rest.find(':')?;
    Some(after_key + colon_rel + 1)
}

fn json_string_field(text: &str, key: &str) -> Option<String> {
    let start = json_find_key(text, key)?;
    let mut p = JsonP::new(&text[start..]);
    p.ws();
    p.string()
}

fn json_num_field(text: &str, key: &str) -> Option<i64> {
    let start = json_find_key(text, key)?;
    let mut p = JsonP::new(&text[start..]);
    p.ws();
    let begin = p.i;
    if !p.number() {
        return None;
    }
    let raw: String = p.c[begin..p.i].iter().collect();
    raw.parse::<i64>().ok()
}

fn num_f(v: &Value) -> Result<f64, InterpError> {    match v {
        Value::Num(n) => Ok(*n as f64),
        Value::Dec(d) => Ok(*d),
        other => Err(InterpError {
            line: 0,
            col: 0,
            message: format!("সংখ্যা প্রত্যাশিত, '{}' পাওয়া গেছে", other),
        }),
    }
}

fn to_vecf(v: &Value) -> Result<Vec<f64>, InterpError> {
    match v {
        Value::Arr(a) => a.borrow().iter().map(num_f).collect(),
        other => Err(InterpError {
            line: 0,
            col: 0,
            message: format!("ভেক্টর (দশমিক[]) প্রত্যাশিত, '{}' পাওয়া গেছে", other),
        }),
    }
}

fn vecf_val(v: Vec<f64>) -> Value {
    Value::Arr(Rc::new(RefCell::new(v.into_iter().map(Value::Dec).collect())))
}

fn to_matf(v: &Value) -> Result<Vec<Vec<f64>>, InterpError> {
    match v {
        Value::Arr(a) => a.borrow().iter().map(to_vecf).collect(),
        other => Err(InterpError {
            line: 0,
            col: 0,
            message: format!("ম্যাট্রিক্স (দশমিক[][]) প্রত্যাশিত, '{}' পাওয়া গেছে", other),
        }),
    }
}

fn matf_val(m: Vec<Vec<f64>>) -> Value {
    Value::Arr(Rc::new(RefCell::new(m.into_iter().map(vecf_val).collect())))
}

// Validates that every row has the same length, returning (rows, cols).
// A `দশমিক[][]` value is just nested arrays with no rectangularity
// guarantee, so every matrix op checks this before trusting row/col counts.
fn mat_shape(m: &[Vec<f64>], pos: Pos) -> Result<(usize, usize), InterpError> {
    let rows = m.len();
    let cols = m.first().map(|r| r.len()).unwrap_or(0);
    if m.iter().any(|r| r.len() != cols) {
        return Err(err(pos, "ম্যাট্রিক্স-এর প্রতিটি সারি একই দৈর্ঘ্যের হতে হবে"));
    }
    Ok((rows, cols))
}

// Gaussian elimination with partial pivoting, shared by নির্ণায়ক ও বিপরীত.
// Returns the row-echelon form, the pivot value product (for the
// determinant), and the number of row swaps (its sign flips the sign).
fn mat_eliminate(mut m: Vec<Vec<f64>>) -> (Vec<Vec<f64>>, f64, u32) {
    let n = m.len();
    let mut det = 1.0;
    let mut swaps = 0u32;
    for col in 0..n {
        let pivot_row = (col..n)
            .max_by(|&a, &b| m[a][col].abs().partial_cmp(&m[b][col].abs()).unwrap())
            .unwrap();
        if pivot_row != col {
            m.swap(col, pivot_row);
            swaps += 1;
        }
        let pivot = m[col][col];
        det *= pivot;
        if pivot.abs() < 1e-12 {
            continue;
        }
        for row in (col + 1)..n {
            let factor = m[row][col] / pivot;
            for c in col..n {
                m[row][c] -= factor * m[col][c];
            }
        }
    }
    (m, det, swaps)
}

fn mat_det(m: Vec<Vec<f64>>) -> f64 {
    let (_, det, swaps) = mat_eliminate(m);
    if swaps % 2 == 1 { -det } else { det }
}

// Gauss-Jordan on the augmented [m | I] matrix.
fn mat_inv(m: Vec<Vec<f64>>) -> Option<Vec<Vec<f64>>> {
    let n = m.len();
    let mut aug: Vec<Vec<f64>> = m
        .into_iter()
        .enumerate()
        .map(|(i, mut row)| {
            let mut id = vec![0.0; n];
            id[i] = 1.0;
            row.extend(id);
            row
        })
        .collect();
    for col in 0..n {
        let pivot_row = (col..n)
            .max_by(|&a, &b| aug[a][col].abs().partial_cmp(&aug[b][col].abs()).unwrap())
            .unwrap();
        aug.swap(col, pivot_row);
        let pivot = aug[col][col];
        if pivot.abs() < 1e-12 {
            return None;
        }
        for c in 0..(2 * n) {
            aug[col][c] /= pivot;
        }
        for row in 0..n {
            if row == col {
                continue;
            }
            let factor = aug[row][col];
            for c in 0..(2 * n) {
                aug[row][c] -= factor * aug[col][c];
            }
        }
    }
    Some(aug.into_iter().map(|row| row[n..].to_vec()).collect())
}

// Shoelace formula. Caller (জ্যামিতি.বহুভুজের_ক্ষেত্রফল) has already checked
// `points.len() >= 3` and that every point is `[x, y]`.
fn poly_area(points: &[Vec<f64>]) -> f64 {
    let n = points.len();
    let sum: f64 = (0..n)
        .map(|i| {
            let (x1, y1) = (points[i][0], points[i][1]);
            let (x2, y2) = (points[(i + 1) % n][0], points[(i + 1) % n][1]);
            x1 * y2 - x2 * y1
        })
        .sum();
    sum.abs() / 2.0
}

// `n` evenly spaced points on the ellipse centered at `(cx, cy)` with radii
// `(rx, ry)` — a circle when `rx == ry`. Shared by নিয়মিত_বহুভুজ (rx == ry)
// and উপবৃত্ত_বিন্দু.
fn regular_polygon(cx: f64, cy: f64, rx: f64, ry: f64, n: i64) -> Vec<Vec<f64>> {
    (0..n)
        .map(|i| {
            let angle = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            vec![cx + rx * angle.cos(), cy + ry * angle.sin()]
        })
        .collect()
}

// পরিসংখ্যান — সব দশমিক[] উপর কাজ করে। ভেদাংক/আদর্শ_বিচ্যুতি/সহভেদাংক
// population সংস্করণ (n দিয়ে ভাগ, n-1 নয়) — পুরো ডেটাসেট হাতে আছে ধরে,
// একটা sample-থেকে-অনুমান নয়।
fn stat_mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn stat_median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

fn stat_mode(v: &[f64]) -> f64 {
    let mut best = v[0];
    let mut best_count = 0usize;
    for &x in v {
        let count = v.iter().filter(|&&y| y == x).count();
        if count > best_count {
            best_count = count;
            best = x;
        }
    }
    best
}

fn stat_variance(v: &[f64]) -> f64 {
    let m = stat_mean(v);
    v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64
}

fn stat_covariance(a: &[f64], b: &[f64]) -> f64 {
    let (ma, mb) = (stat_mean(a), stat_mean(b));
    a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum::<f64>() / a.len() as f64
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Howard Hinnant's `civil_from_days` — days-since-epoch to (year, month,
/// day) in the proleptic Gregorian calendar. Public-domain algorithm, no
/// date crate needed; correct for any i64 day count, not just "now".
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// (year, month, day) for the current moment — UTC, to sidestep timezone
/// dependence.
fn now_civil() -> (i64, u32, u32) {
    let days = now_epoch_ms().div_euclid(1000).div_euclid(86400);
    civil_from_days(days)
}

/// (hour, minute, second) for the current moment — UTC.
fn now_time_of_day() -> (u32, u32, u32) {
    let secs = now_epoch_ms().div_euclid(1000).rem_euclid(86400) as u32;
    (secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn values_eq(l: &Value, r: &Value) -> bool {
    match (l, r) {
        (Value::Num(a), Value::Num(b)) => a == b,
        (Value::Dec(a), Value::Dec(b)) => a == b,
        (Value::Num(a), Value::Dec(b)) | (Value::Dec(b), Value::Num(a)) => (*a as f64) == *b,
        (Value::Txt(a), Value::Txt(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Ch(a), Value::Ch(b)) => a == b,
        (Value::Null, Value::Null) => true,
        (Value::Arr(a), Value::Arr(b)) => Rc::ptr_eq(a, b),
        (Value::Shared(a), Value::Shared(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn deep_copy(v: &Value) -> Value {
    match v {
        Value::Arr(a) => {
            let nv: Vec<Value> = a.borrow().iter().map(deep_copy).collect();
            Value::Arr(Rc::new(RefCell::new(nv)))
        }
        other => other.clone(),
    }
}

fn arith(op: BinOp, l: Value, r: Value, pos: Pos) -> Result<Value, InterpError> {
    if let (Value::Num(a), Value::Num(b)) = (&l, &r) {
        return match op {
            BinOp::Add => a
                .checked_add(*b)
                .map(Value::Num)
                .ok_or_else(|| err(pos, "পূর্ণসংখ্যা ওভারফ্লো")),
            BinOp::Sub => a
                .checked_sub(*b)
                .map(Value::Num)
                .ok_or_else(|| err(pos, "পূর্ণসংখ্যা ওভারফ্লো")),
            BinOp::Mul => a
                .checked_mul(*b)
                .map(Value::Num)
                .ok_or_else(|| err(pos, "পূর্ণসংখ্যা ওভারফ্লো")),
            BinOp::Div => {
                if *b == 0 {
                    Err(err(pos, "শূন্য দিয়ে ভাগ করা যাবে না"))
                } else {
                    a.checked_div(*b)
                        .map(Value::Num)
                        .ok_or_else(|| err(pos, "পূর্ণসংখ্যা ওভারফ্লো"))
                }
            }
            BinOp::Mod => {
                if *b == 0 {
                    Err(err(pos, "শূন্য দিয়ে ভাগ (মডুলো) করা যাবে না"))
                } else {
                    a.checked_rem(*b)
                        .map(Value::Num)
                        .ok_or_else(|| err(pos, "পূর্ণসংখ্যা ওভারফ্লো"))
                }
            }
            _ => unreachable!(),
        };
    }
    let (a, b) = numeric_pair(l, r, pos)?;
    match op {
        BinOp::Add => Ok(Value::Dec(a + b)),
        BinOp::Sub => Ok(Value::Dec(a - b)),
        BinOp::Mul => Ok(Value::Dec(a * b)),
        BinOp::Div => {
            if b == 0.0 {
                Err(err(pos, "শূন্য দিয়ে ভাগ করা যাবে না"))
            } else {
                Ok(Value::Dec(a / b))
            }
        }
        BinOp::Mod => {
            if b == 0.0 {
                Err(err(pos, "শূন্য দিয়ে ভাগ (মডুলো) করা যাবে না"))
            } else {
                Ok(Value::Dec(a % b))
            }
        }
        _ => unreachable!(),
    }
}

fn numeric_pair(l: Value, r: Value, pos: Pos) -> Result<(f64, f64), InterpError> {
    match (l, r) {
        (Value::Num(a), Value::Num(b)) => Ok((a as f64, b as f64)),
        (Value::Num(a), Value::Dec(b)) => Ok((a as f64, b)),
        (Value::Dec(a), Value::Num(b)) => Ok((a, b as f64)),
        (Value::Dec(a), Value::Dec(b)) => Ok((a, b)),
        _ => Err(err(pos, "এই অপারেটরের অপারেন্ড সংখ্যা হতে হবে")),
    }
}
