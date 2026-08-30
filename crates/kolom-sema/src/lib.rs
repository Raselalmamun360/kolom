use std::collections::HashMap;

use kolom_lexer::bn_num;

use kolom_syntax::ast::*;

pub type Diagnostic = kolom_lexer::Diagnostic;

#[derive(Debug, Clone)]
pub enum StdSig {
    Const(Ty),
    Fn(Vec<Ty>, Ty),
}

pub const STDLIB_MODULES: &[&str] = &[
    "গণিত",
    "লেখা",
    "ফাইল",
    "সময়",
    "র‍্যান্ডম",
    "ফাইলসিস্টেম",
    "পাথ",
    "জেসন",
    "নেটওয়ার্ক",
    "গ্রাফিক্স",
    "ম্যাট্রিক্স",
];

pub fn stdlib_lookup(module: &str, item: &str) -> Option<StdSig> {
    use StdSig::*;
    let d = || Ty::Dec;
    let i = || Ty::Num;
    let s = || Ty::Txt;
    let n = || Ty::Null;
    let b = || Ty::Bool;
    let sa = || Ty::Arr(Box::new(Ty::Txt));
    let da = || Ty::Arr(Box::new(Ty::Dec));
    let ma = || Ty::Arr(Box::new(Ty::Arr(Box::new(Ty::Dec))));
    Some(match (module, item) {
        ("গণিত", "পাই") => Const(d()),
        ("গণিত", "ই") => Const(d()),
        ("গণিত", "বর্গমূল") => Fn(vec![d()], d()),
        ("গণিত", "ঘাত") => Fn(vec![d(), d()], d()),
        ("গণিত", "ফ্লোর") => Fn(vec![d()], i()),
        ("গণিত", "সিলিং") => Fn(vec![d()], i()),
        ("গণিত", "রাউন্ডঅফ") => Fn(vec![d()], i()),
        ("গণিত", "সাইন") => Fn(vec![d()], d()),
        ("গণিত", "কোসাইন") => Fn(vec![d()], d()),
        ("গণিত", "ট্যান") => Fn(vec![d()], d()),
        // `লগ` is base ten, the way an unmarked "log" is read in Bangladeshi
        // schooling; the natural logarithm is spelled out. It used to be the
        // other way round, which quietly returned ln to anyone who wrote what
        // they were taught.
        ("গণিত", "লগ") => Fn(vec![d()], d()),
        ("গণিত", "লন") => Fn(vec![d()], d()),
        // These three take সংখ্যা or দশমিক — `check_math_overload` decides,
        // and intercepts the call before this signature is consulted. The
        // entries exist so `গণিত.সর্বনিম্ন` is still *found* when it appears
        // outside a call.
        ("গণিত", "পরম_মান") => Fn(vec![i()], i()),
        ("গণিত", "সর্বনিম্ন") => Fn(vec![i(), i()], i()),
        ("গণিত", "সর্বোচ্চ") => Fn(vec![i(), i()], i()),

        ("লেখা", "বড়হাতের") => Fn(vec![s()], s()),
        ("লেখা", "ছোটহাতের") => Fn(vec![s()], s()),
        ("লেখা", "ছাঁটো") => Fn(vec![s()], s()),
        ("লেখা", "স্প্লিট") => Fn(vec![s(), s()], sa()),
        ("লেখা", "জুড়াও") => Fn(vec![sa(), s()], s()),
        ("লেখা", "বদলাও") => Fn(vec![s(), s(), s()], s()),
        ("লেখা", "খুঁজো") => Fn(vec![s(), s()], i()),
        ("লেখা", "স্লাইস") => Fn(vec![s(), i(), i()], s()),
        ("লেখা", "শুরুতে_আছে") => Fn(vec![s(), s()], b()),
        ("লেখা", "শেষে_আছে") => Fn(vec![s(), s()], b()),

        ("ফাইল", "পড়ো") => Fn(vec![s()], s()),
        ("ফাইল", "লেখো") => Fn(vec![s(), s()], n()),
        // আগে `যোগ` নামে ছিল — docs জুড়ে "দুই সংখ্যা যোগ করা"-র উদাহরণ
        // ফাংশনের নামও `যোগ`, তাই বিভ্রান্তিকর ছিল যোগ-করা-র সাথে কোনো
        // সম্পর্ক না থাকা সত্ত্বেও। রানটাইম সিম্বল (`kl_io_append_file`)
        // অপরিবর্তিত — শুধু এই ম্যাচ-আর্মের লিটারেল বদলেছে।
        ("ফাইল", "এপেন্ড") => Fn(vec![s(), s()], n()),
        ("ফাইল", "লাইন_তালিকা") => Fn(vec![s()], sa()),

        ("সময়", "এখন_মিলিসেকেন্ড") => Fn(vec![], i()),
        ("সময়", "সেকেন্ড") => Fn(vec![], d()),

        ("র‍্যান্ডম", "বীজ") => Fn(vec![i()], n()),
        ("র‍্যান্ডম", "সংখ্যা") => Fn(vec![], i()),
        ("র‍্যান্ডম", "মধ্যে") => Fn(vec![i(), i()], i()),
        ("র‍্যান্ডম", "দশমিক") => Fn(vec![], d()),

        ("ফাইলসিস্টেম", "ফাইল_আছে") => Fn(vec![s()], b()),
        ("ফাইলসিস্টেম", "ডিরেক্টরি_আছে") => Fn(vec![s()], b()),
        ("ফাইলসিস্টেম", "ডিরেক্টরি_বানাও") => Fn(vec![s()], n()),
        // ফাইল-শুধু (ডিরেক্টরির উপর ব্যর্থ হয়) — রিকার্সিভ ডিলিটের জন্য
        // আলাদা, স্পষ্ট নাম `ডিরেক্টরি_মুছো` (নিচে), যাতে ভুলে গোটা ট্রি
        // মুছে না যায়।
        ("ফাইলসিস্টেম", "মুছো") => Fn(vec![s()], n()),
        ("ফাইলসিস্টেম", "ডিরেক্টরি_মুছো") => Fn(vec![s()], n()),
        ("ফাইলসিস্টেম", "তালিকা") => Fn(vec![s()], sa()),
        ("ফাইলসিস্টেম", "কপি") => Fn(vec![s(), s()], n()),
        ("ফাইলসিস্টেম", "ডিরেক্টরি_কপি") => Fn(vec![s(), s()], n()),
        ("ফাইলসিস্টেম", "সরাও") => Fn(vec![s(), s()], n()),
        ("ফাইলসিস্টেম", "আকার") => Fn(vec![s()], i()),
        ("ফাইলসিস্টেম", "পরিবর্তনের_সময়") => Fn(vec![s()], i()),
        ("ফাইলসিস্টেম", "বর্তমান_ডিরেক্টরি") => Fn(vec![], s()),

        ("পাথ", "জোড়ো") => Fn(vec![s(), s()], s()),
        ("পাথ", "ফাইলনাম") => Fn(vec![s()], s()),
        ("পাথ", "ডিরেক্টরিনাম") => Fn(vec![s()], s()),
        ("পাথ", "এক্সটেনশন") => Fn(vec![s()], s()),
        ("পাথ", "পরম_পাথ") => Fn(vec![s()], s()),

        ("জেসন", "বৈধ") => Fn(vec![s()], b()),
        ("জেসন", "বের_হও") => Fn(vec![s()], s()),
        ("জেসন", "লেখা_বের_করো") => Fn(vec![s(), s()], s()),
        ("জেসন", "সংখ্যা_বের_করো") => Fn(vec![s(), s()], i()),

        ("নেটওয়ার্ক", "কানেক্ট") => Fn(vec![s(), i()], i()),
        ("নেটওয়ার্ক", "সেন্ড") => Fn(vec![i(), s()], n()),
        ("নেটওয়ার্ক", "রিসিভ") => Fn(vec![i(), i()], s()),
        ("নেটওয়ার্ক", "ক্লোজ") => Fn(vec![i()], n()),

        // NOTE: map operations are global builtins (`ম্যাপ_তৈরি`,
        // `চাবি_গুলো`, `আছে_কি`, `চাবি_মুছো`), not members of a `ম্যাপ`
        // module — `ম্যাপ` is a type keyword and is not importable.

        ("গ্রাফিক্স", "রঙ") => Fn(vec![i(), i(), i()], n()),
        ("গ্রাফিক্স", "বিন্দু") => Fn(vec![i(), i()], n()),
        ("গ্রাফিক্স", "রেখা") => Fn(vec![i(), i(), i(), i()], n()),
        ("গ্রাফিক্স", "আয়ত") => Fn(vec![i(), i(), i(), i()], n()),
        ("গ্রাফিক্স", "ভরাট_আয়ত") => Fn(vec![i(), i(), i(), i()], n()),
        ("গ্রাফিক্স", "বৃত্ত") => Fn(vec![i(), i(), i()], n()),
        ("গ্রাফিক্স", "ভরাট_বৃত্ত") => Fn(vec![i(), i(), i()], n()),
        ("গ্রাফিক্স", "লেখা") => Fn(vec![i(), i(), s()], n()),
        ("গ্রাফিক্স", "ফন্ট") => Fn(vec![s(), i()], n()),

        // ম্যাট্রিক্স: ভেক্টর = দশমিক[], ম্যাট্রিক্স = দশমিক[][] (row-major)।
        // সংখ্যা[]/সংখ্যা[][] নেয় না — নির্ণায়ক/বিপরীত সবসময় ভগ্নাংশ ফল দিতে
        // পারে (গণিত.বর্গমূল-এর মতো), তাই ইনপুট-টাইপ যাই হোক আউটপুট সবসময়
        // দশমিক রাখাই সহজ ও সামঞ্জস্যপূর্ণ।
        ("ম্যাট্রিক্স", "ভেক্টর_যোগ") => Fn(vec![da(), da()], da()),
        ("ম্যাট্রিক্স", "ভেক্টর_বিয়োগ") => Fn(vec![da(), da()], da()),
        ("ম্যাট্রিক্স", "ভেক্টর_স্কেল") => Fn(vec![da(), d()], da()),
        ("ম্যাট্রিক্স", "ডট") => Fn(vec![da(), da()], d()),
        ("ম্যাট্রিক্স", "ক্রস") => Fn(vec![da(), da()], da()),
        ("ম্যাট্রিক্স", "নর্ম") => Fn(vec![da()], d()),

        ("ম্যাট্রিক্স", "যোগ") => Fn(vec![ma(), ma()], ma()),
        ("ম্যাট্রিক্স", "বিয়োগ") => Fn(vec![ma(), ma()], ma()),
        ("ম্যাট্রিক্স", "স্কেল") => Fn(vec![ma(), d()], ma()),
        ("ম্যাট্রিক্স", "গুণ") => Fn(vec![ma(), ma()], ma()),
        ("ম্যাট্রিক্স", "ট্রান্সপোজ") => Fn(vec![ma()], ma()),
        ("ম্যাট্রিক্স", "নির্ণায়ক") => Fn(vec![ma()], d()),
        ("ম্যাট্রিক্স", "বিপরীত") => Fn(vec![ma()], ma()),
        ("ম্যাট্রিক্স", "অভেদক") => Fn(vec![i()], ma()),
        ("ম্যাট্রিক্স", "শূন্য_ম্যাট্রিক্স") => Fn(vec![i(), i()], ma()),

        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Num,
    Dec,
    Txt,
    Bool,
    Ch,
    Null,
    Arr(Box<Ty>),
    Shared(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Struct(String),
    Unknown,
    Err,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Num => write!(f, "সংখ্যা"),
            Ty::Dec => write!(f, "দশমিক"),
            Ty::Txt => write!(f, "লেখা"),
            Ty::Bool => write!(f, "বুলিয়ান"),
            Ty::Ch => write!(f, "অক্ষর"),
            Ty::Null => write!(f, "ফাঁকা"),
            Ty::Arr(t) => write!(f, "{}[]", t),
            Ty::Shared(t) => write!(f, "শেয়ার {}", t),
            Ty::Map(k, v) => write!(f, "ম্যাম[{}, {}]", k, v),
            Ty::Struct(name) => write!(f, "{}", name),
            Ty::Unknown => write!(f, "অজানা"),
            Ty::Err => write!(f, "ত্রুটি"),
        }
    }
}

fn is_move_ty(t: &Ty) -> bool {
    matches!(t, Ty::Txt | Ty::Arr(_) | Ty::Map(_, _))
}

fn poisoned(t: &Ty) -> bool {
    matches!(t, Ty::Err | Ty::Unknown)
}

fn unify(a: &Ty, b: &Ty) -> bool {
    a == b || poisoned(a) || poisoned(b)
}

#[derive(Debug, Clone)]
struct Binding {
    ty: Ty,
    is_const: bool,
    moved: bool,
    moved_by: String,
}

#[derive(Debug, Clone)]
struct FnSig {
    params: Vec<Ty>,
    ret: Ty,
}

#[derive(Debug, Default)]
pub struct Types {
    pub expr: HashMap<usize, Ty>,
    pub decl: HashMap<usize, Ty>,
    pub funcs: HashMap<String, (Vec<Ty>, Ty)>,
}

impl Types {
    pub fn expr_of(&self, e: &Expr) -> Ty {
        self.expr
            .get(&(e as *const Expr as usize))
            .cloned()
            .unwrap_or(Ty::Unknown)
    }

    pub fn decl_of(&self, id: &Ident) -> Ty {
        self.decl
            .get(&(id as *const Ident as usize))
            .cloned()
            .unwrap_or(Ty::Unknown)
    }
}

struct Ck {
    diags: Vec<Diagnostic>,
    funcs: HashMap<String, FnSig>,
    scopes: Vec<HashMap<String, Binding>>,
    cur_ret: Option<Ty>,
    types: Types,
    imports: std::collections::HashSet<String>,
    structs: HashMap<String, Vec<(String, Ty)>>,
    /// Names of all declared `তথ্য` types, collected before any field type
    /// is resolved so a struct may reference one declared later in the file.
    struct_names: std::collections::HashSet<String>,
    /// Imports that name one of the user's own `.ক` files rather than a
    /// standard-library module. Their contents are merged into this program
    /// before analysis, so the module name itself is never a value — which
    /// makes `helper.foo()` an error, and this set is what lets that error
    /// say so instead of claiming the import is missing.
    user_imports: std::collections::HashSet<String>,
}

pub fn analyze(prog: &Program) -> Vec<Diagnostic> {
    analyze_typed(prog).0
}

pub fn analyze_typed(prog: &Program) -> (Vec<Diagnostic>, Types) {
    let mut ck = Ck {
        diags: Vec::new(),
        funcs: HashMap::new(),
        scopes: vec![HashMap::new()],
        cur_ret: None,
        types: Types::default(),
        imports: std::collections::HashSet::new(),
        structs: HashMap::new(),
        struct_names: std::collections::HashSet::new(),
        user_imports: std::collections::HashSet::new(),
    };
    ck.check_program(prog);
    let types = std::mem::take(&mut ck.types);
    (ck.diags, types)
}

impl Ck {
    fn err(&mut self, pos: Pos, msg: impl Into<String>) {
        self.diags.push(Diagnostic {
            line: pos.line,
            col: pos.col,
            message: msg.into(),
        });
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        for s in self.scopes.iter().rev() {
            if let Some(b) = s.get(name) {
                return Some(b);
            }
        }
        None
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Binding> {
        for s in self.scopes.iter_mut().rev() {
            if s.contains_key(name) {
                return s.get_mut(name);
            }
        }
        None
    }

    /// If `var` is a local binding of struct type with a field `field`,
    /// returns that field's type. Used to tell `struct_var.field` apart from
    /// `module.item`, which are syntactically identical.
    fn local_struct_field(&self, var: &str, field: &str) -> Option<Ty> {
        let b = self.lookup(var)?;
        let Ty::Struct(sname) = &b.ty else { return None };
        let fields = self.structs.get(sname)?;
        fields.iter().find(|(n, _)| n == field).map(|(_, t)| t.clone())
    }

    /// Rejects structs that contain themselves *by value*, directly or
    /// through a chain of struct fields — such a type would need infinite
    /// space. Reaching itself through a `শেয়ার`, array, or map field is
    /// fine: those are pointers, which is exactly how recursive data
    /// structures (lists, trees, ASTs) are built.
    fn check_struct_cycles(&mut self, prog: &Program) {
        for decl in &prog.structs {
            let start = &decl.name.name;
            let mut seen = std::collections::HashSet::new();
            if let Some(path) = self.value_cycle_from(start, start, &mut seen) {
                self.err(
                    decl.name.pos,
                    format!(
                        "'{}' নিজেকেই ধারণ করছে ({}) — অসীম আকার।                          পুনরাবৃত্ত গঠনের জন্য 'শেয়ার {}' বা '{}[]' ব্যবহার করুন",
                        start, path, start, start
                    ),
                );
            }
        }
    }

    /// Depth-first search for a by-value path from `current` back to `start`.
    /// Returns the field path that closes the cycle, for the error message.
    fn value_cycle_from(
        &self,
        start: &str,
        current: &str,
        seen: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        if !seen.insert(current.to_string()) {
            return None;
        }
        let fields = self.structs.get(current)?.clone();
        for (fname, fty) in fields {
            // Only Struct fields are stored inline; Shared/Arr/Map are
            // pointers and therefore break the size recursion.
            if let Ty::Struct(next) = fty {
                if next == start {
                    return Some(format!("{}.{}", current, fname));
                }
                if let Some(rest) = self.value_cycle_from(start, &next, seen) {
                    return Some(format!("{}.{} -> {}", current, fname, rest));
                }
            }
        }
        None
    }

    fn resolve_type(&mut self, te: &TypeExpr) -> Ty {
        match te {
            TypeExpr::Named(id) => match id.name.as_str() {
                "সংখ্যা" => Ty::Num,
                "দশমিক" => Ty::Dec,
                "লেখা" => Ty::Txt,
                "বুলিয়ান" => Ty::Bool,
                "অক্ষর" => Ty::Ch,
                "ফাঁকা" => Ty::Null,
                other => {
                    if self.struct_names.contains(other) {
                        Ty::Struct(other.to_string())
                    } else {
                        self.err(
                            id.pos,
                            format!("'{}' অজানা টাইপ — প্রিমিটিভ টাইপ বা ঘোষিত 'তথ্য' ব্যবহার করুন", other),
                        );
                        Ty::Err
                    }
                }
            },
            TypeExpr::Array(inner) => Ty::Arr(Box::new(self.resolve_type(inner))),
            TypeExpr::Shared(inner) => Ty::Shared(Box::new(self.resolve_type(inner))),
            TypeExpr::Map(k, v) => {
                let kt = self.resolve_type(k);
                if kt != Ty::Txt && kt != Ty::Num && !poisoned(&kt) {
                    self.err(
                        Pos { line: 0, col: 0 },
                        format!("ম্যাপ key হতে হবে 'লেখা' বা 'সংখ্যা', '{}' নয়", kt),
                    );
                }
                Ty::Map(Box::new(kt), Box::new(self.resolve_type(v)))
            }
        }
    }

    /// Why `module.item` could not be resolved. An import the user wrote
    /// themselves needs a different answer from one they forgot: their own
    /// module's functions are called by bare name, because its declarations
    /// are merged into this program rather than kept behind a namespace.
    fn unknown_module_msg(&self, module: &str) -> String {
        if self.user_imports.contains(module) {
            format!(
                "'{}' আপনার নিজের মডিউল — এর ফাংশন ও ধ্রুবক সরাসরি নাম দিয়ে ব্যবহার করুন, '{}.' ছাড়া",
                module, module
            )
        } else {
            format!(
                "মডিউল '{}' ইম্পোর্ট করা হয়নি — ফাইলের শুরুতে 'ইম্পোর্ট {}' দিন",
                module, module
            )
        }
    }

    fn check_program(&mut self, prog: &Program) {
        for imp in &prog.imports {
            if STDLIB_MODULES.contains(&imp.name.as_str()) {
                if !self.imports.insert(imp.name.clone()) {
                    self.err(
                        imp.pos,
                        format!("'{}' আগেই ইম্পোর্ট করা হয়েছে", imp.name),
                    );
                }
            } else {
                self.user_imports.insert(imp.name.clone());
            }
        }
        // Pass 1: every `তথ্য` name, so field types below may refer to a
        // struct declared later in the file (and to itself, through a
        // pointer-like `শেয়ার`/array field).
        for s in &prog.structs {
            self.struct_names.insert(s.name.name.clone());
        }
        // Pass 2: resolve field types now that all names are known.
        for s in &prog.structs {
            let fields: Vec<(String, Ty)> = s
                .fields
                .iter()
                .map(|(n, t)| (n.name.clone(), self.resolve_type(t)))
                .collect();
            if self.structs.insert(s.name.name.clone(), fields).is_some() {
                self.err(
                    s.name.pos,
                    format!("দুটি তথ্যের একই নাম — '{}'", s.name.name),
                );
            }
        }
        self.check_struct_cycles(prog);
        for f in &prog.funcs {
            let sig = FnSig {
                params: f.params.iter().map(|p| self.resolve_type(&p.ty)).collect(),
                ret: self.resolve_type(&f.ret),
            };
            if self.funcs.insert(f.name.name.clone(), sig).is_some() {
                self.err(
                    f.name.pos,
                    format!("দুটি ফাংশনের একই নাম — '{}'", f.name.name),
                );
            }
        }
        for f in &prog.funcs {
            let sig = self.funcs.get(&f.name.name).unwrap();
            let entry = (
                sig.params.clone(),
                sig.ret.clone(),
            );
            self.types.funcs.insert(f.name.name.clone(), entry);
        }
        for c in &prog.consts {
            let ty = self.resolve_type(&c.ty);
            self.types
                .decl
                .insert(&c.name as *const Ident as usize, ty.clone());
            self.scopes.last_mut().unwrap().insert(
                c.name.name.clone(),
                Binding {
                    ty,
                    is_const: true,
                    moved: false,
                    moved_by: String::new(),
                },
            );
        }
        for c in &prog.consts {
            let t = self.expr(&c.init).unwrap_or(Ty::Unknown);
            self.try_move_src(&c.init, &c.name.name);
            let ann = self
                .lookup(&c.name.name)
                .map(|b| b.ty.clone())
                .unwrap_or(Ty::Err);
            if !unify(&ann, &t) && !matches!(t, Ty::Unknown) {
                self.err(
                    c.name.pos,
                    format!(
                        "'{}' ঘোষণায় '{}' চাওয়া হয়েছে, কিন্তু মানটি '{}'",
                        c.name.name, ann, t
                    ),
                );
            }
        }
        for f in &prog.funcs {
            self.check_fn(f);
        }
        match &prog.app {
            Some(app) => {
                let has_display = app.body.stmts.iter().any(|s| matches!(s, Stmt::Display(_)));
                if has_display {
                    for st in &app.body.stmts {
                        self.check_widget_outside_display(st);
                    }
                }
                self.check_block(&app.body);
            }
            None => self.err(
                Pos { line: 1, col: 1 },
                "কোনো 'অ্যাপ' ডিক্লারেশন পাওয়া যায়নি — এন্ট্রি পয়েন্ট আবশ্যক",
            ),
        }
    }

    fn check_tick(&mut self, pos: Pos, args: &[Expr]) -> Ty {
        if args.len() != 2 {
            self.err(
                pos,
                format!(
                    "'টিক' ২টি আর্গুমেন্ট নেয় — (মিলিসেকেন্ড, হ্যান্ডলার); {}টি পেয়েছে",
                    bn_num(args.len() as u32)
                ),
            );
            return Ty::Null;
        }
        let mt = self.expr(&args[0]).unwrap_or(Ty::Unknown);
        if !poisoned(&mt) && mt != Ty::Num {
            self.err(args[0].pos, format!("'টিক'-এর প্রথম আর্গুমেন্ট 'সংখ্যা' হতে হবে, পেয়েছে '{}'", mt));
        }
        if let ExprKind::Ident(h) = &args[1].kind {
            match self.types.funcs.get(&h.name) {
                Some((params, _)) if params.is_empty() => {}
                Some((params, _)) => self.err(
                    args[1].pos,
                    format!(
                        "হ্যান্ডলার '{}' শূন্য-প্যারামিটার ফাংশন হতে হবে, {}টি প্যারামিটার",
                        h.name,
                        bn_num(params.len() as u32)
                    ),
                ),
                None => self.err(args[1].pos, format!("অজানা হ্যান্ডলার ফাংশন '{}'", h.name)),
            }
        } else {
            self.err(args[1].pos, "'টিক'-এর দ্বিতীয় আর্গুমেন্ট হ্যান্ডলার ফাংশনের নাম".to_string());
        }
        Ty::Null
    }

    fn check_widget_outside_display(&mut self, s: &Stmt) {
        match s {
            Stmt::Widget(w) => self.err(
                w.pos,
                "'ডিসপ্লে' থাকলে উইজেটগুলো তার ভেতরে দাও — 'অ্যাপ'-বডিতে সরাসরি নয়".to_string(),
            ),
            Stmt::If(i) => {
                for st in &i.then.stmts {
                    self.check_widget_outside_display(st);
                }
                if let Some(e) = &i.els {
                    match e {
                        ElseBranch::Block(b) => {
                            for st in &b.stmts {
                                self.check_widget_outside_display(st);
                            }
                        }
                        ElseBranch::If(inner) => {
                            let wrapped = [Stmt::If((**inner).clone())];
                            for st in &wrapped {
                                self.check_widget_outside_display(st);
                            }
                        }
                    }
                }
            }
            Stmt::Loop(l) => {
                for st in &l.body.stmts {
                    self.check_widget_outside_display(st);
                }
            }
            Stmt::While(w2) => {
                for st in &w2.body.stmts {
                    self.check_widget_outside_display(st);
                }
            }
            Stmt::ForEach(fe) => {
                for st in &fe.body.stmts {
                    self.check_widget_outside_display(st);
                }
            }
            Stmt::Nested(b) => {
                for st in &b.stmts {
                    self.check_widget_outside_display(st);
                }
            }
            _ => {}
        }
    }

    fn check_fn(&mut self, f: &FuncDecl) {
        self.scopes.push(HashMap::new());
        let params: Vec<(String, Ty)> = f
            .params
            .iter()
            .map(|p| (p.name.name.clone(), self.resolve_type(&p.ty)))
            .collect();
        for (name, ty) in &params {
            let dup = self.scopes.last().unwrap().contains_key(name);
            if dup {
                self.err(f.name.pos, format!("প্যারামিটার '{}' দুবার ঘোষিত", name));
                continue;
            }
            self.scopes.last_mut().unwrap().insert(
                name.clone(),
                Binding {
                    ty: ty.clone(),
                    is_const: false,
                    moved: false,
                    moved_by: String::new(),
                },
            );
            if let Some(p) = f.params.iter().find(|p| &p.name.name == name) {
                let key = &p.name as *const Ident as usize;
                self.types.decl.insert(key, ty.clone());
            }
        }
        self.cur_ret = self.funcs.get(&f.name.name).map(|s| s.ret.clone());
        for st in &f.body.stmts {
            let _ = self.check_stmt(st);
        }
        self.cur_ret = None;
        self.scopes.pop();
    }

    fn check_block(&mut self, b: &Block) {
        self.scopes.push(HashMap::new());
        for st in &b.stmts {
            let _ = self.check_stmt(st);
        }
        self.scopes.pop();
    }

    fn snapshot(&self) -> Vec<HashMap<String, Binding>> {
        self.scopes.clone()
    }

    fn restore(&mut self, snap: Vec<HashMap<String, Binding>>) {
        self.scopes = snap;
    }

    fn merge_branches(&mut self, a: &[HashMap<String, Binding>], b: &[HashMap<String, Binding>]) {
        for (ia, sa) in a.iter().enumerate() {
            if let Some(sb) = b.get(ia) {
                if let Some(live) = self.scopes.get_mut(ia) {
                    for (name, ba) in sa.iter() {
                        if let Some(bb) = sb.get(name) {
                            if let Some(lb) = live.get_mut(name) {
                                lb.moved = ba.moved || bb.moved;
                                if lb.moved {
                                    lb.moved_by = if ba.moved {
                                        ba.moved_by.clone()
                                    } else {
                                        bb.moved_by.clone()
                                    };
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn define_var(&mut self, name: &Ident, ty: Ty, is_const: bool) {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(&name.name) {
            self.err(name.pos, format!("'{}' এই স্কোপে আগেই ঘোষিত", name.name));
            return;
        }
        scope.insert(
            name.name.clone(),
            Binding {
                ty,
                is_const,
                moved: false,
                moved_by: String::new(),
            },
        );
    }

    fn try_move_src(&mut self, src: &Expr, dest: &str) {
        if let ExprKind::Ident(id) = &src.kind {
            let is_mv = self
                .lookup(&id.name)
                .map(|b| is_move_ty(&b.ty))
                .unwrap_or(false);
            if is_mv {
                if let Some(b) = self.lookup_mut(&id.name) {
                    b.moved = true;
                    b.moved_by = dest.to_string();
                }
            }
        }
    }

    fn check_stmt(&mut self, s: &Stmt) -> Ty {
        match s {
            Stmt::Var(v) => {
                let t = self.expr(&v.init).unwrap_or(Ty::Unknown);
                self.try_move_src(&v.init, &v.name.name);
                let final_ty = match &v.ty {
                    Some(te) => {
                        let ann = self.resolve_type(te);
                        if !unify(&ann, &t) && !matches!(t, Ty::Unknown) {
                            self.err(
                                v.name.pos,
                                format!(
                                    "'{}' ঘোষণায় '{}' চাওয়া হয়েছে, কিন্তু মানটি '{}'",
                                    v.name.name, ann, t
                                ),
                            );
                        }
                        ann
                    }
                    None => {
                        if matches!(t, Ty::Unknown) {
                            self.err(
                                v.name.pos,
                                "খালি অ্যারের টাইপ অনুমান করা যায় না — ঘোষণায় ': টাইপ[]' দিন"
                                    .to_string(),
                            );
                            Ty::Err
                        } else {
                            t
                        }
                    }
                };
                self.define_var(&v.name, final_ty.clone(), false);
                self.types
                    .decl
                    .insert(&v.name as *const Ident as usize, final_ty);
                Ty::Null
            }
            Stmt::Const(c) => {
                let t = self.expr(&c.init).unwrap_or(Ty::Unknown);
                self.try_move_src(&c.init, &c.name.name);
                let ann = self.resolve_type(&c.ty);
                if !unify(&ann, &t) && !matches!(t, Ty::Unknown) {
                    self.err(
                        c.name.pos,
                        format!(
                            "'{}' ঘোষণায় '{}' চাওয়া হয়েছে, কিন্তু মানটি '{}'",
                            c.name.name, ann, t
                        ),
                    );
                }
                self.define_var(&c.name, ann.clone(), true);
                self.types
                    .decl
                    .insert(&c.name as *const Ident as usize, ann);
                Ty::Null
            }
            Stmt::If(i) => {
                let ct = self.expr(&i.cond).unwrap_or(Ty::Unknown);
                if !poisoned(&ct) && ct != Ty::Bool {
                    self.err(
                        i.pos,
                        format!("'যদি'-এর শর্ত 'বুলিয়ান' টাইপের হতে হবে, পেয়েছে '{}'", ct),
                    );
                }
                let pre = self.snapshot();
                self.check_block(&i.then);
                let after_then = self.snapshot();
                self.restore(pre.clone());
                if let Some(els) = &i.els {
                    match els {
                        ElseBranch::Block(b) => self.check_block(b),
                        ElseBranch::If(inner) => {
                            let _ = self.check_stmt(&Stmt::If((**inner).clone()));
                        }
                    }
                }
                let after_else = self.snapshot();
                self.restore(pre);
                self.merge_branches(&after_then, &after_else);
                Ty::Null
            }
            Stmt::Loop(l) => {
                let ct = self.expr(&l.count).unwrap_or(Ty::Unknown);
                if !poisoned(&ct) && ct != Ty::Num {
                    self.err(
                        l.pos,
                        format!(
                            "'লুপ'-এর সংখ্যা 'সংখ্যা' টাইপের হতে হবে, পেয়েছে '{}'",
                            ct
                        ),
                    );
                }
                self.check_block(&l.body);
                Ty::Null
            }
            Stmt::While(w) => {
                let ct = self.expr(&w.cond).unwrap_or(Ty::Unknown);
                if !poisoned(&ct) && ct != Ty::Bool {
                    self.err(
                        w.pos,
                        format!(
                            "'যতক্ষণ'-এর শর্ত 'বুলিয়ান' টাইপের হতে হবে, পেয়েছে '{}'",
                            ct
                        ),
                    );
                }
                self.check_block(&w.body);
                Ty::Null
            }
            Stmt::ForEach(fe) => {
                let it = self.expr(&fe.iter).unwrap_or(Ty::Unknown);
                let elem = match it {
                    Ty::Arr(t) => *t,
                    Ty::Unknown | Ty::Err => Ty::Unknown,
                    other => {
                        self.err(
                            fe.pos,
                            format!("'প্রতি'-তে অ্যারে প্রত্যাশিত, পেয়েছে '{}'", other),
                        );
                        Ty::Err
                    }
                };
                self.types
                    .decl
                    .insert(&fe.var as *const Ident as usize, elem.clone());
                let mut sc = HashMap::new();
                sc.insert(
                    fe.var.name.clone(),
                    Binding {
                        ty: elem,
                        is_const: false,
                        moved: false,
                        moved_by: String::new(),
                    },
                );
                self.scopes.push(sc);
                for st in &fe.body.stmts {
                    let _ = self.check_stmt(st);
                }
                self.scopes.pop();
                Ty::Null
            }
            Stmt::Return(r) => {
                let expected = self.cur_ret.clone().unwrap_or(Ty::Null);
                match &r.value {
                    Some(e) => {
                        let t = self.expr(e).unwrap_or(Ty::Unknown);
                        self.try_move_src(e, "রিটার্ন");
                        if !unify(&expected, &t) {
                            self.err(
                                r.pos,
                                format!(
                                    "'রিটার্ন' টাইপ অমিল — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                                    expected, t
                                ),
                            );
                        }
                    }
                    None => {
                        if expected != Ty::Null && !poisoned(&expected) {
                            self.err(
                                r.pos,
                                format!(
                                    "এই ফাংশন '{}' ফেরত দেওয়ার কথা — 'রিটার্ন'-এর পরে মান দিন",
                                    expected
                                ),
                            );
                        }
                    }
                }
                Ty::Null
            }
            Stmt::Break(_) | Stmt::Continue(_) => Ty::Null,
            Stmt::Expr(e) => self.expr(e).unwrap_or(Ty::Unknown),
            Stmt::Nested(b) => {
                self.check_block(b);
                Ty::Null
            }
            Stmt::Widget(w) => {
                match w.kw.as_str() {
                    "ইনপুট" => {
                        if !w.args.is_empty() {
                            self.err(w.pos, "'ইনপুট' কোনো আর্গুমেন্ট নেয় না".to_string());
                        }
                    }
                    "টেক্সট" => {
                        if w.args.len() != 1 {
                            self.err(
                                w.pos,
                                format!(
                                    "'টেক্সট' ঠিক ১টি 'লেখা' আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                                    bn_num(w.args.len() as u32)
                                ),
                            );
                        } else {
                            let t = self.expr(&w.args[0]).unwrap_or(Ty::Unknown);
                            if !poisoned(&t) && t != Ty::Txt {
                                self.err(
                                    w.args[0].pos,
                                    format!("'টেক্সট'-এর আর্গুমেন্ট 'লেখা' হতে হবে, পেয়েছে '{}'", t),
                                );
                            }
                        }
                    }
                    "বাটন" => {
                        if w.args.is_empty() || w.args.len() > 2 {
                            self.err(
                                w.pos,
                                format!(
                                    "'বাটন' ১টি 'লেখা' ও (ঐচ্ছিক) ১টি হ্যান্ডলার নেয়, {}টি পেয়েছে",
                                    bn_num(w.args.len() as u32)
                                ),
                            );
                        } else {
                            let t = self.expr(&w.args[0]).unwrap_or(Ty::Unknown);
                            if !poisoned(&t) && t != Ty::Txt {
                                self.err(
                                    w.args[0].pos,
                                    format!("'বাটন'-এর লেবেল 'লেখা' হতে হবে, পেয়েছে '{}'", t),
                                );
                            }
                            if w.args.len() == 2 {
                                if let ExprKind::Ident(h) = &w.args[1].kind {
                                    match self.types.funcs.get(&h.name) {
                                        Some((params, _)) if params.is_empty() => {}
                                        Some((params, _)) => self.err(
                                            w.args[1].pos,
                                            format!(
                                                "হ্যান্ডলার '{}' শূন্য-প্যারামিটার ফাংশন হতে হবে, {}টি প্যারামিটার",
                                                h.name,
                                                bn_num(params.len() as u32)
                                            ),
                                        ),
                                        None => self.err(
                                            w.args[1].pos,
                                            format!("অজানা হ্যান্ডলার ফাংশন '{}'", h.name),
                                        ),
                                    }
                                } else {
                                    self.err(w.args[1].pos, "'বাটন'-এর দ্বিতীয় আর্গুমেন্ট হ্যান্ডলার ফাংশনের নাম".to_string());
                                }
                            }
                        }
                    }
                    "ক্যানভাস" => {
                        if w.args.len() != 2 {
                            self.err(
                                w.pos,
                                format!(
                                    "'ক্যানভাস' ২টি 'সংখ্যা' আর্গুমেন্ট নেয় (প্রস্থ, উচ্চতা), {}টি পেয়েছে",
                                    bn_num(w.args.len() as u32)
                                ),
                            );
                        } else {
                            for (i, a) in w.args.iter().enumerate() {
                                let t = self.expr(a).unwrap_or(Ty::Unknown);
                                if !poisoned(&t) && t != Ty::Num {
                                    self.err(
                                        a.pos,
                                        format!(
                                            "'ক্যানভাস'-এর আর্গুমেন্ট #{} 'সংখ্যা' হতে হবে, পেয়েছে '{}'",
                                            bn_num((i + 1) as u32),
                                            t
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    "ছবি" => {
                        if w.args.len() != 1 {
                            self.err(
                                w.pos,
                                format!(
                                    "'ছবি' ঠিক ১টি 'লেখা' আর্গুমেন্ট (ফাইলপথ) নেয়, {}টি পেয়েছে",
                                    bn_num(w.args.len() as u32)
                                ),
                            );
                        } else {
                            let t = self.expr(&w.args[0]).unwrap_or(Ty::Unknown);
                            if !poisoned(&t) && t != Ty::Txt {
                                self.err(
                                    w.args[0].pos,
                                    format!("'ছবি'-এর আর্গুমেন্ট 'লেখা' হতে হবে, পেয়েছে '{}'", t),
                                );
                            }
                        }
                    }
                    _ => {
                        for a in &w.args {
                            let _ = self.expr(a);
                        }
                    }
                }
                if let Some(b) = &w.body {
                    self.check_block(b);
                }
                Ty::Null
            }
            Stmt::TryCatch(tc) => {
                self.check_block(&tc.body);
                self.scopes.push(HashMap::new());
                self.scopes.last_mut().unwrap().insert(
                    tc.err_var.name.clone(),
                    Binding {
                        ty: Ty::Txt,
                        is_const: false,
                        moved: false,
                        moved_by: String::new(),
                    },
                );
                self.check_block(&tc.handler);
                self.scopes.pop();
                Ty::Null
            }
            Stmt::Display(_) => Ty::Null,
        }
    }

    fn expr(&mut self, e: &Expr) -> Option<Ty> {
        let t = self.expr_inner(e);
        let key = e as *const Expr as usize;
        let recorded = t.clone().unwrap_or(Ty::Unknown);
        self.types.expr.insert(key, recorded);
        t
    }

    fn expr_inner(&mut self, e: &Expr) -> Option<Ty> {
        Some(match &e.kind {
            ExprKind::Lit(l) => match l {
                Lit::Int(_) => Ty::Num,
                Lit::Float(_) => Ty::Dec,
                Lit::Str(_) => Ty::Txt,
                Lit::Char(_) => Ty::Ch,
                Lit::Bool(_) => Ty::Bool,
                Lit::Null => Ty::Null,
                Lit::Array(items) => {
                    if items.is_empty() {
                        return None;
                    }
                    let mut elem: Option<Ty> = None;
                    for it in items {
                        let t = self.expr(it).unwrap_or(Ty::Unknown);
                        match &elem {
                            None => elem = Some(t),
                            Some(prev) => {
                                if !unify(prev, &t) {
                                    self.err(
                                        it.pos,
                                        format!(
                                            "অ্যারের সব এলিমেন্ট একই টাইপের হতে হবে — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                                            prev, t
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    Ty::Arr(Box::new(elem.unwrap_or(Ty::Err)))
                }
            },
            ExprKind::Ident(id) => {
                if matches!(
                    id.name.as_str(),
                    "লেখো" | "দৈর্ঘ্য" | "কপি" | "শেয়ার_করো" | "মান" | "বসাও" | "লেখায়"
                        | "ম্যাপ_তৈরি" | "চাবি_গুলো" | "আছে_কি" | "চাবি_মুছো" | "পড়ো_লাইন"
                ) || self.funcs.contains_key(&id.name) || self.structs.contains_key(&id.name)
                {
                    return Some(Ty::Unknown);
                }
                let info = match self.lookup(&id.name) {
                    None => None,
                    Some(b) => Some((b.ty.clone(), b.moved, b.moved_by.clone())),
                };
                match info {
                    None => {
                        self.err(id.pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", id.name));
                        Ty::Err
                    }
                    Some((ty, moved, moved_by)) => {
                        if moved {
                            self.err(
                                id.pos,
                                format!(
                                    "'{}' ইতিমধ্যে '{}'-তে মুভ হয়ে গেছে। মুভ হওয়ার পর '{}' ব্যবহার করা যাবে না।",
                                    id.name, moved_by, id.name
                                ),
                            );
                        }
                        ty
                    }
                }
            }
            ExprKind::Qualified { module, name } => {
                // Check if module is actually a local struct variable → field access
                if let Some(b) = self.lookup(&module.name) {
                    if let Ty::Struct(sname) = &b.ty {
                        if let Some(fields) = self.structs.get(sname) {
                            match fields.iter().find(|(n, _)| n == &name.name) {
                                Some((_, ft)) => return Some(ft.clone()),
                                None => {
                                    self.err(
                                        name.pos,
                                        format!("'{}' তথ্যে ফিল্ড '{}' নেই", sname, name.name),
                                    );
                                    return Some(Ty::Err);
                                }
                            }
                        }
                    }
                }
                if !self.imports.contains(&module.name) {
                    let msg = self.unknown_module_msg(&module.name);
                    self.err(module.pos, msg);
                    return None;
                }
                match stdlib_lookup(&module.name, &name.name) {
                    None => {
                        self.err(
                            name.pos,
                            format!("মডিউল '{}'-এ '{}' নেই", module.name, name.name),
                        );
                        Ty::Err
                    }
                    Some(StdSig::Const(t)) => t,
                    Some(StdSig::Fn(_, _)) => {
                        self.err(
                            name.pos,
                            format!(
                                "'{}.{}' একটি ফাংশন — কল করে ব্যবহার করুন",
                                module.name, name.name
                            ),
                        );
                        Ty::Err
                    }
                }
            }
            ExprKind::FieldAssign(base, field, rhs) => {
                let rt = self.expr(rhs).unwrap_or(Ty::Unknown);
                let bt = match self.lookup(&base.name) {
                    Some(b) => b.ty.clone(),
                    None => {
                        self.err(base.pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", base.name));
                        return Some(Ty::Null);
                    }
                };
                if let Ty::Struct(sname) = &bt {
                    if let Some(fields) = self.structs.get(sname) {
                        if let Some((_, fty)) = fields.iter().find(|(n, _)| n == &field.name) {
                            if !unify(fty, &rt) {
                                self.err(
                                    field.pos,
                                    format!(
                                        "ফিল্ড '{}' টাইপ অমিল — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                                        field.name, fty, rt
                                    ),
                                );
                            }
                        } else {
                            self.err(field.pos, format!("'{}' তথ্যে ফিল্ড '{}' নেই", sname, field.name));
                        }
                    }
                } else {
                    self.err(base.pos, format!("'{}' তথ্য নয়", base.name));
                }
                Ty::Null
            }
            ExprKind::Unary(op, inner) => {
                let t = self.expr(inner).unwrap_or(Ty::Unknown);
                match op {
                    UnaryOp::Neg => {
                        if poisoned(&t) {
                            Ty::Unknown
                        } else if t == Ty::Num || t == Ty::Dec {
                            t
                        } else {
                            self.err(e.pos, "ইউনারি '-'-এর অপারেন্ড সংখ্যা হতে হবে");
                            Ty::Err
                        }
                    }
                    UnaryOp::Not => {
                        if poisoned(&t) {
                            Ty::Unknown
                        } else if t == Ty::Bool {
                            Ty::Bool
                        } else {
                            self.err(e.pos, "'না'-এর অপারেন্ড 'বুলিয়ান' হতে হবে");
                            Ty::Err
                        }
                    }
                }
            }
            ExprKind::Binary(op, l, r) => {
                let lt = self.expr(l).unwrap_or(Ty::Unknown);
                let rt = self.expr(r).unwrap_or(Ty::Unknown);
                self.binary(*op, lt, rt, e.pos)
            }
            ExprKind::Assign(target, rhs) => {
                let rt = self.expr(rhs).unwrap_or(Ty::Unknown);
                self.check_assign(target, rhs, rt);
                Ty::Null
            }
            ExprKind::Postfix(base, sfx) => {
                let mut callable: Option<String> = match &base.kind {
                    ExprKind::Ident(id) => Some(id.name.clone()),
                    ExprKind::Qualified { module, name } => {
                        // `a.b` is a module item only when `a` is not a local
                        // struct variable; otherwise it is a field read that
                        // more suffixes may chain onto (`ব.ভি.মান`).
                        if self.local_struct_field(&module.name, &name.name).is_some() {
                            None
                        } else {
                            if !self.imports.contains(&module.name) {
                                let msg = self.unknown_module_msg(&module.name);
                                self.err(module.pos, msg);
                            }
                            Some(format!("{}::{}", module.name, name.name))
                        }
                    }
                    _ => None,
                };
                let mut cur = match &base.kind {
                    ExprKind::Ident(id) => {
                        let info = self
                            .lookup(&id.name)
                            .map(|b| (b.ty.clone(), b.moved, b.moved_by.clone()));
                        match info {
                            None => {
                                if !matches!(
                                    id.name.as_str(),
                                    "লেখো"
                                        | "দৈর্ঘ্য"
                                        | "কপি"
                                        | "শেয়ার_করো"
                                        | "মান"
                                        | "বসাও"
                                        | "লেখায়"
                                        | "ম্যাপ_তৈরি"
                                        | "চাবি_গুলো"
                                        | "আছে_কি"
                                        | "চাবি_মুছো"
                                        | "পড়ো_লাইন"
                                ) && !self.funcs.contains_key(&id.name) && !self.structs.contains_key(&id.name)
                                {
                                    self.err(
                                        id.pos,
                                        format!("অঘোষিত ভ্যারিয়েবল '{}'", id.name),
                                    );
                                }
                                Ty::Unknown
                            }
                            Some((ty, moved, moved_by)) => {
                                if moved {
                                    self.err(
                                        id.pos,
                                        format!(
                                            "'{}' ইতিমধ্যে '{}'-তে মুভ হয়ে গেছে। মুভ হওয়ার পর '{}' ব্যবহার করা যাবে না।",
                                            id.name, moved_by, id.name
                                        ),
                                    );
                                }
                                ty
                            }
                        }
                    }
                    ExprKind::Qualified { module, name } => {
                        match stdlib_lookup(&module.name, &name.name) {
                            Some(StdSig::Const(t)) => t,
                            _ => Ty::Unknown,
                        }
                    }
                    _ => self.expr(base).unwrap_or(Ty::Unknown),
                };
                for s in sfx {
                    match s {
                        Suffix::Call(args, cpos) => {
                            let name = match callable.take() {
                                Some(n) => n,
                                None => {
                                    self.err(*cpos, "এটি কলযোগ্য ফাংশন নয়");
                                    cur = Ty::Err;
                                    continue;
                                }
                            };
                            cur = self.call(&name, *cpos, args);
                        }
                        Suffix::Field(fname) => {
                            let field_ty = match &cur {
                                Ty::Struct(sname) => {
                                    match self.structs.get(sname) {
                                        Some(fields) => {
                                            match fields.iter().find(|(n, _)| n == &fname.name) {
                                                Some((_, ft)) => ft.clone(),
                                                None => {
                                                    self.err(
                                                        fname.pos,
                                                        format!(
                                                            "'{}' তথ্যে ফিল্ড '{}' নেই",
                                                            sname, fname.name
                                                        ),
                                                    );
                                                    Ty::Err
                                                }
                                            }
                                        }
                                        None => Ty::Err,
                                    }
                                }
                                Ty::Unknown | Ty::Err => Ty::Unknown,
                                other => {
                                    self.err(
                                        fname.pos,
                                        format!(
                                            "'{}' টাইপের উপর ফিল্ড অ্যাক্সেস করা যায় না",
                                            other
                                        ),
                                    );
                                    Ty::Err
                                }
                            };
                            cur = field_ty;
                        }
                        Suffix::Index(ix, ipos) => {
                            callable = None;
                            let it = self.expr(ix).unwrap_or(Ty::Unknown);
                            let expected_idx = match &cur {
                                Ty::Map(k, _) => (**k).clone(),
                                _ => Ty::Num,
                            };
                            if !poisoned(&it) && it != expected_idx && !poisoned(&expected_idx) {
                                self.err(
                                    *ipos,
                                    format!("ইনডেক্স '{}' টাইপের হতে হবে, '{}' পাওয়া গেছে", expected_idx, it),
                                );
                            }
                            cur = match cur {
                                Ty::Arr(t) => *t,
                                Ty::Txt => Ty::Ch,
                                Ty::Map(_, ref v) => (**v).clone(),
                                Ty::Unknown | Ty::Err => Ty::Unknown,
                                other => {
                                    self.err(
                                        *ipos,
                                        format!("শুধু অ্যারে, লেখা বা ম্যাপ ইনডেক্স করা যায়, '{}' নয়", other),
                                    );
                                    Ty::Err
                                }
                            };
                        }
                    }
                }
                cur
            }
        })
    }

    fn binary(&mut self, op: BinOp, lt: Ty, rt: Ty, pos: Pos) -> Ty {
        use BinOp::*;
        if poisoned(&lt) || poisoned(&rt) {
            return Ty::Unknown;
        }
        let arith_num = |ck: &mut Ck, lt: &Ty, rt: &Ty| -> Ty {
            if numeric(lt) && numeric(rt) {
                if *lt == Ty::Num && *rt == Ty::Num {
                    Ty::Num
                } else {
                    Ty::Dec
                }
            } else {
                ck.err(
                    pos,
                    format!(
                        "'{}' অপারেটর ('{}', '{}')-এর জন্য সংজ্ঞায়িত নয়",
                        op_sym(op),
                        lt,
                        rt
                    ),
                );
                Ty::Err
            }
        };
        match op {
            And | Or => {
                if lt == Ty::Bool && rt == Ty::Bool {
                    Ty::Bool
                } else {
                    self.err(pos, "লজিক্যাল অপারেটরের অপারেন্ড 'বুলিয়ান' হতে হবে");
                    Ty::Err
                }
            }
            Eq | Neq => {
                if lt == rt || (numeric(&lt) && numeric(&rt)) {
                    Ty::Bool
                } else {
                    self.err(pos, format!("তুলনা সম্ভব নয় — '{}' বনাম '{}'", lt, rt));
                    Ty::Err
                }
            }
            Lt | Gt | Le | Ge => {
                if numeric(&lt) && numeric(&rt) {
                    Ty::Bool
                } else if lt == Ty::Txt && rt == Ty::Txt {
                    Ty::Bool
                } else {
                    self.err(
                        pos,
                        format!("তুলনায় সংখ্যা বা লেখা প্রত্যাশিত — '{}' বনাম '{}'", lt, rt),
                    );
                    Ty::Err
                }
            }
            Sub | Mul | Div | Mod => arith_num(self, &lt, &rt),
            Add => {
                if numeric(&lt) && numeric(&rt) {
                    if lt == Ty::Num && rt == Ty::Num {
                        Ty::Num
                    } else {
                        Ty::Dec
                    }
                } else if lt == Ty::Txt && rt == Ty::Txt {
                    Ty::Txt
                } else if let (Ty::Arr(a), Ty::Arr(b)) = (&lt, &rt) {
                    if a == b {
                        lt.clone()
                    } else {
                        self.err(
                            pos,
                            format!(
                                "অ্যারে কনক্যাটিনেশনে একই টাইপ লাগে — '{}' বনাম '{}'",
                                lt, rt
                            ),
                        );
                        Ty::Err
                    }
                } else {
                    self.err(
                        pos,
                        format!("'+' অপারেটর ('{}', '{}')-এর জন্য সংজ্ঞায়িত নয়", lt, rt),
                    );
                    Ty::Err
                }
            }
        }
    }

    fn call(&mut self, name: &str, pos: Pos, args: &[Expr]) -> Ty {
        // Struct constructor: Name(args...) where Name is a declared struct
        if self.structs.contains_key(name) {
            let fields: Vec<(String, Ty)> = self.structs[name].clone();
            if fields.len() != args.len() {
                self.err(
                    pos,
                    format!(
                        "'{}' তথ্যে {}টি ফিল্ড আছে, {}টি আর্গুমেন্ট দেওয়া হয়েছে",
                        name,
                        bn_num(fields.len() as u32),
                        bn_num(args.len() as u32)
                    ),
                );
                return Ty::Struct(name.to_string());
            }
            for (i, ((fname, fty), a)) in fields.iter().zip(args.iter()).enumerate() {
                let at = self.expr(a).unwrap_or(Ty::Unknown);
                if !unify(fty, &at) {
                    self.err(
                        a.pos,
                        format!(
                            "'{}'-এর ফিল্ড #{} '{}' টাইপ অমিল — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                            name,
                            bn_num((i + 1) as u32),
                            fname,
                            fty,
                            at
                        ),
                    );
                }
            }
            return Ty::Struct(name.to_string());
        }
        if let Some((module, item)) = name.split_once("::") {
            return self.call_stdlib(module, item, pos, args);
        }
        match name {
            "ম্যাপ_তৈরি" => {
                if !args.is_empty() {
                    self.err(pos, "'ম্যাপ_তৈরি' কোনো আর্গুমেন্ট নেয় না".to_string());
                }
                Ty::Unknown
            }
            "চাবি_গুলো" => {
                if args.len() != 1 {
                    self.err(pos, "'চাবি_গুলো' ১টি আর্গুমেন্ট নেয়".to_string());
                    return Ty::Arr(Box::new(Ty::Txt));
                }
                let t = self.expr(&args[0]).unwrap_or(Ty::Unknown);
                match t {
                    Ty::Map(_, _) | Ty::Unknown | Ty::Err => {}
                    other => self.err(
                        pos,
                        format!("'চাবি_গুলো' ম্যাপ নেয়, '{}' নয়", other),
                    ),
                }
                Ty::Arr(Box::new(Ty::Txt))
            }
            "আছে_কি" => {
                if args.len() != 2 {
                    self.err(pos, "'আছে_কি' ২টি আর্গুমেন্ট নেয়".to_string());
                    return Ty::Bool;
                }
                let m = self.expr(&args[0]).unwrap_or(Ty::Unknown);
                if matches!(m, Ty::Map(_, _)) || poisoned(&m) {
                } else {
                    self.err(args[0].pos, format!("'আছে_কি' ম্যাপ নেয়, '{}' নয়", m));
                }
                let _ = self.expr(&args[1]);
                Ty::Bool
            }
            "চাবি_মুছো" => {
                for a in args {
                    let _ = self.expr(a);
                }
                Ty::Null
            }
            "লেখো" => {
                for a in args {
                    let _ = self.expr(a);
                }
                Ty::Null
            }
            // Reads one line from standard input. A builtin rather than a
            // `ফাইল` member, since it reads from the user, not a file.
            "পড়ো_লাইন" => {
                if !args.is_empty() {
                    self.err(pos, "'পড়ো_লাইন' কোনো আর্গুমেন্ট নেয় না".to_string());
                    for a in args {
                        let _ = self.expr(a);
                    }
                }
                Ty::Txt
            }
            "দৈর্ঘ্য" => {
                if args.len() != 1 {
                    self.err(
                        pos,
                        format!("'দৈর্ঘ্য' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে", bn_num(args.len() as u32)),
                    );
                    return Ty::Num;
                }
                let t = self.expr(&args[0]).unwrap_or(Ty::Unknown);
                match t {
                    Ty::Txt | Ty::Arr(_) | Ty::Map(_, _) | Ty::Unknown | Ty::Err => Ty::Num,
                    other => {
                        self.err(
                            pos,
                            format!("'দৈর্ঘ্য' 'লেখা', অ্যারে বা ম্যাপ নেয়, '{}' নয়", other),
                        );
                        Ty::Num
                    }
                }
            }
            "কপি" => {
                if args.len() != 1 {
                    self.err(
                        pos,
                        format!("'কপি' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে", bn_num(args.len() as u32)),
                    );
                    return Ty::Err;
                }
                self.expr(&args[0]).unwrap_or(Ty::Unknown)
            }
            "শেয়ার_করো" => {
                if args.len() != 1 {
                    self.err(
                        pos,
                        format!("'শেয়ার_করো' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে", bn_num(args.len() as u32)),
                    );
                    return Ty::Err;
                }
                let t = self.expr(&args[0]).unwrap_or(Ty::Unknown);
                self.try_move_src(&args[0], "শেয়ার_করো");
                Ty::Shared(Box::new(t))
            }
            "মান" => {
                if args.len() != 1 {
                    self.err(
                        pos,
                        format!("'মান' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে", bn_num(args.len() as u32)),
                    );
                    return Ty::Err;
                }
                let t = self.expr(&args[0]).unwrap_or(Ty::Unknown);
                match t {
                    Ty::Shared(inner) => *inner,
                    Ty::Unknown | Ty::Err => Ty::Unknown,
                    other => {
                        self.err(pos, format!("'মান' 'শেয়ার' মান নেয়, '{}' নয়", other));
                        Ty::Err
                    }
                }
            }
            "বসাও" => {
                if args.len() != 2 {
                    self.err(
                        pos,
                        format!(
                            "'বসাও' ২টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                            bn_num(args.len() as u32)
                        ),
                    );
                    for a in args {
                        let _ = self.expr(a);
                    }
                    return Ty::Null;
                }
                let cell = self.expr(&args[0]).unwrap_or(Ty::Unknown);
                let inner = match &cell {
                    Ty::Shared(i) => (**i).clone(),
                    Ty::Unknown | Ty::Err => Ty::Unknown,
                    other => {
                        self.err(args[0].pos, format!("'বসাও'-এর প্রথম আর্গুমেন্ট 'শেয়ার' হতে হবে, '{}' নয়", other));
                        Ty::Err
                    }
                };
                let v = self.expr(&args[1]).unwrap_or(Ty::Unknown);
                if !unify(&inner, &v) {
                    self.err(
                        args[1].pos,
                        format!("'বসাও' টাইপ অমিল — সেল '{}', মান '{}'", inner, v),
                    );
                }
                Ty::Null
            }
            "লেখায়" => {
                if args.len() != 1 {
                    self.err(
                        pos,
                        format!("'লেখায়' ১টি আর্গুমেন্ট নেয়, {}টি পেয়েছে", bn_num(args.len() as u32)),
                    );
                    return Ty::Txt;
                }
                let t = self.expr(&args[0]).unwrap_or(Ty::Unknown);
                if matches!(t, Ty::Shared(_)) {
                    self.err(pos, "'লেখায়'-এর আগে 'মান(x)' দিয়ে ভেতরের মান নিন".to_string());
                }
                Ty::Txt
            }
            _ => {
                let sig = match self.funcs.get(name) {
                    Some(s) => s.clone(),
                    None => {
                        self.err(pos, format!("অজানা ফাংশন '{}'", name));
                        return Ty::Err;
                    }
                };
                if sig.params.len() != args.len() {
                    self.err(
                        pos,
                        format!(
                            "'{}' {}টি প্যারামিটার নেয়, {}টি পেয়েছে",
                            name,
                            bn_num(sig.params.len() as u32),
                            bn_num(args.len() as u32)
                        ),
                    );
                    for a in args {
                        let _ = self.expr(a);
                    }
                    return sig.ret;
                }
                for (i, (pt, a)) in sig.params.iter().zip(args.iter()).enumerate() {
                    let at = self.expr(a).unwrap_or(Ty::Unknown);
                    if !unify(pt, &at) {
                        self.err(
                            a.pos,
                            format!(
                                "'{}'-এর আর্গুমেন্ট #{} টাইপ অমিল — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                                name,
                                bn_num((i + 1) as u32),
                                pt,
                                at
                            ),
                        );
                    }
                }
                sig.ret
            }
        }
    }

    /// `পরম_মান`, `সর্বনিম্ন` and `সর্বোচ্চ` accept either `সংখ্যা` or
    /// `দশমিক` and give back the same type. Returns `None` for anything else,
    /// leaving it to the ordinary signature table.
    ///
    /// Kolom has no general overloading, so this is a deliberate special case.
    /// The alternative is a separate name per type, which is where `পরমদ` and
    /// `ছোটদশমিক` came from — names that describe an implementation detail
    /// rather than what the function does.
    ///
    /// Mixed arguments are rejected rather than promoted: nothing else in the
    /// language converts `সংখ্যা` to `দশমিক` on its own, and doing it here
    /// alone would be a surprise.
    fn check_math_overload(&mut self, item: &str, pos: Pos, args: &[Expr]) -> Option<Ty> {
        let arity = match item {
            "পরম_মান" => 1,
            "সর্বনিম্ন" | "সর্বোচ্চ" => 2,
            _ => return None,
        };
        if args.len() != arity {
            self.err(
                pos,
                format!(
                    "'গণিত.{}' {}টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                    item,
                    bn_num(arity as u32),
                    bn_num(args.len() as u32)
                ),
            );
            for a in args {
                let _ = self.expr(a);
            }
            return Some(Ty::Err);
        }
        let tys: Vec<Ty> = args.iter().map(|a| self.expr(a).unwrap_or(Ty::Unknown)).collect();
        if tys.iter().any(|t| matches!(t, Ty::Err | Ty::Unknown)) {
            return Some(Ty::Err);
        }
        if tys.iter().all(|t| matches!(t, Ty::Num)) {
            return Some(Ty::Num);
        }
        if tys.iter().all(|t| matches!(t, Ty::Dec)) {
            return Some(Ty::Dec);
        }
        let shown: Vec<String> = tys.iter().map(|t| t.to_string()).collect();
        self.err(
            pos,
            format!(
                "'গণিত.{}'-এর সব আর্গুমেন্ট একই টাইপের হতে হবে — সবগুলো 'সংখ্যা' অথবা সবগুলো 'দশমিক' ({} পাওয়া গেছে)",
                item,
                shown.join(", ")
            ),
        );
        Some(Ty::Err)
    }

    fn call_stdlib(&mut self, module: &str, item: &str, pos: Pos, args: &[Expr]) -> Ty {
        if !self.imports.contains(module) {
            let msg = self.unknown_module_msg(module);
            self.err(pos, msg);
            return Ty::Err;
        }
        if module == "গ্রাফিক্স" && item == "টিক" {
            return self.check_tick(pos, args);
        }
        if module == "গণিত" {
            if let Some(t) = self.check_math_overload(item, pos, args) {
                return t;
            }
        }
        let sig = match stdlib_lookup(module, item) {
            Some(StdSig::Fn(p, r)) => (p, r),
            Some(StdSig::Const(_)) => {
                self.err(
                    pos,
                    format!(
                        "'{}.{}' একটি কনস্ট্যান্ট — বন্ধনী ছাড়াই ব্যবহার করুন",
                        module, item
                    ),
                );
                return Ty::Err;
            }
            None => {
                self.err(pos, format!("মডিউল '{}'-এ '{}' নেই", module, item));
                return Ty::Err;
            }
        };
        if sig.0.len() != args.len() {
            self.err(
                pos,
                format!(
                    "'{}.{}' {}টি আর্গুমেন্ট নেয়, {}টি পেয়েছে",
                    module,
                    item,
                    bn_num(sig.0.len() as u32),
                    bn_num(args.len() as u32)
                ),
            );
            for a in args {
                let _ = self.expr(a);
            }
            return sig.1;
        }
        for (i, (pt, a)) in sig.0.iter().zip(args.iter()).enumerate() {
            let at = self.expr(a).unwrap_or(Ty::Unknown);
            if !unify(pt, &at) {
                self.err(
                    a.pos,
                    format!(
                        "'{}.{}'-এর আর্গুমেন্ট #{} টাইপ অমিল — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                        module,
                        item,
                        bn_num((i + 1) as u32),
                        pt,
                        at
                    ),
                );
            }
        }
        sig.1
    }

    fn check_assign(&mut self, target: &LValue, rhs: &Expr, rt: Ty) {
        // Struct field assignment: p.field = value
        if let Some(field) = &target.field {
            let bt = match self.lookup(&target.base.name) {
                Some(b) => b.ty.clone(),
                None => {
                    self.err(target.base.pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", target.base.name));
                    return;
                }
            };
            if let Ty::Struct(sname) = &bt {
                if let Some(fields) = self.structs.get(sname) {
                    match fields.iter().find(|(n, _)| n == &field.name) {
                        Some((_, fty)) => {
                            if !unify(fty, &rt) {
                                self.err(
                                    rhs.pos,
                                    format!(
                                        "ফিল্ড '{}' টাইপ অমিল — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                                        field.name, fty, rt
                                    ),
                                );
                            }
                        }
                        None => {
                            self.err(field.pos, format!("'{}' তথ্যে ফিল্ড '{}' নেই", sname, field.name));
                        }
                    }
                }
            } else {
                self.err(target.base.pos, format!("'{}' তথ্য নয়", target.base.name));
            }
            return;
        }
        // Regular variable or array index assignment
        if target.idx.is_empty() {
            let name = target.base.name.clone();
            let existing = match self.lookup(&name) {
                Some(b) => b.clone(),
                None => {
                    self.err(target.base.pos, format!("অঘোষিত ভ্যারিয়েবল '{}'", name));
                    return;
                }
            };
            if existing.is_const {
                self.err(
                    target.base.pos,
                    format!("ধ্রুবক '{}'-এ মান নির্ধারণ করা যাবে না", name),
                );
                return;
            }
            if !unify(&existing.ty, &rt) {
                self.err(
                    rhs.pos,
                    format!(
                        "'{}' একটি {}। এখানে {} নির্ধারণ করা যাবে না।",
                        name, existing.ty, rt
                    ),
                );
                return;
            }
            self.try_move_src(rhs, &name);
            if let Some(b) = self.lookup_mut(&name) {
                b.moved = false;
                b.moved_by.clear();
            }
        } else {
            let base_ty = {
                let info = match self.lookup(&target.base.name) {
                    Some(b) => Some((b.ty.clone(), b.moved, b.moved_by.clone())),
                    None => None,
                };
                match info {
                    None => {
                        self.err(
                            target.base.pos,
                            format!("অঘোষিত ভ্যারিয়েবল '{}'", target.base.name),
                        );
                        return;
                    }
                    Some((ty, moved, moved_by)) => {
                        if moved {
                            self.err(
                                target.base.pos,
                                format!(
                                    "'{}' ইতিমধ্যে '{}'-তে মুভ হয়ে গেছে। মুভ হওয়ার পর '{}' ব্যবহার করা যাবে না",
                                    target.base.name, moved_by, target.base.name
                                ),
                            );
                        }
                        ty
                    }
                }
            };
            let mut elem = base_ty;
            for ie in &target.idx {
                let it = self.expr(ie).unwrap_or(Ty::Unknown);
                let expected_idx = match &elem {
                    Ty::Map(k, _) => (**k).clone(),
                    _ => Ty::Num,
                };
                if !poisoned(&it) && it != expected_idx && !poisoned(&expected_idx) {
                    self.err(
                        ie.pos,
                        format!("ইনডেক্স '{}' টাইপের হতে হবে, '{}' পাওয়া গেছে", expected_idx, it),
                    );
                }
                elem = match elem {
                    Ty::Arr(t) => *t,
                    Ty::Map(_, v) => *v,
                    Ty::Unknown | Ty::Err => Ty::Unknown,
                    other => {
                        self.err(
                            target.base.pos,
                            format!("শুধু অ্যারে, লেখা বা ম্যাপ ইনডেক্স করা যায়, '{}' নয়", other),
                        );
                        Ty::Err
                    }
                };
            }
            if !unify(&elem, &rt) {
                self.err(
                    rhs.pos,
                    format!(
                        "অ্যারে এলিমেন্ট টাইপ অমিল — '{}' প্রত্যাশিত, '{}' পাওয়া গেছে",
                        elem, rt
                    ),
                );
            }
        }
    }
}

fn numeric(t: &Ty) -> bool {
    matches!(t, Ty::Num | Ty::Dec)
}

fn op_sym(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "এবং",
        BinOp::Or => "অথবা",
    }
}
