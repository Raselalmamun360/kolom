use std::collections::HashMap;

use kolom_sema::Types;
use kolom_syntax::ast::*;

type STy = kolom_sema::Ty;

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
}

fn conv(t: &STy) -> Ty {
    match t {
        STy::Num => Ty::Num,
        STy::Dec => Ty::Dec,
        STy::Txt => Ty::Txt,
        STy::Bool => Ty::Bool,
        STy::Ch => Ty::Ch,
        STy::Null | STy::Unknown | STy::Err => Ty::Null,
        STy::Arr(i) => Ty::Arr(Box::new(conv(i))),
        STy::Shared(i) => Ty::Shared(Box::new(conv(i))),
        STy::Map(k, v) => Ty::Map(Box::new(conv(k)), Box::new(conv(v))),
        STy::Struct(name) => Ty::Map(Box::new(Ty::Txt), Box::new(Ty::Null)),
    }
}

impl Ty {
    fn mg(&self) -> String {
        match self {
            Ty::Num => "i".into(),
            Ty::Dec => "d".into(),
            Ty::Txt => "s".into(),
            Ty::Bool => "b".into(),
            Ty::Ch => "c".into(),
            Ty::Null => "n".into(),
            Ty::Arr(i) => format!("a{}", i.mg()),
            Ty::Shared(i) => format!("h{}", i.mg()),
            Ty::Map(k, v) => format!("m{}_{}", k.mg(), v.mg()),
            Ty::Struct(n) => format!("st_{}", n),
        }
    }

    fn c(&self) -> String {
        match self {
            Ty::Num => "int64_t".into(),
            Ty::Dec => "double".into(),
            Ty::Txt => "kl_str".into(),
            Ty::Bool => "bool".into(),
            Ty::Ch => "uint32_t".into(),
            Ty::Null => "uint8_t".into(),
            Ty::Arr(_) => format!("kl_arr_{}", self.mg()),
            Ty::Shared(_) => format!("kl_sh_{}*", self.mg()),
            Ty::Map(_, _) | Ty::Struct(_) => format!("kl_map_{}*", self.mg()),
        }
    }

    fn tracked(&self) -> bool {
        matches!(self, Ty::Txt | Ty::Arr(_) | Ty::Shared(_) | Ty::Map(_, _) | Ty::Struct(_))
    }

    fn zero(&self) -> String {
        match self {
            Ty::Num | Ty::Null | Ty::Ch => "0".into(),
            Ty::Dec => "0.0".into(),
            Ty::Bool => "false".into(),
            Ty::Txt => "kl_str_zero()".into(),
            Ty::Arr(_) => format!("kl_arr_{}_zero()", self.mg()),
            Ty::Shared(_) => "NULL".into(),
            Ty::Map(_, _) | Ty::Struct(_) => "NULL".into(),
        }
    }

    fn up_call(&self, v: &str) -> String {
        match self {
            Ty::Txt => format!("kl_str_incref({})", v),
            Ty::Arr(_) => format!("kl_arr_{}_incref({})", self.mg(), v),
            Ty::Shared(_) => format!("kl_sh_{}_incref({})", self.mg(), v),
            Ty::Map(_, _) | Ty::Struct(_) => format!("kl_map_{}_incref({})", self.mg(), v),
            _ => String::new(),
        }
    }

    fn down_call(&self, addr: &str) -> String {
        match self {
            Ty::Txt => format!("kl_str_decref(&({}))", addr),
            Ty::Arr(_) => format!("kl_arr_{}_decref(&({}))", self.mg(), addr),
            Ty::Shared(_) => format!("kl_sh_{}_decref({})", self.mg(), addr),
            Ty::Map(_, _) | Ty::Struct(_) => format!("kl_map_{}_decref(&({}))", self.mg(), addr),
            _ => String::new(),
        }
    }

    fn copy_expr(&self, v: &str) -> String {
        match self {
            Ty::Txt => format!("kl_str_copy({})", v),
            Ty::Arr(_) => format!("kl_deep_{}({})", self.mg(), v),
            Ty::Shared(_) => {
                let u = self.up_call(v);
                format!("({}, {})", u, v)
            }
            _ => v.to_string(),
        }
    }
}

#[derive(Clone)]
struct Bnd {
    c: String,
    ty: Ty,
}

#[derive(Clone)]
struct FnMeta {
    cname: String,
    params: Vec<(String, Ty)>,
    ret: Ty,
}

struct Gen {
    out: Vec<String>,
    impl_ph: usize,
    indent: usize,
    tmp: usize,
    var_n: usize,
    fn_n: usize,
    types: Types,
    funcs: HashMap<String, FnMeta>,
    impls: Vec<String>,
    seen_p: Vec<String>,
    seen_d: Vec<String>,
    seen_ts: Vec<String>,
    title: String,
    has_ui: bool,
    has_display: bool,
    globals: HashMap<String, Bnd>,
    pending_globals: Vec<(String, Ty)>,
    std_modules: Vec<&'static str>,
    ui_block: Option<String>,
    gfx_block: Option<String>,
    target: String,
}

    pub fn emit(prog: &Program, title: &str, target: &str) -> String {
        let (_, types) = kolom_sema::analyze_typed(prog);
        let mut g = Gen {
            out: Vec::new(),
            impl_ph: 0,
            indent: 0,
            tmp: 0,
            var_n: 0,
            fn_n: 0,
            types,
            funcs: HashMap::new(),
            impls: Vec::new(),
            seen_p: Vec::new(),
            seen_d: Vec::new(),
            seen_ts: Vec::new(),
            title: title.to_string(),
            has_ui: false,
            has_display: false,
            globals: HashMap::new(),
            pending_globals: Vec::new(),
            std_modules: Vec::new(),
            ui_block: None,
            gfx_block: None,
            target: target.to_string(),
        };
    g.run(prog);
    let ph = "@@KL_IMPLS@@";
    if let Some(pos) = g.out.iter().position(|l| l == ph) {
        let mut block = String::new();
        if let Some(gfx) = &g.gfx_block {
            block.push_str(gfx);
            block.push('\n');
        }
        if let Some(ui) = &g.ui_block {
            block.push_str(ui);
            block.push('\n');
        }
        for other in &g.impls {
            block.push_str(other);
            block.push('\n');
        }
        g.out[pos] = block;
    }
    g.out.join("\n")
}

fn esc_bytes(s: &str) -> String {
    let mut o = String::new();
    for b in s.as_bytes() {
        match *b {
            0x20..=0x7E if !matches!(*b, b'"' | b'\\' | b'?') => o.push(*b as char),
            _ => o.push_str(&format!("\\{:03o}", b)),
        }
    }
    o
}

fn cstr(s: &str) -> String {
    format!("\"{}\"", esc_bytes(s))
}

impl Gen {
    fn w(&mut self, line: impl Into<String>) {
        self.out.push(format!("{}{}", "    ".repeat(self.indent), line.into()));
    }

    fn tmp(&mut self) -> String {
        self.tmp += 1;
        format!("_t{}", self.tmp)
    }

    fn fresh_var(&mut self) -> String {
        self.var_n += 1;
        format!("kl_v{}", self.var_n)
    }

    fn decl_ty(&self, id: &Ident) -> Ty {
        conv(&self.types.decl_of(id))
    }

    fn ety(&self, e: &Expr) -> Ty {
        conv(&self.types.expr_of(e))
    }

    fn ensure(&mut self, t: &Ty) {
        match t {
            Ty::Arr(inner) => {
                self.ensure(inner);
                let m = t.mg();
                if !self.has_impl(&m) {
                    let code = self.arr_impl(inner);
                    self.mark_impl(&m, code);
                }
            }
            Ty::Shared(inner) => {
                self.ensure(inner);
                let m = t.mg();
                if !self.has_impl(&m) {
                    let code = self.sh_impl(inner);
                    self.mark_impl(&m, code);
                }
            }
            Ty::Map(kt, vt) => {
                self.ensure(kt);
                self.ensure(vt);
                let m = t.mg();
                if !self.has_impl(&m) {
                    let code = self.map_impl(kt, vt);
                    self.mark_impl(&m, code);
                }
            }
            _ => {}
        }
    }

    fn has_impl(&self, m: &str) -> bool {
        let marker = format!("/* kl_marker_{} */", m);
        self.impls.iter().any(|code| code.starts_with(&marker))
    }

    fn mark_impl(&mut self, m: &str, mut code: String) {
        code = format!("/* kl_marker_{} */\n{}", m, code);
        self.impls.push(code);
    }

    fn ensure_print(&mut self, t: &Ty) {
        if let Ty::Arr(inner) | Ty::Shared(inner) = t {
            self.ensure_print(inner);
        }
        let m = t.mg();
        if !self.seen_p.contains(&m) {
            self.seen_p.push(m.clone());
            let code = self.print_impl(t);
            self.impls.push(code);
        }
    }

    fn ensure_deep(&mut self, t: &Ty) {
        if let Ty::Arr(inner) | Ty::Shared(inner) = t {
            self.ensure_deep(inner);
        }
        if !t.tracked() {
            return;
        }
        let m = t.mg();
        if !self.seen_d.contains(&m) {
            self.seen_d.push(m.clone());
            let code = self.deep_impl(t);
            self.impls.push(code);
        }
    }

    fn ensure_tostr(&mut self, t: &Ty) {
        match t {
            Ty::Arr(inner) => self.ensure_tostr(inner),
            _ => {}
        }
        let m = t.mg();
        if !self.seen_ts.contains(&m) {
            self.seen_ts.push(m);
            let code = self.tostr_impl(t);
            self.impls.push(code);
        }
    }

    fn tostr_impl(&mut self, t: &Ty) -> String {
        let m = t.mg();
        match t {
            Ty::Num => format!(
                "static kl_str kl_tostr_{m}(int64_t v) {{ char b[32]; snprintf(b, sizeof(b), \"%lld\", (long long)v); return kl_str_lit(b); }}"
            ),
            Ty::Dec => format!(
                "static kl_str kl_tostr_{m}(double v) {{ char b[64]; snprintf(b, sizeof(b), \"%g\", v); return kl_str_lit(b); }}"
            ),
            Ty::Bool => format!(
                "static kl_str kl_tostr_{m}(bool v) {{ return kl_str_lit(v ? {t} : {f}); }}",
                m = m,
                t = cstr("সত্য"),
                f = cstr("মিথ্যা")
            ),
            Ty::Ch => format!(
                "static kl_str kl_tostr_{m}(uint32_t cp) {{ uint8_t b[8]; int n = 0; if (cp < 0x80) b[n++] = (uint8_t)cp; else if (cp < 0x800) {{ b[n++] = 0xC0 | (cp >> 6); b[n++] = 0x80 | (cp & 0x3F); }} else if (cp < 0x10000) {{ b[n++] = 0xE0 | (cp >> 12); b[n++] = 0x80 | ((cp >> 6) & 0x3F); b[n++] = 0x80 | (cp & 0x3F); }} else {{ b[n++] = 0xF0 | (cp >> 18); b[n++] = 0x80 | ((cp >> 12) & 0x3F); b[n++] = 0x80 | ((cp >> 6) & 0x3F); b[n++] = 0x80 | (cp & 0x3F); }} b[n] = 0; (void)n; return kl_str_lit((const char*)b); }}",
            ),
            Ty::Txt => format!(
                "static kl_str kl_tostr_{m}(kl_str v) {{ return kl_str_copy(v); }}"
            ),
            Ty::Null => format!(
                "static kl_str kl_tostr_{m}(uint8_t v) {{ (void)v; return kl_str_lit({n}); }}",
                n = cstr("ফাঁকা")
            ),
            Ty::Arr(inner) => {
                let em = inner.mg();
                let elem_expr = format!("kl_tostr_{}(v.data[i])", em);
                format!(
                    r#"
static kl_str kl_tostr_{m}(kl_arr_{m} v) {{
    kl_str r = kl_str_lit("[");
    for (int64_t i = 0; i < v.len; i++) {{
        if (i) r = kl_str_concat(r, kl_str_lit(", "));
        r = kl_str_concat(r, {elem_expr});
    }}
    r = kl_str_concat(r, kl_str_lit("]"));
    return r;
}}"#
                )
            }
            Ty::Shared(_) => String::new(),
            Ty::Map(_, _) => String::new(),
            Ty::Struct(_) => String::new(),
        }
    }

    fn runtime(&self) -> String {
        let panic_msg = "রানটাইম ত্রুটি\n\n%s\n";
        let ovf = "পূর্ণসংখ্যা ওভারফ্লো";
        let divz = "শূন্য দিয়ে ভাগ করা যাবে না";
        let modz = "শূন্য দিয়ে ভাগ (মডুলো) করা যাবে না";
        let oob_fmt = "ইনডেক্স %s সীমার বাইরে (দৈর্ঘ্য %s)";
        format!(
            r#"
static void kl_panic(const char* msg) {{
    fprintf(stderr, {panic});
    fprintf(stderr, "%s\n", msg);
    exit(1);
}}

#include <setjmp.h>
static jmp_buf* kl_try_stack[64];
static int kl_try_sp = 0;
static char kl_last_err[512] = "";

static void kl_throw(const char* msg) {{
    snprintf(kl_last_err, sizeof(kl_last_err), "%s", msg);
    if (kl_try_sp > 0) {{
        longjmp(*kl_try_stack[kl_try_sp - 1], 1);
    }}
    fprintf(stderr, {panic});
    fprintf(stderr, "%s\n", msg);
    exit(1);
}}

static void kl_bn(int64_t v, char* out) {{
    char tmp[32];
    int i = 0, j = 0;
    unsigned long long u;
    if (v < 0) {{ out[j++] = (char)0xE2; out[j++] = (char)0x88; out[j++] = (char)0x92; u = (unsigned long long)(-(v + 1)) + 1ULL; }}
    else u = (unsigned long long)v;
    if (u == 0) tmp[i++] = '0';
    while (u > 0) {{ tmp[i++] = (char)('0' + (int)(u % 10)); u /= 10; }}
    while (i > 0) {{
        int d = tmp[--i] - '0';
        out[j++] = (char)(0xE0);
        out[j++] = (char)(0xA6);
        out[j++] = (char)(0x98 + d);
    }}
    out[j] = 0;
}}

static int64_t kl_iadd(int64_t a, int64_t b) {{
    if (b > 0 && a > INT64_MAX - b) kl_panic({ovf});
    if (b < 0 && a < INT64_MIN - b) kl_panic({ovf});
    return a + b;
}}

static int64_t kl_isub(int64_t a, int64_t b) {{
    if (b < 0 && a > INT64_MAX + b) kl_panic({ovf});
    if (b > 0 && a < INT64_MIN + b) kl_panic({ovf});
    return a - b;
}}

static int64_t kl_imul(int64_t a, int64_t b) {{
    if (a != 0 && b != 0) {{
        if (a == -1 && b == INT64_MIN) kl_panic({ovf});
        if (b == -1 && a == INT64_MIN) kl_panic({ovf});
        int64_t r = a * b;
        if (r / a != b) kl_panic({ovf});
        return r;
    }}
    return 0;
}}

static int64_t kl_idiv(int64_t a, int64_t b) {{
    if (b == 0) kl_panic({divz});
    if (a == INT64_MIN && b == -1) kl_panic({ovf});
    return a / b;
}}

static int64_t kl_imod(int64_t a, int64_t b) {{
    if (b == 0) kl_panic({modz});
    if (a == INT64_MIN && b == -1) kl_panic({ovf});
    return a % b;
}}

static bool kl_dz(double x) {{ return x == 0.0; }}

static double kl_ddiv(double a, double b) {{
    if (b == 0.0) kl_panic({divz});
    return a / b;
}}

static double kl_dmod(double a, double b) {{
    if (b == 0.0) kl_panic({modz});
    return fmod(a, b);
}}

static void kl_oob(int64_t i, int64_t n) {{
    char b1[40], b2[40], buf[160];
    kl_bn(i, b1);
    kl_bn(n, b2);
    snprintf(buf, sizeof(buf), {oob}, b1, b2);
    kl_panic(buf);
}}

static void kl_emit_utf8(uint32_t cp) {{
    if (cp < 0x80) fputc((int)cp, stdout);
    else if (cp < 0x800) {{ fputc(0xC0 | (int)(cp >> 6), stdout); fputc(0x80 | (int)(cp & 0x3F), stdout); }}
    else if (cp < 0x10000) {{ fputc(0xE0 | (int)(cp >> 12), stdout); fputc(0x80 | (int)((cp >> 6) & 0x3F), stdout); fputc(0x80 | (int)(cp & 0x3F), stdout); }}
    else {{ fputc(0xF0 | (int)(cp >> 18), stdout); fputc(0x80 | (int)((cp >> 12) & 0x3F), stdout); fputc(0x80 | (int)((cp >> 6) & 0x3F), stdout); fputc(0x80 | (int)(cp & 0x3F), stdout); }}
}}

typedef struct {{ int64_t rc; int64_t len; uint8_t* data; }} kl_str;

static kl_str kl_str_zero(void) {{ kl_str s; s.rc = 0; s.len = 0; s.data = NULL; return s; }}

static int64_t kl_cpcount(const uint8_t* p, int64_t n) {{
    int64_t c = 0;
    for (int64_t i = 0; i < n; i++) if ((p[i] & 0xC0) != 0x80) c++;
    return c;
}}

static kl_str kl_str_alloc(int64_t nbytes) {{
    kl_str s; s.rc = 1; s.data = (uint8_t*)calloc((size_t)nbytes + 1, 1);
    if (!s.data) kl_panic("মেমরি বরাদ্দ ব্যর্থ");
    s.len = 0; return s;
}}

static kl_str kl_str_lit(const char* lit) {{
    int64_t n = (int64_t)strlen(lit);
    kl_str s = kl_str_alloc(n);
    memcpy(s.data, lit, (size_t)n);
    s.len = kl_cpcount(s.data, n);
    return s;
}}

static void kl_str_incref(kl_str s) {{ if (s.data) s.rc++; }}

static void kl_str_decref(kl_str* s) {{
    if (!s->data || --s->rc > 0) return;
    free(s->data); s->data = NULL;
}}

static kl_str kl_str_concat(kl_str a, kl_str b) {{
    int64_t na = a.data ? (int64_t)strlen((const char*)a.data) : 0;
    int64_t nb = b.data ? (int64_t)strlen((const char*)b.data) : 0;
    kl_str s = kl_str_alloc(na + nb);
    if (na) memcpy(s.data, a.data, (size_t)na);
    if (nb) memcpy(s.data + na, b.data, (size_t)nb);
    s.len = a.len + b.len;
    return s;
}}

static kl_str kl_str_copy(kl_str a) {{
    int64_t n = a.data ? (int64_t)strlen((const char*)a.data) : 0;
    kl_str s = kl_str_alloc(n);
    if (n) memcpy(s.data, a.data, (size_t)n);
    s.len = a.len;
    return s;
}}

static bool kl_str_eq(kl_str a, kl_str b) {{
    if (!a.data || !b.data) return a.data == b.data;
    return strcmp((const char*)a.data, (const char*)b.data) == 0;
}}
"#,
            panic = cstr(panic_msg),
            ovf = cstr(ovf),
            divz = cstr(divz),
            modz = cstr(modz),
            oob = cstr(oob_fmt),
        )
    }

    fn arr_impl(&mut self, elem: &Ty) -> String {
        self.ensure_print(elem);
        if elem.tracked() {
            self.ensure_deep(elem);
        }
        let ct = elem.c();
        let m = format!("a{}", elem.mg());
        let down_decref = if elem.tracked() {
            format!("{};", elem.down_call("arr->data[i]"))
        } else {
            "(void)0;".into()
        };
        let get_ret = if elem.tracked() {
            format!("return {};", elem.copy_expr("a.data[i]"))
        } else {
            "return a.data[i];".into()
        };
        let set_extra = if elem.tracked() {
            format!(
                "\n    {};\n    {};",
                elem.up_call("v"),
                elem.down_call("a.data[i]")
            )
        } else {
            String::new()
        };
        let cp_x = if elem.tracked() {
            format!("r.data[i] = {};", elem.copy_expr("x.data[i]"))
        } else {
            String::new()
        };
        let cp_y = if elem.tracked() {
            format!("r.data[x.len + i] = {};", elem.copy_expr("y.data[i]"))
        } else {
            String::new()
        };
        let elem_print = format!("kl_print_{}", elem.mg());
        format!(
            r#"
typedef struct {{ int64_t rc; int64_t len; {ct}* data; }} kl_arr_{m};

static kl_arr_{m} kl_arr_{m}_zero(void) {{ kl_arr_{m} a; a.rc = 0; a.len = 0; a.data = NULL; return a; }}

static void kl_arr_{m}_incref(kl_arr_{m} a) {{ if (a.data) a.rc++; }}

static void kl_arr_{m}_decref(kl_arr_{m}* arr) {{
    if (!arr->data || --arr->rc > 0) return;
    for (int64_t i = 0; i < arr->len; i++) {{
        (void)i;
        {down_decref}
    }}
    free(arr->data); arr->data = NULL;
}}

static kl_arr_{m} kl_arr_{m}_new(int64_t n) {{
    kl_arr_{m} a; a.rc = 1; a.len = n;
    a.data = (n > 0) ? ({ct}*)calloc((size_t)n, sizeof({ct})) : NULL;
    if (n > 0 && !a.data) kl_panic("মেমরি বরাদ্দ ব্যর্থ");
    return a;
}}

static {ct}* kl_arr_{m}_at(kl_arr_{m} a, int64_t i) {{
    if (i < 0 || i >= a.len || !a.data) kl_oob(i, a.len);
    return &a.data[i];
}}

static {ct} kl_arr_{m}_get(kl_arr_{m} a, int64_t i) {{
    {ct}* p = kl_arr_{m}_at(a, i);
    (void)p;
    {get_ret}
}}

static void kl_arr_{m}_set(kl_arr_{m} a, int64_t i, {ct} v) {{
    {ct}* p = kl_arr_{m}_at(a, i);
    (void)p;{set_extra}
    *p = v;
}}

static kl_arr_{m} kl_arr_{m}_concat(kl_arr_{m} x, kl_arr_{m} y) {{
    kl_arr_{m} r = kl_arr_{m}_new(x.len + y.len);
    for (int64_t i = 0; i < x.len; i++) {{
        {ct} xv = x.data[i];
        (void)xv;
        {cp_x}
        r.data[i] = x.data[i];
    }}
    for (int64_t i = 0; i < y.len; i++) {{
        {ct} yv = y.data[i];
        (void)yv;
        {cp_y}
        r.data[x.len + i] = y.data[i];
    }}
    return r;
}}

static void kl_print_{m}(kl_arr_{m} v) {{
    fputc('[', stdout);
    for (int64_t i = 0; i < v.len; i++) {{
        if (i) fputs(", ", stdout);
        {elem_print}(v.data[i]);
    }}
    fputc(']', stdout);
}}
"#,
            ct = ct,
            m = m,
            down_decref = down_decref,
            get_ret = get_ret,
            set_extra = set_extra,
            cp_x = cp_x,
            cp_y = cp_y,
            elem_print = elem_print,
        )
    }

    fn map_impl(&mut self, kt: &Ty, vt: &Ty) -> String {
        let km = kt.mg();
        let vm = vt.mg();
        let m = format!("m{}_{}", km, vm);
        let kct = kt.c();
        let vct = vt.c();

        // For MVP: string-keyed map with linear search, fixed capacity
        format!(
            r#"
typedef struct {{ kl_str key; {vct} val; int used; }} kl_map_ent_{m};
typedef struct {{ int64_t rc; kl_map_ent_{m} entries[128]; int64_t len; }} kl_map_{m};

static kl_map_{m}* kl_map_{m}_new(void) {{
    kl_map_{m}* p = (kl_map_{m}*)calloc(1, sizeof(kl_map_{m}));
    if (!p) kl_panic("মেমরি বরাদ্দ ব্যর্থ");
    p->rc = 1;
    return p;
}}

static void kl_map_{m}_incref(kl_map_{m}* p) {{ if (p) p->rc++; }}

static void kl_map_{m}_decref(kl_map_{m}* p) {{
    if (!p || --p->rc > 0) return;
    for (int64_t i = 0; i < p->len; i++) {{
        kl_str_decref(&p->entries[i].key);
    }}
    free(p);
}}

static int64_t kl_map_{m}_find(kl_map_{m}* p, kl_str key) {{
    if (!key.data) return -1;
    for (int64_t i = 0; i < p->len; i++) {{
        if (p->entries[i].used && strcmp((const char*)p->entries[i].key.data, (const char*)key.data) == 0)
            return i;
    }}
    return -1;
}}

static {vct} kl_map_{m}_get(kl_map_{m}* p, kl_str key) {{
    int64_t idx = kl_map_{m}_find(p, key);
    if (idx < 0) {{
        char b[512];
        snprintf(b, sizeof(b), "কী '%s' ম্যাপে নেই", key.data ? (const char*)key.data : "");
        kl_panic(b);
    }}
    return p->entries[idx].val;
}}

static void kl_map_{m}_set(kl_map_{m}* p, kl_str key, {vct} val) {{
    int64_t idx = kl_map_{m}_find(p, key);
    if (idx >= 0) {{
        {down_old}
        p->entries[idx].val = val;
        {up_new}
        return;
    }}
    if (p->len >= 128) kl_panic("????? ?????");
    p->entries[p->len].key = kl_str_copy(key);
    p->entries[p->len].val = val;
    {up_new}
    p->entries[p->len].used = 1;
    p->len++;
}}

static kl_map_ent_{m}* kl_map_{m}_get_or_null(kl_map_{m}* p, kl_str key) {{
    int64_t idx = kl_map_{m}_find(p, key);
    if (idx < 0) return &p->entries[p->len];
    return &p->entries[idx];
}}
"#,
            vct = vct,
            m = m,
            down_old = if vt.tracked() {
                format!("{};", vt.down_call("p->entries[idx].val"))
            } else {
                String::new()
            },
            up_new = if vt.tracked() {
                format!("{};", vt.up_call("val"))
            } else {
                String::new()
            },
        )
    }

    fn sh_impl(&mut self, inner: &Ty) -> String {        self.ensure_print(inner);
        let ct = inner.c();
        let im = inner.mg();
        let m = format!("h{}", im);
        let down = if inner.tracked() {
            format!("\n    {};", inner.down_call("&pp->v"))
        } else {
            String::new()
        };
        let ez = inner.zero();
        format!(
            r#"
typedef struct {{ int64_t rc; {ct} v; }} kl_sh_{m};

static kl_sh_{m}* kl_sh_make_{m}({ct} v) {{
    kl_sh_{m}* p = (kl_sh_{m}*)malloc(sizeof(kl_sh_{m}));
    if (!p) kl_panic("মেমরি বরাদ্দ ব্যর্থ");
    p->rc = 1; p->v = v; return p;
}}

static void kl_sh_{m}_incref(kl_sh_{m}* p) {{ if (p) p->rc++; }}

static void kl_sh_{m}_decref(kl_sh_{m}* pp) {{
    if (!pp || --pp->rc > 0) return;{down}
    free(pp);
}}

static void kl_print_{m}(kl_sh_{m}* p) {{
    if (!p) {{ fputs("{ez}", stdout); return; }}
    {eprint}(p->v);
}}
"#,
            ct = ct,
            m = m,
            down = down,
            ez = esc_bytes(&ez),
            eprint = format!("kl_print_{}", im),
        )
    }

    fn print_impl(&mut self, t: &Ty) -> String {
        let m = t.mg();
        match t {
            Ty::Num => format!(
                "static void kl_print_{m}(int64_t v) {{ printf(\"%lld\", (long long)v); }}",
                m = m
            ),
            Ty::Dec => format!(
                "static void kl_print_{m}(double v) {{ printf(\"%g\", v); }}",
                m = m
            ),
            Ty::Bool => format!(
                "static void kl_print_{m}(bool v) {{ fputs(v ? {t} : {f}, stdout); }}",
                m = m,
                t = cstr("সত্য"),
                f = cstr("মিথ্যা")
            ),
            Ty::Ch => format!(
                "static void kl_print_{m}(uint32_t v) {{ kl_emit_utf8(v); }}",
                m = m
            ),
            Ty::Txt => format!(
                "static void kl_print_{m}(kl_str v) {{ if (v.data) fputs((const char*)v.data, stdout); }}",
                m = m
            ),
            Ty::Null => format!(
                "static void kl_print_{m}(uint8_t v) {{ (void)v; fputs({n}, stdout); }}",
                m = m,
                n = cstr("ফাঁকা")
            ),
            Ty::Arr(_) | Ty::Shared(_) | Ty::Map(_, _) | Ty::Struct(_) => String::new(),
        }
    }

    fn deep_impl(&mut self, t: &Ty) -> String {
        let m = t.mg();
        match t {
            Ty::Txt => format!(
                "static kl_str kl_deep_{m}(kl_str v) {{ return kl_str_copy(v); }}",
                m = m
            ),
            Ty::Arr(inner) => {
                let cp = if inner.tracked() {
                    format!("r.data[i] = {};", inner.copy_expr("src.data[i]"))
                } else {
                    "r.data[i] = src.data[i];".to_string()
                };
                format!(
                    r#"
static kl_arr_{m} kl_deep_{m}(kl_arr_{m} src) {{
    kl_arr_{m} r = kl_arr_{m}_new(src.len);
    for (int64_t i = 0; i < src.len; i++) {{
        {cp}
    }}
    return r;
}}"#
                )
            }
            Ty::Shared(_) => format!(
                "static kl_sh_{m}* kl_deep_{m}(kl_sh_{m}* p) {{ kl_sh_{m}_incref(p); return p; }}",
                m = m
            ),
            _ => String::new(),
        }
    }
}

impl Gen {
    fn run(&mut self, prog: &Program) {
        self.out.push("#include <stdio.h>".into());
        self.out.push("#include <stdlib.h>".into());
        self.out.push("#include <string.h>".into());
        self.out.push("#include <stdint.h>".into());
        self.out.push("#include <stdbool.h>".into());
        self.out.push("#include <math.h>".into());
        self.out.push("#ifdef _WIN32".into());
        self.out.push("#include <fcntl.h>".into());
        self.out.push("#include <io.h>".into());
        self.out.push("#endif".into());
        self.out.push(self.runtime());
        self.impl_ph = self.out.len();
        self.out.push("@@KL_IMPLS@@".into());

        for f in &prog.funcs {
            let cname = format!("kl_f{}", self.fn_n);
            self.fn_n += 1;
            let mut params = Vec::new();
            for p in &f.params {
                let t = self.decl_ty(&p.name);
                self.ensure(&t);
                params.push((p.name.name.clone(), t));
            }
            let ret = match self.types.funcs.get(&f.name.name) {
                Some((_, r)) => conv(r),
                None => Ty::Null,
            };
            self.ensure(&ret);
            self.funcs.insert(
                f.name.name.clone(),
                FnMeta { cname, params, ret },
            );
        }

        if let Some(app) = &prog.app {
            self.scan_ui(&app.body.stmts);
        }
        for imp in &prog.imports {
            if let Some(m) = kolom_sema::STDLIB_MODULES.iter().find(|m| **m == imp.name) {
                if !self.std_modules.contains(m) {
                    self.std_modules.push(m);
                }
            }
        }
        if self.has_ui {
            if self.target == "windows" || self.target.is_empty() {
                self.ui_block = Some(self.ui_runtime());
            } else {
                self.ui_block = Some(self.ui_stubs().to_string());
            }
            self.ensure_graphics_runtime();
        }
        let mut display_stmts: Vec<Stmt> = Vec::new();
        if self.has_display {
            if let Some(app) = &prog.app {
                for st in &app.body.stmts {
                    if let Stmt::Display(d) = st {
                        display_stmts.extend(d.stmts.iter().cloned());
                    }
                }
            }
            self.out.push("static void kl_build_ui(void);".to_string());
            self.out
                .push("static void kl_app_rebuild(void);".to_string());
        }

        for c in &prog.consts {
            let ct = self.decl_ty(&c.name);
            self.ensure(&ct);
            let v = self.fresh_var();
            self.globals.insert(
                c.name.name.clone(),
                Bnd { c: v.clone(), ty: ct.clone() },
            );
            self.pending_globals.push((v.clone(), ct.clone()));
            self.out
                .push(format!("static {} {}; /* global */", ct.c(), v));
        }

        for f in &prog.funcs {
            let meta = self.funcs.get(&f.name.name).unwrap().clone();
            self.out.push(format!(
                "static {} {}({});",
                meta.ret.c(),
                meta.cname,
                meta.params
                    .iter()
                    .map(|(n, t)| format!("{} {}", t.c(), n))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        for f in &prog.funcs {
            self.lower_fn(f);
        }

        if self.has_display {
            self.w("static void kl_build_ui(void) {");
            self.indent += 1;
            let mut bscope: Vec<HashMap<String, Bnd>> = vec![HashMap::new()];
            self.w("kl_ui_begin();");
            for st in &display_stmts {
                self.lower_stmt(st, &mut bscope);
            }
            self.pop_scope(&mut bscope);
            self.indent -= 1;
            self.w("}");
            self.w("");
            self.w("static void kl_app_rebuild(void) {");
            self.w("    kl_ui_on_rebuild = kl_app_rebuild;");
            self.w("    kl_build_ui();");
            self.w("}");
            self.w("");
        }

        self.w("int main(void) {");
        self.indent += 1;
        self.w("#ifdef _WIN32");
        self.w("    _setmode(_fileno(stdout), _O_BINARY);");
        self.w("#endif");
        if self.has_ui {
            let title = self.title.clone();
            self.w(format!("kl_ui_init({});", cstr(&title)));
        }
        let mut scope: Vec<HashMap<String, Bnd>> = vec![HashMap::new()];
        for c in &prog.consts {
            let (v, ct) = self.pending_globals.remove(0);
            let rhs = self.lower_expr(&c.init, &mut scope);
            if ct.tracked() {
                self.w(format!("{} = {};", v, rhs));
                let u = ct.up_call(&v);
                self.w(format!("{};", u));
            } else {
                self.w(format!("{} = {};", v, rhs));
            }
        }
        if let Some(app) = &prog.app {
            for st in &app.body.stmts {
                if self.has_display {
                    if matches!(st, Stmt::Display(_)) {
                        continue;
                    }
                }
                self.lower_stmt(st, &mut scope);
            }
        }
        if self.has_ui {
            if self.has_display {
                self.w("kl_ui_on_rebuild = kl_app_rebuild;");
                self.w("kl_app_rebuild();");
            }
            self.w("kl_ui_show_and_run();");
        }
        while scope.len() > 1 {
            self.pop_scope(&mut scope);
        }
        self.pop_scope(&mut scope);
        self.indent -= 1;
        self.w("    return 0;");
        self.w("}");
    }

    fn push_scope(scope: &mut Vec<HashMap<String, Bnd>>) {
        scope.push(HashMap::new());
    }

    fn pop_scope(&mut self, scope: &mut Vec<HashMap<String, Bnd>>) {
        if scope.len() <= 1 {
            return;
        }
        let top = scope.pop().unwrap();
        let items: Vec<(String, Ty)> = top.into_iter().map(|(_, b)| (b.c, b.ty)).collect();
        for (v, t) in items.iter().rev() {
            if t.tracked() {
                let d = t.down_call(v);
                self.w(format!("{};", d));
            }
        }
    }

    fn lookup(scope: &[HashMap<String, Bnd>], name: &str) -> Option<Bnd> {
        for m in scope.iter().rev() {
            if let Some(b) = m.get(name) {
                return Some(b.clone());
            }
        }
        None
    }

    fn lower_fn(&mut self, f: &FuncDecl) {
        let meta = self.funcs.get(&f.name.name).unwrap().clone();
        self.w(format!(
            "static {} {}({}) {{",
            meta.ret.c(),
            meta.cname,
            meta.params
                .iter()
                .map(|(n, t)| format!("{} {}", t.c(), n))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        self.indent += 1;
        let mut scope: Vec<HashMap<String, Bnd>> = vec![HashMap::new()];
        for (pn, pt) in &meta.params {
            if pt.tracked() {
                let u = pt.up_call(pn);
                self.w(format!("{};", u));
            }
            scope.last_mut().unwrap().insert(
                pn.clone(),
                Bnd {
                    c: pn.clone(),
                    ty: pt.clone(),
                },
            );
        }
        for st in &f.body.stmts {
            self.lower_stmt(st, &mut scope);
        }
        let items: Vec<(String, Ty)> = scope
            .last()
            .unwrap()
            .values()
            .map(|b| (b.c.clone(), b.ty.clone()))
            .collect();
        for (v, t) in items.iter().rev() {
            if t.tracked() {
                let d = t.down_call(v);
                self.w(format!("{};", d));
            }
        }
        self.w(format!("return {};", meta.ret.zero()));
        self.indent -= 1;
        self.w("}");
        self.w("");
    }

    fn lower_decl_local(&mut self, name: &Ident, init: &Expr, scope: &mut Vec<HashMap<String, Bnd>>) {
        let ct = self.decl_ty(name);
        self.ensure(&ct);
        let v = self.fresh_var();
        let rhs = self.lower_expr(init, scope);
        if ct.tracked() {
            self.w(format!("{} {};", ct.c(), v));
            self.w(format!("{} = {};", v, rhs));
            let u = ct.up_call(&v);
            self.w(format!("{};", u));
        } else {
            self.w(format!("{} {} = {};", ct.c(), v, rhs));
        }
        scope
            .last_mut()
            .unwrap()
            .insert(name.name.clone(), Bnd { c: v, ty: ct });
    }

    fn lower_stmt(&mut self, s: &Stmt, scope: &mut Vec<HashMap<String, Bnd>>) {
        match s {
            Stmt::Var(v) => self.lower_decl_local(&v.name, &v.init, scope),
            Stmt::Const(c) => self.lower_decl_local(&c.name, &c.init, scope),
            Stmt::If(i) => {
                let cond = self.lower_expr(&i.cond, scope);
                self.w(format!("if ({}) {{", cond));
                self.indent += 1;
                Self::push_scope(scope);
                for st in &i.then.stmts {
                    self.lower_stmt(st, scope);
                }
                self.pop_scope(scope);
                self.indent -= 1;
                match &i.els {
                    Some(ElseBranch::Block(b)) => {
                        self.w("} else {");
                        self.indent += 1;
                        Self::push_scope(scope);
                        for st in &b.stmts {
                            self.lower_stmt(st, scope);
                        }
                        self.pop_scope(scope);
                        self.indent -= 1;
                        self.w("}");
                    }
                    Some(ElseBranch::If(inner)) => {
                        self.w("} else {");
                        self.indent += 1;
                        self.lower_stmt(&Stmt::If((**inner).clone()), scope);
                        self.indent -= 1;
                        self.w("}");
                    }
                    None => self.w("}"),
                }
            }
            Stmt::Loop(l) => {
                let count = self.lower_expr(&l.count, scope);
                let cv = self.tmp();
                self.w(format!("int64_t {} = {};", cv, count));
                let iv = self.tmp();
                self.w(format!(
                    "for (int64_t {} = 0; {} < {}; {}++) {{",
                    iv, iv, cv, iv
                ));
                self.indent += 1;
                Self::push_scope(scope);
                for st in &l.body.stmts {
                    self.lower_stmt(st, scope);
                }
                self.pop_scope(scope);
                self.indent -= 1;
                self.w("}");
            }
            Stmt::While(wst) => {
                self.w("for (;;) {");
                self.indent += 1;
                Self::push_scope(scope);
                let cond = self.lower_expr(&wst.cond, scope);
                self.w(format!("if (!({})) break;", cond));
                for st in &wst.body.stmts {
                    self.lower_stmt(st, scope);
                }
                self.pop_scope(scope);
                self.indent -= 1;
                self.w("}");
            }
            Stmt::ForEach(fe) => {
                let iter_ty = conv(&self.types.expr_of(&fe.iter));
                self.ensure(&iter_ty);
                let itv_raw = self.lower_expr(&fe.iter, scope);
                let im = iter_ty.mg();
                let iv = self.tmp();
                self.w(format!("kl_arr_{} {};", im, iv));
                self.w(format!("{} = {};", iv, itv_raw));
                let u = Ty::Arr(match &iter_ty {
                    Ty::Arr(i) => i.clone(),
                    _ => Box::new(Ty::Null),
                })
                .up_call(&iv);
                self.w(format!("{};", u));
                Self::push_scope(scope);
                let kv = self.tmp();
                self.w(format!(
                    "for (int64_t {} = 0; {} < {}.len; {}++) {{",
                    kv, kv, iv, kv
                ));
                self.indent += 1;
                let elem_ty = self.decl_ty(&fe.var);
                let ev = self.fresh_var();
                let get = format!("kl_arr_{}_get({}, {})", im, iv, kv);
                if elem_ty.tracked() {
                    self.ensure(&elem_ty);
                    self.w(format!("{} {};", elem_ty.c(), ev));
                    self.w(format!("{} = {};", ev, get));
                } else {
                    self.w(format!("{} {} = {};", elem_ty.c(), ev, get));
                }
                Self::push_scope(scope);
                scope
                    .last_mut()
                    .unwrap()
                    .insert(fe.var.name.clone(), Bnd { c: ev, ty: elem_ty });
                for st in &fe.body.stmts {
                    self.lower_stmt(st, scope);
                }
                self.pop_scope(scope);
                self.indent -= 1;
                self.w("}");
                let d = Ty::Arr(match &iter_ty {
                    Ty::Arr(i) => i.clone(),
                    _ => Box::new(Ty::Null),
                })
                .down_call(&iv);
                self.w(format!("{};", d));
                self.pop_scope(scope);
            }
            Stmt::Return(r) => {
                let v = match &r.value {
                    Some(e) => self.lower_expr(e, scope),
                    None => "0".into(),
                };
                self.w(format!("return {};", v));
            }
            Stmt::Break(_) => self.w("break;"),
            Stmt::Continue(_) => self.w("continue;"),
            Stmt::Expr(e) => {
                let v = self.lower_expr(e, scope);
                if v != "(0)" {
                    self.w(format!("(void)({});", v));
                }
            }
            Stmt::Nested(b) => {
                self.w("{");
                self.indent += 1;
                Self::push_scope(scope);
                for st in &b.stmts {
                    self.lower_stmt(st, scope);
                }
                self.pop_scope(scope);
                self.indent -= 1;
                self.w("}");
            }
            Stmt::Widget(wd) => match wd.kw.as_str() {
                "ইনপুট" => {
                    self.w("kl_ui_input();");
                }
                "ক্যানভাস" => {
                    let joined: Vec<String> = wd
                        .args
                        .iter()
                        .map(|a| self.lower_expr(a, scope))
                        .collect();
                    let w0 = joined.get(0).cloned().unwrap_or_else(|| "100".into());
                    let h0 = joined.get(1).cloned().unwrap_or_else(|| "80".into());
                    self.w(format!("kl_ui_canvas({}, {});", w0, h0));
                }
                "ছবি" => {
                    if let Some(a) = wd.args.first() {
                        let at = self.ety(a);
                        self.ensure(&at);
                        let v = self.lower_expr(a, scope);
                        if at == Ty::Txt {
                            let tv = self.tmp();
                            self.w(format!("{} {} = kl_str_lit(\"\");", at.c(), tv));
                            self.w(format!("if ({}.data) {} = {};", v, tv, v));
                            let d = at.down_call(&tv);
                            self.w(format!("{};", d));
                            self.w(format!("kl_ui_image((const char*){}.data);", tv));
                        } else {
                            self.w("kl_ui_image(\"\");");
                        }
                    } else {
                        self.w("kl_ui_image(\"\");");
                    }
                }
                "টেক্সট" => {
                    let label = self.widget_label(wd.args.first(), scope);
                    self.w(format!("kl_ui_text({});", label));
                }
                "বাটন" => {
                    let label = self.widget_label(wd.args.first(), scope);
                    let handler = wd
                        .args
                        .get(1)
                        .map(|h| match &h.kind {
                            ExprKind::Ident(id) => self
                                .funcs
                                .get(&id.name)
                                .map(|m| format!("(kl_handler){}", m.cname))
                                .unwrap_or_else(|| "NULL".into()),
                            _ => "NULL".into(),
                        })
                        .unwrap_or_else(|| "NULL".into());
                    self.w(format!("kl_ui_button({}, {});", label, handler));
                }
                container_kind => {
                    let kind = match container_kind {
                        "সারি" => "KL_W_ROW",
                        "কার্ড" => "KL_W_CARD",
                        "ডায়ালগ" => "KL_W_DIALOG",
                        "স্ক্রল" => "KL_W_SCROLL",
                        _ => "KL_W_COL",
                    };
                    if let Some(b) = &wd.body {
                        self.w(format!("kl_ui_push({});", kind));
                        self.indent += 1;
                        Self::push_scope(scope);
                        for st in &b.stmts {
                            self.lower_stmt(st, scope);
                        }
                        self.pop_scope(scope);
                        self.indent -= 1;
                        self.w("kl_ui_pop();");
                    } else {
                        self.w(format!("kl_ui_push({});", kind));
                        self.w("kl_ui_pop();");
                    }
                }
            },
            Stmt::TryCatch(tc) => {
                self.w("{");
                self.indent += 1;
                self.w("#ifdef _WIN32");
                self.w("    jmp_buf kl_try_buf;");
                self.w("    extern char kl_last_err[512];");
                self.w("    extern jmp_buf* kl_try_stack[];");
                self.w("    extern int kl_try_sp;");
                self.w("    kl_try_stack[kl_try_sp++] = &kl_try_buf;");
                self.w("    if (setjmp(kl_try_buf) == 0) {");
                self.indent += 1;
                Self::push_scope(scope);
                for st in &tc.body.stmts {
                    self.lower_stmt(st, scope);
                }
                self.pop_scope(scope);
                self.indent -= 1;
                self.w("    } else {");
                self.w("        kl_try_sp--;");
                self.indent += 1;
                Self::push_scope(scope);
                let ev = self.fresh_var();
                self.w(format!("kl_str {} = kl_str_lit(kl_last_err);", ev));
                scope.last_mut().unwrap().insert(
                    tc.err_var.name.clone(),
                    Bnd { c: ev, ty: Ty::Txt },
                );
                for st in &tc.handler.stmts {
                    self.lower_stmt(st, scope);
                }
                self.pop_scope(scope);
                self.indent -= 1;
                self.w("    }");
                self.w("#endif");
                self.indent -= 1;
                self.w("}");
            }
            Stmt::Display(_) => {}
        }
    }

    fn ensure_math_runtime(&mut self) {
        if !self.std_modules.contains(&"__math_emitted") {
            self.std_modules.push("__math_emitted");
            self.impls.push(
                "static int64_t kl_math_abs(int64_t v) { return v < 0 ? -v : v; }\n"
                    .to_string(),
            );
        }
    }

    fn ensure_str_runtime(&mut self) {        if !self.std_modules.contains(&"__str_emitted") {
            self.std_modules.push("__str_emitted");
            self.impls.push(str_runtime().to_string());
        }
    }

    fn ensure_io_runtime(&mut self) {
        if !self.std_modules.contains(&"__io_emitted") {
            self.std_modules.push("__io_emitted");
            // `লাইন_তালিকা`-র রিটার্ন টাইপ `লেখা[]` — সেই প্রোগ্রাম অন্য কোথাও
            // `লেখা[]` ব্যবহার না করলেও `kl_arr_as` টাইপ/হেল্পার নিশ্চিতভাবে
            // এমিট হওয়া দরকার।
            self.ensure(&Ty::Arr(Box::new(Ty::Txt)));
            // `kl_io_read_lines` (নিচে) `kl_str_split` কল করে, যেটা
            // str_runtime()-এ — সেই টেক্সট এই io_runtime()-এর *আগে* এমিট
            // হওয়া চাই, নাহলে C ফরওয়ার্ড-রেফারেন্স এরর দেয়।
            self.ensure_str_runtime();
            self.impls.push(io_runtime().to_string());
        }
    }

    fn ensure_fs_runtime(&mut self) {
        if !self.std_modules.contains(&"__fs_emitted") {
            self.std_modules.push("__fs_emitted");
            self.ensure(&Ty::Arr(Box::new(Ty::Txt)));
            self.impls.push(fs_runtime().to_string());
        }
    }

    fn ensure_path_runtime(&mut self) {
        if !self.std_modules.contains(&"__path_emitted") {
            self.std_modules.push("__path_emitted");
            self.impls.push(path_runtime().to_string());
        }
    }

    fn ensure_json_runtime(&mut self) {
        if !self.std_modules.contains(&"__json_emitted") {
            self.std_modules.push("__json_emitted");
            self.impls.push(json_runtime().to_string());
        }
    }

    fn ensure_net_runtime(&mut self) {
        if !self.std_modules.contains(&"__net_emitted") {
            self.std_modules.push("__net_emitted");
            self.impls.push(net_runtime().to_string());
        }
    }

    fn ensure_graphics_runtime(&mut self) {
        if self.gfx_block.is_none() {
            self.gfx_block = Some(graphics_runtime().to_string());
        }
    }

    fn ensure_rand_runtime(&mut self) {
        if !self.std_modules.contains(&"__rand_emitted") {
            self.std_modules.push("__rand_emitted");
            self.impls.push(rand_runtime().to_string());
        }
    }

    fn call_stdlib(
        &mut self,
        module: &str,
        item: &str,
        args: &[Expr],
        scope: &mut Vec<HashMap<String, Bnd>>,
        ret_out: &mut Ty,
    ) -> String {
        let vals: Vec<(Ty, String)> = args
            .iter()
            .map(|a| {
                let t = self.ety(a);
                let v = self.lower_expr(a, scope);
                (t, v)
            })
            .collect();
        let g = |i: usize| vals.get(i).map(|(_, v)| v.clone()).unwrap_or_else(|| "0".into());
        if module == "গ্রাফিক্স" && item == "টিক" {
            self.ensure_rand_runtime();
            let ms = g(0);
            let handler = args
                .get(1)
                .map(|h| match &h.kind {
                    ExprKind::Ident(id) => self
                        .funcs
                        .get(&id.name)
                        .map(|m| format!("(kl_handler){}", m.cname))
                        .unwrap_or_else(|| "NULL".into()),
                    _ => "NULL".into(),
                })
                .unwrap_or_else(|| "NULL".into());
            *ret_out = Ty::Null;
            return format!("kl_ui_tick({}, {})", ms, handler);
        }
        if module == "গ্রাফিক্স" {
            self.ensure_graphics_runtime();
            *ret_out = Ty::Null;
            let fname = match item {
                "রঙ" => "kl_g_color",
                "বিন্দু" => "kl_g_pixel",
                "রেখা" => "kl_g_line",
                "আয়ত" => "kl_g_rect",
                "ভরাট_আয়ত" => "kl_g_fillrect",
                "বৃত্ত" => "kl_g_circle",
                "ভরাট_বৃত্ত" => "kl_g_fillcircle",
                "লেখা" => "kl_g_text",
                "ফন্ট" => "kl_g_font",
                _ => "kl_g_nop",
            };
            let mut parts: Vec<String> = Vec::new();
            for (i, a) in args.iter().enumerate() {
                let t = self.ety(a);
                self.ensure(&t);
                let v = self.lower_expr(a, scope);
                let wants_cstr = (item == "লেখা" && i == 2) || (item == "ফন্ট" && i == 0);
                if wants_cstr {
                    let tv = self.tmp();
                    self.w(format!("{} {} = kl_str_lit(\"\");", t.c(), tv));
                    self.w(format!("if ({}.data) {} = {};", v, tv, v));
                    let d = t.down_call(&tv);
                    self.w(format!("{};", d));
                    parts.push(format!("(const char*){}.data", tv));
                } else {
                    parts.push(v);
                }
            }
            return format!("{}({})", fname, parts.join(", "));
        }
        match (module, item) {
            // Overloaded on the argument's type; sema has already checked it.
            ("গণিত", "পরম_মান") => {
                if matches!(self.ety(&args[0]), Ty::Dec) {
                    *ret_out = Ty::Dec;
                    format!("fabs({})", g(0))
                } else {
                    self.ensure_math_runtime();
                    *ret_out = Ty::Num;
                    format!("kl_math_abs({})", g(0))
                }
            }
            ("গণিত", "বর্গমূল") => {
                *ret_out = Ty::Dec;
                format!("sqrt({})", g(0))
            }
            ("গণিত", "ঘাত") => {
                *ret_out = Ty::Dec;
                format!("pow({}, {})", g(0), g(1))
            }
            ("গণিত", "ফ্লোর") => {
                *ret_out = Ty::Num;
                format!("(int64_t)floor({})", g(0))
            }
            ("গণিত", "সিলিং") => {
                *ret_out = Ty::Num;
                format!("(int64_t)ceil({})", g(0))
            }
            ("গণিত", "রাউন্ডঅফ") => {
                *ret_out = Ty::Num;
                format!("(int64_t)round({})", g(0))
            }
            ("গণিত", "সাইন") => {
                *ret_out = Ty::Dec;
                format!("sin({})", g(0))
            }
            ("গণিত", "কোসাইন") => {
                *ret_out = Ty::Dec;
                format!("cos({})", g(0))
            }
            ("গণিত", "ট্যান") => {
                *ret_out = Ty::Dec;
                format!("tan({})", g(0))
            }
            ("গণিত", "লগ") => {
                *ret_out = Ty::Dec;
                format!("log10({})", g(0))
            }
            ("গণিত", "লন") => {
                *ret_out = Ty::Dec;
                format!("log({})", g(0))
            }
            // The ternary works for both types, so only the result type
            // needs deciding. Sema has checked that the arguments agree.
            ("গণিত", "সর্বনিম্ন") | ("গণিত", "সর্বোচ্চ") => {
                *ret_out = if matches!(self.ety(&args[0]), Ty::Dec) { Ty::Dec } else { Ty::Num };
                let op = if item == "সর্বনিম্ন" { "<" } else { ">" };
                format!("(({}) {} ({}) ? ({}) : ({}))", g(0), op, g(1), g(0), g(1))
            }

            ("লেখা", _) => {
                self.ensure_str_runtime();
                *ret_out = match item {
                    "স্প্লিট" => Ty::Arr(Box::new(Ty::Txt)),
                    "জুড়াও" | "বড়হাতের" | "ছোটহাতের" | "ছাঁটো" | "বদলাও" | "স্লাইস" => Ty::Txt,
                    "খুঁজো" => Ty::Num,
                    _ => Ty::Bool,
                };
                let fname = match item {
                    "বড়হাতের" => "kl_str_upper",
                    "ছোটহাতের" => "kl_str_lower",
                    "ছাঁটো" => "kl_str_trim",
                    "স্প্লিট" => "kl_str_split",
                    "জুড়াও" => "kl_str_join",
                    "বদলাও" => "kl_str_replace",
                    "খুঁজো" => "kl_str_find",
                    "স্লাইস" => "kl_str_slice",
                    "শুরুতে_আছে" => "kl_str_starts",
                    _ => "kl_str_ends",
                };
                let joined: Vec<String> = vals.iter().map(|(_, v)| v.clone()).collect();
                format!("{}({})", fname, joined.join(", "))
            }

            ("ফাইল", _) => {
                self.ensure_io_runtime();
                *ret_out = match item {
                    "পড়ো" => Ty::Txt,
                    "লাইন_তালিকা" => Ty::Arr(Box::new(Ty::Txt)),
                    _ => Ty::Null,
                };
                // এক্সপ্লিসিট ম্যাচ, wildcard নয় — sema-approved কিন্তু এখানে
                // বাস্তবায়িত নয় এমন কোনো item silently ভুল ফাংশনে না গিয়ে
                // স্পষ্ট প্যানিক দেয়।
                let fname = match item {
                    "পড়ো" => "kl_io_read_file",
                    "লেখো" => "kl_io_write_file",
                    "এপেন্ড" => "kl_io_append_file",
                    "লাইন_তালিকা" => "kl_io_read_lines",
                    other => panic!("ফাইল.{other}: --সি (legacy) ব্যাকএন্ডে সমর্থিত নয়, ডিফল্ট Cranelift ব্যাকএন্ড ব্যবহার করুন"),
                };
                let joined: Vec<String> = vals.iter().map(|(_, v)| v.clone()).collect();
                format!("{}({})", fname, joined.join(", "))
            }

            ("ফাইলসিস্টেম", _) => {
                self.ensure_fs_runtime();
                *ret_out = match item {
                    "ফাইল_আছে" | "ডিরেক্টরি_আছে" => Ty::Bool,
                    "তালিকা" => Ty::Arr(Box::new(Ty::Txt)),
                    "আকার" => Ty::Num,
                    "বর্তমান_ডিরেক্টরি" => Ty::Txt,
                    _ => Ty::Null,
                };
                let fname = match item {
                    "ফাইল_আছে" => "kl_fs_file_exists",
                    "ডিরেক্টরি_আছে" => "kl_fs_dir_exists",
                    "ডিরেক্টরি_বানাও" => "kl_fs_mkdir",
                    "মুছো" => "kl_fs_remove",
                    "তালিকা" => "kl_fs_list",
                    "কপি" => "kl_fs_copy",
                    "সরাও" => "kl_fs_move",
                    "আকার" => "kl_fs_size",
                    "বর্তমান_ডিরেক্টরি" => "kl_fs_cwd",
                    // রিকার্সিভ ডিরেক্টরি ওয়াক (POSIX+Win32 দুই দিকেই) এবং
                    // Windows FILETIME→epoch-ms রূপান্তর — legacy ব্যাকএন্ডে
                    // ইচ্ছাকৃতভাবে বাদ, ডিফল্ট Cranelift ব্যাকএন্ডে যান।
                    other => panic!("ফাইলসিস্টেম.{other}: --সি (legacy) ব্যাকএন্ডে সমর্থিত নয়, ডিফল্ট Cranelift ব্যাকএন্ড ব্যবহার করুন"),
                };
                let joined: Vec<String> = vals.iter().map(|(_, v)| v.clone()).collect();
                format!("{}({})", fname, joined.join(", "))
            }

            ("পাথ", _) => {
                self.ensure_path_runtime();
                *ret_out = Ty::Txt;
                let fname = match item {
                    "জোড়ো" => "kl_path_join",
                    "ফাইলনাম" => "kl_path_basename",
                    "ডিরেক্টরিনাম" => "kl_path_dirname",
                    "এক্সটেনশন" => "kl_path_extension",
                    "পরম_পাথ" => "kl_path_abs",
                    other => panic!("পাথ.{other}: --সি (legacy) ব্যাকএন্ডে সমর্থিত নয়, ডিফল্ট Cranelift ব্যাকএন্ড ব্যবহার করুন"),
                };
                let joined: Vec<String> = vals.iter().map(|(_, v)| v.clone()).collect();
                format!("{}({})", fname, joined.join(", "))
            }

            ("জেসন", _) => {
                self.ensure_json_runtime();
                *ret_out = match item {
                    "বৈধ" => Ty::Bool,
                    "বের_হও" | "লেখা_বের_করো" => Ty::Txt,
                    _ => Ty::Num,
                };
                let fname = match item {
                    "বৈধ" => "kl_json_valid",
                    "বের_হও" => "kl_json_escape",
                    "লেখা_বের_করো" => "kl_json_get_string",
                    _ => "kl_json_get_int",
                };
                let joined: Vec<String> = vals.iter().map(|(_, v)| v.clone()).collect();
                format!("{}({})", fname, joined.join(", "))
            }

            ("নেটওয়ার্ক", _) => {
                self.ensure_net_runtime();
                *ret_out = match item {
                    "কানেক্ট" => Ty::Num,
                    "রিসিভ" => Ty::Txt,
                    _ => Ty::Null,
                };
                let fname = match item {
                    "কানেক্ট" => "kl_net_connect",
                    "সেন্ড" => "kl_net_send",
                    "রিসিভ" => "kl_net_recv",
                    _ => "kl_net_close",
                };
                let joined: Vec<String> = vals.iter().map(|(_, v)| v.clone()).collect();
                format!("{}({})", fname, joined.join(", "))
            }

            ("সময়", "এখন_মিলিসেকেন্ড") => {
                *ret_out = Ty::Num;
                "kl_time_now_ms()".into()
            }
            ("সময়", "সেকেন্ড") => {
                *ret_out = Ty::Dec;
                "kl_time_clock()".into()
            }

            ("র‍্যান্ডম", _) => {
                self.ensure_rand_runtime();
                let joined: Vec<String> = vals.iter().map(|(_, v)| v.clone()).collect();
                let fname = match item {
                    "বীজ" => "kl_rand_seed",
                    "সংখ্যা" => "kl_rand_int",
                    "মধ্যে" => "kl_rand_range",
                    _ => "kl_rand_float",
                };
                *ret_out = match item {
                    "বীজ" => Ty::Null,
                    "সংখ্যা" | "মধ্যে" => Ty::Num,
                    _ => Ty::Dec,
                };
                format!("{}({})", fname, joined.join(", "))
            }

            _ => "(0)".into(),
        }
    }

    fn widget_label(
        &mut self,
        arg: Option<&Expr>,
        scope: &mut Vec<HashMap<String, Bnd>>,
    ) -> String {
        match arg {
            Some(a) => {
                let at = self.ety(a);
                self.ensure(&at);
                let v = self.lower_expr(a, scope);
                if at == Ty::Txt {
                    let tv = self.tmp();
                    self.w(format!("{} {} = kl_str_lit(\"\");", at.c(), tv));
                    self.w(format!("if ({}.data) {} = {};", v, tv, v));
                    let d = at.down_call(&tv);
                    self.w(format!("{};", d));
                    format!("(const char*){}.data", tv)
                } else {
                    "\"\"".into()
                }
            }
            None => "\"\"".into(),
        }
    }

    fn ui_runtime(&self) -> String {
        r#"
#ifdef _WIN32
#include <windows.h>
#include <usp10.h>

typedef void (*kl_handler)(void);

static HMODULE kl_usp = NULL;
static HRESULT (WINAPI *kl_pfn_analyse)(HDC, const void*, int, int, int, DWORD, int,
    SCRIPT_CONTROL*, SCRIPT_STATE*, const int*, SCRIPT_TABDEF*, const BYTE*,
    SCRIPT_STRING_ANALYSIS*) = NULL;
static HRESULT (WINAPI *kl_pfn_out)(SCRIPT_STRING_ANALYSIS, int, int, UINT, const RECT*,
    int, int, BOOL) = NULL;
static HRESULT (WINAPI *kl_pfn_free)(SCRIPT_STRING_ANALYSIS*) = NULL;

static void kl_ui_load_usp(void) {
    if (kl_usp) return;
    kl_usp = LoadLibraryW(L"usp10.dll");
    if (!kl_usp) return;
    kl_pfn_analyse = (void*)GetProcAddress(kl_usp, "ScriptStringAnalyse");
    kl_pfn_out = (void*)GetProcAddress(kl_usp, "ScriptStringOut");
    kl_pfn_free = (void*)GetProcAddress(kl_usp, "ScriptStringFree");
}

enum { KL_W_TEXT = 0, KL_W_BUTTON = 1, KL_W_INPUT = 2,
       KL_W_ROW = 3, KL_W_COL = 4, KL_W_CARD = 5, KL_W_DIALOG = 6, KL_W_SCROLL = 7,
       KL_W_CANVAS = 8, KL_W_IMAGE = 9 };

#define KL_MAXW 256
#define KL_MAXK 24

static HWND kl_hwnd;
static void (*kl_ui_on_rebuild)(void) = NULL;
static void (*kl_tick_fn)(void) = NULL;
static UINT kl_tick_ms = 0;

typedef struct {
    int kind;
    wchar_t* text;
    kl_handler handler;
    wchar_t inbuf[128];
    int ilen;
    int focused;
    int child[KL_MAXK];
    int nchild;
    RECT r;
    int gw, gh;
    HBITMAP bmp;
} kl_wid;

static kl_wid kl_pool[KL_MAXW];
static int kl_np = 0;
static int kl_root = -1;
static int kl_stack[64];
static int kl_sp = 0;

static int kl_t_alloc(int kind) {
    if (kl_np >= KL_MAXW) return -1;
    int i = kl_np++;
    memset(&kl_pool[i], 0, sizeof(kl_wid));
    kl_pool[i].kind = kind;
    if (kl_sp > 0 && kl_stack[kl_sp - 1] >= 0) {
        kl_wid* p = &kl_pool[kl_stack[kl_sp - 1]];
        if (p->nchild < KL_MAXK) p->child[p->nchild++] = i;
    }
    return i;
}

static wchar_t* kl_to_wide(const char* utf8) {
    int n = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, NULL, 0);
    wchar_t* w = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, w, n);
    return w;
}

static void kl_ui_begin(void) {
    kl_np = 0; kl_sp = 0;
    kl_root = kl_t_alloc(KL_W_COL);
    kl_stack[kl_sp++] = kl_root;
}

static void kl_ui_text(const char* utf8) {
    int i = kl_t_alloc(KL_W_TEXT);
    if (i >= 0) kl_pool[i].text = kl_to_wide(utf8);
}

static void kl_ui_button(const char* utf8, kl_handler h) {
    int i = kl_t_alloc(KL_W_BUTTON);
    if (i >= 0) { kl_pool[i].text = kl_to_wide(utf8); kl_pool[i].handler = h; }
}

static void kl_ui_input(void) {
    kl_t_alloc(KL_W_INPUT);
}

static void kl_ui_canvas(int w, int h) {
    int i = kl_t_alloc(KL_W_CANVAS);
    if (i >= 0) { kl_pool[i].gw = w; kl_pool[i].gh = h; }
}

static void kl_ui_image(const char* path_utf8) {
    int i = kl_t_alloc(KL_W_IMAGE);
    if (i < 0) return;
    int n = MultiByteToWideChar(CP_UTF8, 0, path_utf8, -1, NULL, 0);
    wchar_t* w = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    MultiByteToWideChar(CP_UTF8, 0, path_utf8, -1, w, n);
    HBITMAP bmp = (HBITMAP)LoadImageW(NULL, w, IMAGE_BITMAP, 0, 0,
        LR_LOADFROMFILE | LR_CREATEDIBSECTION);
    free(w);
    if (i >= 0 && i < KL_MAXW) kl_pool[i].bmp = bmp;
}

static void kl_ui_tick(int ms, kl_handler h) {
    kl_tick_fn = h;
    if (ms < 16) ms = 16;
    SetTimer(kl_hwnd, 3, (UINT)ms, NULL);
}

static void kl_ui_push(int kind) {
    int i = kl_t_alloc(kind);
    if (kl_sp < 64 && i >= 0) kl_stack[kl_sp++] = i;
    else if (i >= 0) kl_np--;
}

static void kl_ui_pop(void) {
    if (kl_sp > 1) kl_sp--;
}

static HFONT kl_ui_font(void) {
    return CreateFontW(-26, 0, 0, 0, FW_NORMAL, 0, 0, 0, DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
        DEFAULT_PITCH | FF_DONTCARE, L"Nirmala UI");
}

static void kl_ui_shaped(HDC dc, const wchar_t* txt, int x, int y) {
    if (kl_pfn_analyse && kl_pfn_out && kl_pfn_free) {
        SCRIPT_STRING_ANALYSIS ssa = NULL;
        HRESULT hr = kl_pfn_analyse(dc, txt, lstrlenW(txt), 0, -1,
            SSA_GLYPHS | SSA_FALLBACK, 0, NULL, NULL, NULL, NULL, NULL, &ssa);
        if (SUCCEEDED(hr) && ssa) {
            kl_pfn_out(ssa, x, y, 0, NULL, 0, 0, FALSE);
            kl_pfn_free(&ssa);
            return;
        }
    }
    TextOutW(dc, x, y, txt, lstrlenW(txt));
}

static int kl_leaf_h(void) { return 52; }
static int kl_gap(void) { return 16; }

static int kl_measure(int idx, int w) {
    kl_wid* n = &kl_pool[idx];
    switch (n->kind) {
    case KL_W_CANVAS:
        return n->gh > 0 ? n->gh + 8 : 88;
    case KL_W_IMAGE: {
        if (n->bmp) {
            BITMAP bm;
            if (GetObject(n->bmp, sizeof(bm), &bm)) return bm.bmHeight;
        }
        return 80;
    }
    case KL_W_ROW: {
        int mx = 0;
        for (int i = 0; i < n->nchild; i++) {
            int cw = (w - (n->nchild - 1) * kl_gap()) / (n->nchild > 0 ? n->nchild : 1);
            int ch = kl_measure(n->child[i], cw);
            if (ch > mx) mx = ch;
        }
        return mx;
    }
    case KL_W_CARD: case KL_W_DIALOG: {
        int total = 24;
        for (int i = 0; i < n->nchild; i++) {
            total += kl_measure(n->child[i], w - 24) + kl_gap();
        }
        return total;
    }
    default: {
        int total = 0;
        for (int i = 0; i < n->nchild; i++) {
            total += kl_measure(n->child[i], w) + kl_gap();
        }
        return (total > 0 ? total - kl_gap() : kl_leaf_h());
    }
    }
}

static void kl_paint(HDC dc, int idx, RECT box) {
    kl_wid* n = &kl_pool[idx];
    n->r = box;
    HFONT f = kl_ui_font();
    HFONT old = (HFONT)SelectObject(dc, f);
    SetBkMode(dc, TRANSPARENT);

    switch (n->kind) {
    case KL_W_TEXT: {
        SetTextColor(dc, RGB(25, 25, 25));
        kl_ui_shaped(dc, n->text ? n->text : L"", box.left + 6, box.top + 6);
        break;
    }
    case KL_W_BUTTON: {
        HBRUSH b = CreateSolidBrush(RGB(0, 120, 215));
        FillRect(dc, &box, b);
        DeleteObject(b);
        FrameRect(dc, &box, (HBRUSH)GetStockObject(GRAY_BRUSH));
        SetTextColor(dc, RGB(255, 255, 255));
        SIZE sz = {0, 0};
        const wchar_t* t = n->text ? n->text : L"";
        GetTextExtentPoint32W(dc, t, lstrlenW(t), &sz);
        kl_ui_shaped(dc, t,
            box.left + ((box.right - box.left) - sz.cx) / 2,
            box.top + ((box.bottom - box.top) - sz.cy) / 2);
        break;
    }
    case KL_W_INPUT: {
        HBRUSH b = CreateSolidBrush(RGB(250, 250, 250));
        FillRect(dc, &box, b);
        DeleteObject(b);
        FrameRect(dc, &box, (HBRUSH)GetStockObject(n->focused ? RGB(0, 120, 215) : RGB(160, 160, 160)));
        SetTextColor(dc, RGB(25, 25, 25));
        kl_ui_shaped(dc, n->inbuf, box.left + 10, box.top + 10);
        if (n->focused) {
            SIZE sz = {0, 0};
            GetTextExtentPoint32W(dc, n->inbuf, n->ilen, &sz);
            RECT cr = {box.left + 12 + sz.cx, box.top + 10, box.left + 14 + sz.cx, box.bottom - 10};
            HBRUSH cb = CreateSolidBrush(RGB(0, 120, 215));
            FillRect(dc, &cr, cb);
            DeleteObject(cb);
        }
        break;
    }
    case KL_W_CANVAS: {
        HBRUSH b = CreateSolidBrush(RGB(255, 255, 255));
        FillRect(dc, &box, b);
        DeleteObject(b);
        FrameRect(dc, &box, (HBRUSH)GetStockObject(GRAY_BRUSH));
        int saved = SaveDC(dc);
        IntersectClipRect(dc, box.left, box.top, box.right, box.bottom);
        POINT oldorg = {0, 0};
        SetViewportOrgEx(dc, box.left, box.top, &oldorg);
        for (int ci = 0; ci < kl_ng; ci++) {
            kl_gcmd* cm = &kl_gbuf[ci];
            switch (cm->op) {
            case KL_C_COLOR:
                SetDCBrushColor(dc, RGB(cm->a, cm->b, cm->c));
                SetTextColor(dc, RGB(cm->a, cm->b, cm->c));
                break;
            case KL_C_PIXEL:
                SetPixel(dc, cm->a, cm->b, kl_g_color_cur);
                break;
            case KL_C_LINE: {
                MoveToEx(dc, cm->a, cm->b, NULL);
                LineTo(dc, cm->c, cm->d);
                break;
            }
            case KL_C_RECT: {
                HBRUSH br = (HBRUSH)GetStockObject(HOLLOW_BRUSH);
                HBRUSH ob = (HBRUSH)SelectObject(dc, br);
                HPEN pn = CreatePen(PS_SOLID, 1, kl_g_color_cur);
                HPEN opn = (HPEN)SelectObject(dc, pn);
                Rectangle(dc, cm->a, cm->b, cm->a + cm->c, cm->b + cm->d);
                SelectObject(dc, ob); DeleteObject(pn); SelectObject(dc, ob);
                break;
            }
            case KL_C_FILLRECT: {
                HBRUSH br = CreateSolidBrush(kl_g_color_cur);
                RECT fr = {cm->a, cm->b, cm->a + cm->c, cm->b + cm->d};
                FillRect(dc, &fr, br);
                DeleteObject(br);
                break;
            }
            case KL_C_CIRCLE: {
                HBRUSH br = (HBRUSH)GetStockObject(HOLLOW_BRUSH);
                HBRUSH ob = (HBRUSH)SelectObject(dc, br);
                HPEN pn = CreatePen(PS_SOLID, 1, kl_g_color_cur);
                HPEN opn = (HPEN)SelectObject(dc, pn);
                Ellipse(dc, cm->a - cm->c, cm->b - cm->c, cm->a + cm->c, cm->b + cm->c);
                SelectObject(dc, ob); DeleteObject(pn); SelectObject(dc, ob);
                break;
            }
            case KL_C_FILLCIRCLE: {
                HBRUSH br = CreateSolidBrush(kl_g_color_cur);
                HBRUSH ob = (HBRUSH)SelectObject(dc, br);
                HPEN pn = (HPEN)GetStockObject(NULL_PEN);
                HPEN opn = (HPEN)SelectObject(dc, pn);
                Ellipse(dc, cm->a - cm->c, cm->b - cm->c, cm->a + cm->c, cm->b + cm->c);
                SelectObject(dc, ob); SelectObject(dc, opn); DeleteObject(br);
                break;
            }
            case KL_C_TEXT: {
                if (cm->txt) kl_ui_shaped(dc, cm->txt, cm->a, cm->b);
                break;
            }
            case KL_C_FONT: {
                HFONT nf = CreateFontW(-cm->a, 0, 0, 0, FW_NORMAL, 0, 0, 0,
                    DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE,
                    cm->txt ? cm->txt : L"Nirmala UI");
                HFONT of = (HFONT)SelectObject(dc, nf);
                static HFONT keep[8]; static int kn = 0;
                if (kn < 8) keep[kn++] = of;
                break;
            }
            }
        }
        RestoreDC(dc, saved);
        break;
    }
    case KL_W_IMAGE: {
        if (n->bmp) {
            HDC mem = CreateCompatibleDC(dc);
            HBITMAP old = (HBITMAP)SelectObject(mem, n->bmp);
            BITMAP bm;
            if (GetObject(n->bmp, sizeof(bm), &bm)) {
                int bw = bm.bmWidth, bh = bm.bmHeight;
                int bwid = box.right - box.left;
                double sc = bwid > 0 ? (double)bwid / bw : 1.0;
                if (bh * sc > (box.bottom - box.top)) sc = (double)(box.bottom - box.top) / bh;
                SetStretchBltMode(dc, COLORONCOLOR);
                StretchBlt(dc, box.left, box.top, (int)(bw * sc), (int)(bh * sc),
                    mem, 0, 0, bw, bh, SRCCOPY);
            }
            SelectObject(mem, old);
            DeleteDC(mem);
        } else {
            HBRUSH b = CreateSolidBrush(RGB(220, 220, 220));
            FillRect(dc, &box, b);
            DeleteObject(b);
            FrameRect(dc, &box, (HBRUSH)GetStockObject(GRAY_BRUSH));
        }
        break;
    }
    case KL_W_ROW: {
        int k = n->nchild > 0 ? n->nchild : 1;
        int cw = ((box.right - box.left) - (k - 1) * kl_gap()) / k;
        int cx = box.left;
        for (int i = 0; i < n->nchild; i++) {
            int ch = kl_measure(n->child[i], cw);
            RECT cr = {cx, box.top, cx + cw, box.top + ch};
            kl_paint(dc, n->child[i], cr);
            cx += cw + kl_gap();
        }
        break;
    }
    case KL_W_CARD: case KL_W_DIALOG: {
        HBRUSH b = CreateSolidBrush(n->kind == KL_W_CARD ? RGB(245, 245, 245) : RGB(235, 240, 250));
        FillRect(dc, &box, b);
        DeleteObject(b);
        FrameRect(dc, &box, (HBRUSH)GetStockObject(GRAY_BRUSH));
        int y = box.top + 12;
        for (int i = 0; i < n->nchild; i++) {
            int ch = kl_measure(n->child[i], (box.right - box.left) - 24);
            RECT cr = {box.left + 12, y, box.right - 12, y + ch};
            kl_paint(dc, n->child[i], cr);
            y += ch + kl_gap();
        }
        break;
    }
    default: {
        int y = box.top;
        for (int i = 0; i < n->nchild; i++) {
            int ch = kl_measure(n->child[i], box.right - box.left);
            RECT cr = {box.left, y, box.right, y + ch};
            kl_paint(dc, n->child[i], cr);
            y += ch + kl_gap();
        }
        break;
    }
    }
    SelectObject(dc, old);
    DeleteObject(f);
}

static int kl_test_next_click(void);

static LRESULT CALLBACK kl_wndproc(HWND h, UINT msg, WPARAM wp, LPARAM lp) {
    switch (msg) {    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(h, &ps);
        RECT rc;
        GetClientRect(h, &rc);
        FillRect(dc, &rc, (HBRUSH)GetStockObject(WHITE_BRUSH));
        if (kl_root >= 0 && kl_root < kl_np) kl_paint(dc, kl_root, rc);
        EndPaint(h, &ps);
        return 0;
    }
    case WM_LBUTTONDOWN: {
        int mx = (short)LOWORD(lp), my = (short)HIWORD(lp);
        int hit_btn = -1, hit_in = -1;
        for (int i = 0; i < kl_np; i++) {
            RECT* r = &kl_pool[i].r;
            if (mx >= r->left && mx <= r->right && my >= r->top && my <= r->bottom) {
                if (kl_pool[i].kind == KL_W_BUTTON && hit_btn < 0) hit_btn = i;
                if (kl_pool[i].kind == KL_W_INPUT) hit_in = i;
            }
        }
        int changed = 0;
        for (int i = 0; i < kl_np; i++)
            if (kl_pool[i].kind == KL_W_INPUT && kl_pool[i].focused != (i == hit_in)) {
                kl_pool[i].focused = (i == hit_in);
                changed = 1;
            }
        if (hit_btn >= 0 && kl_pool[hit_btn].handler) {
            kl_pool[hit_btn].handler();
            if (kl_ui_on_rebuild) { kl_ui_on_rebuild(); InvalidateRect(h, NULL, TRUE); return 0; }
            changed = 1;
        }
        if (changed) InvalidateRect(h, NULL, TRUE);
        SetFocus(h);
        return 0;
    }
    case WM_CHAR: {
        wchar_t c = (wchar_t)wp;
        for (int i = 0; i < kl_np; i++) {
            if (kl_pool[i].kind == KL_W_INPUT && kl_pool[i].focused) {
                if (c == 8) { if (kl_pool[i].ilen > 0) kl_pool[i].inbuf[--kl_pool[i].ilen] = 0; }
                else if (c >= 32 && kl_pool[i].ilen < 126) { kl_pool[i].inbuf[kl_pool[i].ilen++] = c; kl_pool[i].inbuf[kl_pool[i].ilen] = 0; }
                InvalidateRect(h, NULL, TRUE);
                return 0;
            }
        }
        break;
    }
    case WM_TIMER: {
        if (wp == 3) {
            if (kl_tick_fn) kl_tick_fn();
            if (kl_ui_on_rebuild) { kl_ui_on_rebuild(); InvalidateRect(h, NULL, TRUE); }
            return 0;
        }
        if (wp == 2) {
            int idx = kl_test_next_click();
            if (idx >= 0) {
                for (int i = 0; i < kl_np; i++) {
                    if (kl_pool[i].kind == KL_W_BUTTON) {
                        if (idx-- == 0) {
                            if (kl_pool[i].handler) kl_pool[i].handler();
                            if (kl_ui_on_rebuild) kl_ui_on_rebuild();
                            break;
                        }
                    }
                }
                InvalidateRect(h, NULL, TRUE);
                int ms = 400;
                const char* ac = getenv("KLOM_UI_AUTOCLOSE_MS");
                if (ac && atoi(ac) > 0) ms = atoi(ac) / 3;
                if (ms < 60) ms = 60;
                SetTimer(h, 2, (UINT)ms, NULL);
                return 0;
            }
            PostQuitMessage(0);
            return 0;
        }
        KillTimer(h, wp);
        PostQuitMessage(0);
        return 0;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(h, msg, wp, lp);
}

static char* kl_click_script = NULL;
static int kl_click_pos = 0;

static int kl_test_next_click(void) {
    if (!kl_click_script) return -1;    while (kl_click_script[kl_click_pos]) {
        char c = kl_click_script[kl_click_pos++];
        if (c == ',') continue;
        if (c >= '0' && c <= '9') return c - '0';
        return -1;
    }
    return -1;
}

static void kl_ui_init(const char* title_utf8) {
    kl_ui_load_usp();
    WNDCLASSW wc = {0};
    wc.lpfnWndProc = kl_wndproc;
    wc.hInstance = GetModuleHandleW(NULL);
    wc.lpszClassName = L"KolomWin";
    wc.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    wc.hbrBackground = (HBRUSH)GetStockObject(WHITE_BRUSH);
    RegisterClassW(&wc);
    int n = MultiByteToWideChar(CP_UTF8, 0, title_utf8, -1, NULL, 0);
    wchar_t* t = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    MultiByteToWideChar(CP_UTF8, 0, title_utf8, -1, t, n);
    DWORD style = WS_OVERLAPPEDWINDOW & ~(WS_MAXIMIZEBOX | WS_THICKFRAME);
    kl_hwnd = CreateWindowW(L"KolomWin", t, style, CW_USEDEFAULT, CW_USEDEFAULT,
        440, 620, NULL, NULL, GetModuleHandleW(NULL), NULL);
    free(t);
    const char* sc = getenv("KLOM_UI_SCRIPT_CLICKS");
    if (sc && *sc) {
        kl_click_script = _strdup(sc);
        SetTimer(kl_hwnd, 2, 300, NULL);
    } else {
        const char* ac = getenv("KLOM_UI_AUTOCLOSE_MS");
        if (ac && *ac && atoi(ac) > 0) SetTimer(kl_hwnd, 1, (UINT)atoi(ac), NULL);
    }
}

static void kl_ui_show_and_run(void) {
    ShowWindow(kl_hwnd, SW_SHOW);
    UpdateWindow(kl_hwnd);
    MSG m;
    while (GetMessageW(&m, NULL, 0, 0) > 0) {
        TranslateMessage(&m);
        DispatchMessageW(&m);
    }
}
#endif
"#
        .to_string()
    }

    fn ui_stubs(&self) -> &'static str {
        r#"
/* নন-Windows: UI স্টাব — প্রোগ্রাম কম্পাইল হয় কিন্তু GUI ছাড়া চলে */

typedef void (*kl_handler)(void);
typedef struct { int kind; } kl_wid_stub;

static void kl_ui_init(const char* title) { (void)title; }
static void kl_ui_show_and_run(void) {
    fprintf(stderr, "UI এই প্ল্যাটফর্মে সমর্থিত নয় — শুধু কনসোল আউটপুট\n");
}
static void kl_ui_text(const char* s) { printf("%s\n", s); }
static void kl_ui_button(const char* s, void (*h)(void)) { if (h) h(); }
static void kl_ui_input(void) { }
static void kl_ui_canvas(int w, int h) { (void)w; (void)h; }
static void kl_ui_image(const char* p) { (void)p; }
static void kl_ui_tick(int ms, void (*h)(void)) { (void)ms; if (h) h(); }
static void kl_ui_push(int kind) { (void)kind; }
static void kl_ui_pop(void) { }
static void kl_app_rebuild(void) { }
static void kl_build_ui(void) { }
static void kl_g_color(int r, int g, int b) { (void)r; (void)g; (void)b; }
static void kl_g_pixel(int x, int y) { (void)x; (void)y; }
static void kl_g_line(int x1, int y1, int x2, int y2) { (void)x1; (void)y1; (void)x2; (void)y2; }
static void kl_g_rect(int x, int y, int w, int h) { (void)x; (void)y; (void)w; (void)h; }
static void kl_g_fillrect(int x, int y, int w, int h) { (void)x; (void)y; (void)w; (void)h; }
static void kl_g_circle(int cx, int cy, int r) { (void)cx; (void)cy; (void)r; }
static void kl_g_fillcircle(int cx, int cy, int r) { (void)cx; (void)cy; (void)r; }
static void kl_g_text(int x, int y, const char* s) { printf("%s\n", s); (void)x; (void)y; }
static void kl_g_font(const char* n, int s) { (void)n; (void)s; }
"#
    }

    fn scan_ui(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Display(_) => {
                    self.has_ui = true;
                    self.has_display = true;
                }
                Stmt::Widget(w) => {
                    if matches!(w.kw.as_str(), "টেক্সট" | "বাটন" | "ইনপুট") {
                        self.has_ui = true;
                    }
                    if let Some(b) = &w.body {
                        self.scan_ui(&b.stmts);
                    }
                }
                Stmt::If(i) => {
                    self.scan_ui(&i.then.stmts);
                    if let Some(e) = &i.els {
                        match e {
                            ElseBranch::Block(b) => self.scan_ui(&b.stmts),
                            ElseBranch::If(inner) => {
                                let wrapped = [Stmt::If((**inner).clone())];
                                self.scan_ui(&wrapped);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn lower_expr(&mut self, e: &Expr, scope: &mut Vec<HashMap<String, Bnd>>) -> String {
        let t = self.ety(e);
        self.ensure(&t);
        match &e.kind {
            ExprKind::Lit(l) => match l {
                Lit::Int(v) => format!("{}", v),
                Lit::Float(v) => {
                    if v.fract() == 0.0 && v.is_finite() && v.abs() < 1e15 {
                        format!("{:.1}", v)
                    } else {
                        format!("{}", v)
                    }
                }
                Lit::Str(s) => format!("kl_str_lit({})", cstr(s)),
                Lit::Char(c) => format!("{}", *c as u32),
                Lit::Bool(b) => format!("{}", b),
                Lit::Null => "0".into(),
                Lit::Array(items) => {
                    let m = t.mg();
                    let a = self.tmp();
                    self.w(format!(
                        "kl_arr_{} {} = kl_arr_{}_new({});",
                        m,
                        a,
                        m,
                        items.len()
                    ));
                    for (i, it) in items.iter().enumerate() {
                        let v = self.lower_expr(it, scope);
                        let set = format!("kl_arr_{}_set({}, {}, {});", m, a, i, v);
                        self.w(set);
                    }
                    a
                }
            },
            ExprKind::Ident(id) => match id.name.as_str() {
                "লেখো" | "দৈর্ঘ্য" | "কপি" | "শেয়ার_করো" | "মান" | "বসাও" | "লেখায়" => "(0)".into(),
                _ => match Self::lookup(scope, &id.name).or_else(|| self.globals.get(&id.name).cloned()) {
                    Some(b) => b.c,
                    None => "(0)".into(),
                },
            },
            ExprKind::Qualified { module, name } => {
                match (module.name.as_str(), name.name.as_str()) {
                    ("গণিত", "পাই") => "3.141592653589793".into(),
                    ("গণিত", "ই") => "2.718281828459045".into(),
                    _ => "(0)".into(),
                }
            }
            ExprKind::Unary(op, inner) => {
                let v = self.lower_expr(inner, scope);
                match op {
                    UnaryOp::Neg => format!("(-{})", v),
                    UnaryOp::Not => format!("(!({}))", v),
                }
            }
            ExprKind::Binary(op, l, r) => {
                let lt = self.ety(l);
                let rt = self.ety(r);
                let lv = self.lower_expr(l, scope);
                let rv = self.lower_expr(r, scope);
                self.binop(*op, &lt, &lv, &rt, &rv)
            }
            ExprKind::Assign(target, rhs) => {
                let rt = self.ety(rhs);
                let rv = self.lower_expr(rhs, scope);
                if let Some(field) = &target.field {
                    // Struct field assignment: p.field = value
                    let base = match Self::lookup(scope, &target.base.name) {
                        Some(b) => b.c,
                        None => "(0)".into(),
                    };
                    self.w(format!(
                        "kl_map_{}_set(({}), kl_str_lit(\"{}\"), {});",
                        rt.mg(), base, field.name, rv
                    ));
                } else if target.idx.is_empty() {
                    if let Some(b) = Self::lookup(scope, &target.base.name) {
                        if rt.tracked() {
                            let u = rt.up_call(&rv);
                            self.w(format!("{};", u));
                            let d = rt.down_call(&b.c);
                            self.w(format!("{};", d));
                            self.w(format!("{} = {};", b.c, rv));
                        } else {
                            self.w(format!("{} = {};", b.c, rv));
                        }
                    }
                } else {
                    let bt = match Self::lookup(scope, &target.base.name) {
                        Some(b) => b.ty,
                        None => Ty::Null,
                    };
                    let mut cur_c = match Self::lookup(scope, &target.base.name) {
                        Some(b) => b.c,
                        None => "(0)".into(),
                    };
                    let mut cur_t = bt;
                    for (k, ie) in target.idx.iter().enumerate() {
                        let ix = self.lower_expr(ie, scope);
                        let et = match &cur_t {
                            Ty::Arr(i) => (**i).clone(),
                            _ => Ty::Null,
                        };
                        let last = k + 1 == target.idx.len();
                        let m = cur_t.mg();
                        if last {
                            let pv = self.tmp();
                            self.w(format!(
                                "{}* {} = kl_arr_{}_at({}, {});",
                                et.c(),
                                pv,
                                m,
                                cur_c,
                                ix
                            ));
                            if et.tracked() {
                                let u = et.up_call(&rv);
                                self.w(format!("{};", u));
                                let d = et.down_call(&format!("(*{})", pv));
                                self.w(format!("{};", d));
                            }
                            self.w(format!("*{} = {};", pv, rv));
                        } else {
                            let pv = self.tmp();
                            self.w(format!(
                                "{}* {} = kl_arr_{}_at({}, {});",
                                et.c(),
                                pv,
                                m,
                                cur_c,
                                ix
                            ));
                            cur_c = format!("(*{})", pv);
                            cur_t = et;
                        }
                    }
                }
                "(0)".into()
            }
            ExprKind::FieldAssign(base, field, rhs) => {
                let rt = self.ety(rhs);
                let rv = self.lower_expr(rhs, scope);
                let bt = match Self::lookup(scope, &base.name) {
                    Some(b) => b.c,
                    None => "(0)".into(),
                };
                self.w(format!(
                    "kl_map_{}_set(({}), kl_str_lit(\"{}\"), {});",
                    "ms_n", bt, field.name, rv
                ));
                "(0)".into()
            }
            ExprKind::Postfix(base, sfx) => {
                let mut callable: Option<String> = None;
                let (mut cur, mut cur_t) = match &base.kind {
                    ExprKind::Ident(id) => {
                        if matches!(
                            id.name.as_str(),
                            "লেখো" | "দৈর্ঘ্য" | "কপি" | "শেয়ার_করো" | "মান" | "বসাও" | "লেখায়"
                        ) || self.funcs.contains_key(&id.name)
                        {
                            callable = Some(id.name.clone());
                            ("(0)".into(), Ty::Null)
                        } else {
                            match Self::lookup(scope, &id.name) {
                                Some(b) => (b.c, b.ty),
                                None => ("(0)".into(), Ty::Null),
                            }
                        }
                    }
                    ExprKind::Qualified { module, name } => {
                        match (module.name.as_str(), name.name.as_str()) {
                            ("গণিত", "পাই") | ("গণিত", "ই") => {
                                let v = self.lower_expr(base, scope);
                                (v, Ty::Dec)
                            }
                            _ => {
                                callable = Some(format!("{}::{}", module.name, name.name));
                                ("(0)".into(), Ty::Null)
                            }
                        }
                    }
                    _ => {
                        let bt = self.ety(base);
                        let v = self.lower_expr(base, scope);
                        if bt.tracked() {
                            let tv = self.tmp();
                            self.w(format!("{} {};", bt.c(), tv));
                            self.w(format!("{} = {};", tv, v));
                            let u = bt.up_call(&tv);
                            self.w(format!("{};", u));
                            (tv, bt)
                        } else {
                            (v, bt)
                        }
                    }
                };
                for sf in sfx {
                    match sf {
                        Suffix::Call(args, _) => {
                            let name = callable.take().unwrap_or_default();
                            cur = self.call(&name, args, scope, &mut cur_t);
                        }
                        Suffix::Field(fname) => {
                            callable = None;
                            let m = cur_t.mg();
                            let tv = self.tmp();
                            self.ensure(&cur_t);
                            self.w(format!(
                                "kl_str {t}_k = kl_str_lit(\"{f}\");",
                                t = tv,
                                f = fname.name
                            ));
                            self.w(format!(
                                "{ct}* {tv2} = kl_map_{m}_get_or_null({cur}, {t}_k);",
                                ct = cur_t.c(),
                                tv2 = tv,
                                t = tv,
                                m = m,
                                cur = cur
                            ));
                            cur_t = Ty::Null;
                            cur = format!("(*{})", tv);
                        }
                        Suffix::Index(ix, ipos) => {
                            callable = None;
                            let it = self.lower_expr(ix, scope);
                            let et = match &cur_t {
                                Ty::Arr(i) => (**i).clone(),
                                Ty::Txt => Ty::Ch,
                                _ => Ty::Null,
                            };
                            self.ensure(&et);
                            let m = cur_t.mg();
                            if cur_t == Ty::Txt {
                                // string indexing → char
                                let gv = self.tmp();
                                self.w(format!("uint32_t {} = kl_str_char_at({}, {});", gv, cur, it));
                                cur = gv;
                            } else {
                                let get = format!("kl_arr_{}_get({}, {})", m, cur, it);
                                if et.tracked() {
                                    let gv = self.tmp();
                                    self.w(format!("{} {};", et.c(), gv));
                                    self.w(format!("{} = {};", gv, get));
                                    cur = gv;
                                } else {
                                    cur = get;
                                }
                            }
                            cur_t = et;
                        }
                    }
                }
                cur
            }
        }
    }

    fn call(
        &mut self,
        name: &str,
        args: &[Expr],
        scope: &mut Vec<HashMap<String, Bnd>>,
        ret_out: &mut Ty,
    ) -> String {
        if name == "ম্যাপ_তৈরি" {
            let mt = self.ety(&args.first().cloned().unwrap_or_else(|| Expr {
                kind: ExprKind::Lit(Lit::Null),
                pos: Pos { line: 0, col: 0 },
            }));
            let m = match &mt {
                Ty::Map(_, _) => mt.mg(),
                _ => "ms_n".to_string(),
            };
            self.ensure(&mt);
            *ret_out = mt.clone();
            return format!("kl_map_{}_new()", m);
        }
        if name == "চাবি_গুলো" || name == "আছে_কি" || name == "চাবি_মুছো" {
            for a in args {
                let _ = self.lower_expr(a, scope);
            }
            *ret_out = match name {
                "চাবি_গুলো" => Ty::Arr(Box::new(Ty::Txt)),
                "আছে_কি" => Ty::Bool,
                _ => Ty::Null,
            };
            return "(0)".into();
        }
        if let Some((module, item)) = name.split_once("::") {
            return self.call_stdlib(module, item, args, scope, ret_out);
        }
        match name {
            "লেখো" => {
                for a in args {
                    let at = self.ety(a);
                    self.ensure_print(&at);
                    let v = self.lower_expr(a, scope);
                    self.w(format!("kl_print_{}({});", at.mg(), v));
                }
                self.w("fputc('\\n', stdout);");
                *ret_out = Ty::Null;
                "(0)".into()
            }
            "দৈর্ঘ্য" => {
                let v = self.lower_expr(&args[0], scope);
                *ret_out = Ty::Num;
                format!("((int64_t)(({}).len))", v)
            }
            "কপি" => {
                let at = self.ety(&args[0]);
                self.ensure_deep(&at);
                let v = self.lower_expr(&args[0], scope);
                *ret_out = at.clone();
                if at.tracked() {
                    format!("kl_deep_{}({})", at.mg(), v)
                } else {
                    v
                }
            }
            "শেয়ার_করো" => {
                let at = self.ety(&args[0]);
                let shared = Ty::Shared(Box::new(at.clone()));
                self.ensure(&shared);
                let v = self.lower_expr(&args[0], scope);
                *ret_out = shared.clone();
                format!("kl_sh_make_{}({})", shared.mg(), v)
            }
            "মান" => {
                let at = self.ety(&args[0]);
                let inner = match &at {
                    Ty::Shared(i) => (**i).clone(),
                    _ => Ty::Null,
                };
                self.ensure(&inner);
                let v = self.lower_expr(&args[0], scope);
                *ret_out = inner.clone();
                if inner.tracked() {
                    let tv = self.tmp();
                    self.w(format!("{} {};", inner.c(), tv));
                    self.w(format!("if ({}) {} = ({})->v;", v, tv, v));
                    let u = inner.up_call(&tv);
                    self.w(format!("{};", u));
                    tv
                } else {
                    format!("(({}) ? ({})->v : {})", v, v, inner.zero())
                }
            }
            "বসাও" => {
                let cell_ty = self.ety(&args[0]);
                let inner = match &cell_ty {
                    Ty::Shared(i) => (**i).clone(),
                    _ => Ty::Null,
                };
                self.ensure(&inner);
                let cell = self.lower_expr(&args[0], scope);
                let nv = self.lower_expr(&args[1], scope);
                if inner.tracked() {
                    let u = inner.up_call(&nv);
                    self.w(format!("{};", u));
                    let d = inner.down_call(&format!("({})->v", cell));
                    self.w(format!("{};", d));
                }
                self.w(format!("if ({}) ({})->v = {};", cell, cell, nv));
                *ret_out = Ty::Null;
                "(0)".into()
            }
            "লেখায়" => {
                let at = self.ety(&args[0]);
                self.ensure_tostr(&at);
                let v = self.lower_expr(&args[0], scope);
                *ret_out = Ty::Txt;
                format!("kl_tostr_{}({})", at.mg(), v)
            }
            // Reads a line from stdin — a builtin, not a `ফাইল` member.
            "পড়ো_লাইন" => {
                self.ensure_io_runtime();
                *ret_out = Ty::Txt;
                "kl_io_readline()".into()
            }
            user => {
                let meta = self.funcs.get(user).cloned();
                match meta {
                    Some(m) => {
                        let mut parts = Vec::new();
                        for a in args {
                            parts.push(self.lower_expr(a, scope));
                        }
                        *ret_out = m.ret.clone();
                        format!("{}({})", m.cname, parts.join(", "))
                    }
                    None => {
                        *ret_out = Ty::Null;
                        "(0)".into()
                    }
                }
            }
        }
    }

    fn binop(&mut self, op: BinOp, lt: &Ty, lv: &str, rt: &Ty, rv: &str) -> String {
        use BinOp::*;
        let both_num = matches!(lt, Ty::Num | Ty::Dec) && matches!(rt, Ty::Num | Ty::Dec);
        match op {
            Add => {
                if both_num && *lt == Ty::Num && *rt == Ty::Num {
                    format!("kl_iadd({}, {})", lv, rv)
                } else if both_num {
                    format!("((double)({}) + (double)({}))", lv, rv)
                } else if *lt == Ty::Txt && *rt == Ty::Txt {
                    format!("kl_str_concat({}, {})", lv, rv)
                } else if let (Ty::Arr(_), Ty::Arr(_)) = (lt, rt) {
                    format!("kl_arr_{}_concat({}, {})", lt.mg(), lv, rv)
                } else {
                    "(0)".into()
                }
            }
            Sub | Mul | Div | Mod => {
                if !both_num {
                    return "(0)".into();
                }
                if *lt == Ty::Num && *rt == Ty::Num {
                    match op {
                        Sub => format!("kl_isub({}, {})", lv, rv),
                        Mul => format!("kl_imul({}, {})", lv, rv),
                        Div => format!("kl_idiv({}, {})", lv, rv),
                        Mod => format!("kl_imod({}, {})", lv, rv),
                        _ => unreachable!(),
                    }
                } else {
                    match op {
                        Sub => format!("((double)({}) - (double)({}))", lv, rv),
                        Mul => format!("((double)({}) * (double)({}))", lv, rv),
                        Div => format!("kl_ddiv((double)({}), (double)({}))", lv, rv),
                        Mod => format!("kl_dmod((double)({}), (double)({}))", lv, rv),
                        _ => unreachable!(),
                    }
                }
            }
            Eq | Neq => {
                let eq = if *lt == Ty::Txt && *rt == Ty::Txt {
                    format!("kl_str_eq({}, {})", lv, rv)
                } else if let (Ty::Arr(_), Ty::Arr(_)) = (lt, rt) {
                    format!("({}.data == {}.data)", lv, rv)
                } else if let (Ty::Shared(_), Ty::Shared(_)) = (lt, rt) {
                    format!("({} == {})", lv, rv)
                } else if matches!(lt, Ty::Null) && matches!(rt, Ty::Null) {
                    "true".into()
                } else {
                    format!("((double)({}) == (double)({}))", lv, rv)
                };
                if op == Eq {
                    eq
                } else {
                    format!("(!({}))", eq)
                }
            }
            Lt | Gt | Le | Ge => {
                let sym = match op {
                    Lt => "<",
                    Gt => ">",
                    Le => "<=",
                    Ge => ">=",
                    _ => unreachable!(),
                };
                if *lt == Ty::Num && *rt == Ty::Num {
                    format!("(({}) {} ({}))", lv, sym, rv)
                } else if *lt == Ty::Txt && *rt == Ty::Txt {
                    format!("(strcmp((const char*){}.data, (const char*){}.data) {} 0)", lv, sym, rv)
                } else {
                    format!("((double)({}) {} (double)({}))", lv, sym, rv)
                }
            }
            And => format!("(({}) && ({}))", lv, rv),
            Or => format!("(({}) || ({}))", lv, rv),
        }
    }
}

fn str_runtime() -> &'static str {
    r#"

static uint32_t kl_str_char_at(kl_str s, int64_t i) {
    if (!s.data) return 0;
    int64_t ci = 0;
    const uint8_t* p = s.data;
    while (*p) {
        if ((*p & 0xC0) != 0x80) {
            if (ci == i) {
                uint32_t cp = 0;
                if ((*p & 0x80) == 0) cp = *p;
                else if ((*p & 0xE0) == 0xC0) { cp = *p & 0x1F; cp = (cp << 6) | (p[1] & 0x3F); }
                else if ((*p & 0xF0) == 0xE0) { cp = *p & 0x0F; cp = (cp << 6) | (p[1] & 0x3F); cp = (cp << 6) | (p[2] & 0x3F); }
                else { cp = *p & 0x07; cp = (cp << 6) | (p[1] & 0x3F); cp = (cp << 6) | (p[2] & 0x3F); cp = (cp << 6) | (p[3] & 0x3F); }
                return cp;
            }
            ci++;
        }
        p++;
    }
    return 0;
}

static kl_str kl_str_upper(kl_str s) {
    kl_str r = kl_str_alloc(s.data ? (int64_t)strlen((const char*)s.data) : 0);
    if (s.data) {
        int64_t n = (int64_t)strlen((const char*)s.data);
        for (int64_t i = 0; i < n; i++) r.data[i] = (s.data[i] >= 'a' && s.data[i] <= 'z') ? s.data[i] - 32 : s.data[i];
    }
    return r;
}

static kl_str kl_str_lower(kl_str s) {
    kl_str r = kl_str_alloc(s.data ? (int64_t)strlen((const char*)s.data) : 0);
    if (s.data) {
        int64_t n = (int64_t)strlen((const char*)s.data);
        for (int64_t i = 0; i < n; i++) r.data[i] = (s.data[i] >= 'A' && s.data[i] <= 'Z') ? s.data[i] + 32 : s.data[i];
    }
    return r;
}

static kl_str kl_str_trim(kl_str s) {
    if (!s.data) return kl_str_lit("");
    const char* p = (const char*)s.data;
    while (*p == ' ' || *p == '\t' || *p == '\r' || *p == '\n') p++;
    const char* e = p + strlen(p);
    while (e > p && (e[-1] == ' ' || e[-1] == '\t' || e[-1] == '\r' || e[-1] == '\n')) e--;
    int64_t n = e - p;
    kl_str r = kl_str_alloc(n);
    memcpy(r.data, p, (size_t)n);
    r.len = kl_cpcount(r.data, n);
    return r;
}

static kl_arr_as kl_str_split(kl_str s, kl_str sep) {
    kl_arr_as out = kl_arr_as_new(4);
    int64_t cnt = 0;
    size_t sepl = sep.data ? strlen((const char*)sep.data) : 0;
    if (!sepl || !s.data) { out.data[0] = kl_str_copy(s); out.len = 1; return out; }
    const char* start = (const char*)s.data;
    const char* p = start;
    while (*p) {
        if (strncmp(p, (const char*)sep.data, sepl) == 0) {
            kl_str part = kl_str_alloc(p - start);
            memcpy(part.data, start, (size_t)(p - start));
            part.len = kl_cpcount(part.data, p - start);
            if (cnt >= out.len) { out = kl_arr_as_concat(out, kl_arr_as_new(4)); }
            if (cnt < out.len) out.data[cnt++] = part;
            p += sepl; start = p;
        } else p++;
    }
    kl_str last = kl_str_alloc(strlen(start));
    memcpy(last.data, start, strlen(start));
    last.len = kl_cpcount(last.data, strlen(start));
    if (cnt < out.len) out.data[cnt++] = last;
    out.len = cnt;
    return out;
}

static kl_str kl_str_join(kl_arr_as arr, kl_str sep) {
    kl_str r = kl_str_lit("");
    for (int64_t i = 0; i < arr.len; i++) {
        if (i) r = kl_str_concat(r, sep);
        r = kl_str_concat(r, arr.data[i]);
    }
    return r;
}

static kl_str kl_str_replace(kl_str s, kl_str from, kl_str to) {
    if (!from.data || !strlen((const char*)from.data) || !s.data) return kl_str_copy(s);
    kl_str r = kl_str_lit("");
    const char* p = (const char*)s.data;
    size_t fl = strlen((const char*)from.data);
    while (*p) {
        if (strncmp(p, (const char*)from.data, fl) == 0) { r = kl_str_concat(r, to); p += fl; }
        else { char one[2] = { *p, 0 }; r = kl_str_concat(r, kl_str_lit(one)); p++; }
    }
    return r;
}

static int64_t kl_str_find(kl_str s, kl_str sub) {
    if (!s.data || !sub.data) return -1;
    const char* hit = strstr((const char*)s.data, (const char*)sub.data);
    if (!hit) return -1;
    return kl_cpcount((const uint8_t*)s.data, hit - (const char*)s.data);
}

static kl_str kl_str_slice(kl_str s, int64_t st, int64_t ln) {
    if (!s.data) return kl_str_lit("");
    int64_t total = kl_cpcount(s.data, (int64_t)strlen((const char*)s.data));
    if (st < 0) { st = 0; }
    if (st > total) st = total;
    if (ln < 0) ln = 0;
    if (st + ln > total) ln = total - st;
    int64_t bi = 0, ci = 0, bo = 0;
    const uint8_t* p = s.data;
    while (p[bi]) {
        if (ci == st) bo = bi;
        if (ci == st + ln) break;
        if ((p[bi] & 0xC0) != 0x80) ci++;
        bi++;
    }
    if (ci <= st) return kl_str_lit("");
    kl_str r = kl_str_alloc(bo - ((st==0)?0:bo));
    int64_t start_byte = 0;
    ci = 0; bi = 0;
    while (p[bi]) { if (ci == st) { start_byte = bi; } if (ci == st + ln) break; if ((p[bi] & 0xC0) != 0x80) ci++; bi++; }
    int64_t nbytes = bi - start_byte;
    free(r.data);
    r = kl_str_alloc(nbytes);
    memcpy(r.data, p + start_byte, (size_t)nbytes);
    r.len = ln;
    return r;
}

static bool kl_str_starts(kl_str s, kl_str p) {
    if (!s.data || !p.data) return false;
    return strncmp((const char*)s.data, (const char*)p.data, strlen((const char*)p.data)) == 0;
}

static bool kl_str_ends(kl_str s, kl_str p) {
    if (!s.data || !p.data) return false;
    size_t sl = strlen((const char*)s.data), pl = strlen((const char*)p.data);
    if (pl > sl) return false;
    return strcmp((const char*)s.data + sl - pl, (const char*)p.data) == 0;
}
"#
}

fn io_runtime() -> &'static str {
    r#"
static kl_str kl_io_readline(void) {
    char buf[4096];
    if (!fgets(buf, sizeof(buf), stdin)) return kl_str_lit("");
    int64_t n = (int64_t)strlen(buf);
    while (n > 0 && (buf[n-1] == '\n' || buf[n-1] == '\r')) buf[--n] = 0;
    kl_str r = kl_str_alloc(n);
    memcpy(r.data, buf, (size_t)n);
    r.len = kl_cpcount(r.data, n);
    return r;
}

static void kl_io_fail(const char* path, const char* what) {
    char b[512];
    snprintf(b, sizeof(b), "%s '%s' \340\246\255\340\246\276\340\246\207\340\246\262", what, path);
    kl_panic(b);
}

static kl_str kl_io_read_file(kl_str path) {
    FILE* f = fopen((const char*)path.data, "rb");
    if (!f) kl_io_fail("ফাইল পড়া যায়নি", (const char*)path.data);
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    kl_str r = kl_str_alloc(n);
    fread(r.data, 1, (size_t)n, f);
    fclose(f);
    r.len = kl_cpcount(r.data, n);
    return r;
}

static void kl_io_write_file(kl_str path, kl_str content) {
    FILE* f = fopen((const char*)path.data, "wb");
    if (!f) kl_io_fail("ফাইল লেখা যায়নি", (const char*)path.data);
    fwrite(content.data, 1, content.data ? strlen((const char*)content.data) : 0, f);
    fclose(f);
}

static void kl_io_append_file(kl_str path, kl_str content) {
    FILE* f = fopen((const char*)path.data, "ab");
    if (!f) kl_io_fail("ফাইলে এপেন্ড করা যায়নি", (const char*)path.data);
    fwrite(content.data, 1, content.data ? strlen((const char*)content.data) : 0, f);
    fclose(f);
}

static kl_arr_as kl_io_read_lines(kl_str path) {
    kl_str content = kl_io_read_file(path);
    kl_arr_as lines = kl_str_split(content, kl_str_lit("\n"));
    for (int64_t i = 0; i < lines.len; i++) {
        kl_str* s = &lines.data[i];
        int64_t n = s->data ? (int64_t)strlen((const char*)s->data) : 0;
        if (n > 0 && s->data[n - 1] == '\r') {
            s->data[n - 1] = 0;
            s->len = kl_cpcount(s->data, n - 1);
        }
    }
    return lines;
}
"#
}

fn rand_runtime() -> &'static str {
    r#"
static uint64_t kl_rng_state = 0x2545F4914F6CDD1DULL;

static uint64_t kl_rand_next(void) {
    kl_rng_state = kl_rng_state * 6364136223846793005ULL + 1442695040888963407ULL;
    return kl_rng_state >> 33;
}

static void kl_rand_seed(int64_t s) {
    kl_rng_state = (uint64_t)s ^ 0x2545F4914F6CDD1DULL;
}

static int64_t kl_rand_int(void) {
    return (int64_t)(kl_rand_next() & 0x7FFFFFFFFFFFFFFFULL);
}

static int64_t kl_rand_range(int64_t lo, int64_t hi) {
    if (lo > hi) kl_panic("'মধ্যে'-তে নিম্নসীমা উচ্চসীমার বেশি");
    uint64_t span = (uint64_t)(hi - lo + 1);
    return lo + (int64_t)(kl_rand_next() % span);
}

static double kl_rand_float(void) {
    return (double)(kl_rand_next() % 1000000) / 1000000.0;
}
"#
}

fn fs_runtime() -> &'static str {
    r#"
#include <sys/stat.h>
#include <sys/types.h>
#include <errno.h>
#ifdef _WIN32
#include <windows.h>
#include <direct.h>
#else
#include <unistd.h>
#endif

static void kl_fs_fail(const char* what, const char* path) {
    char b[512];
    snprintf(b, sizeof(b), "%s '%s'", what, path);
    kl_panic(b);
}

static bool kl_fs_file_exists(kl_str path) {
    if (!path.data) return false;
    struct stat st;
    return stat((const char*)path.data, &st) == 0 && (st.st_mode & S_IFREG);
}

static bool kl_fs_dir_exists(kl_str path) {
    if (!path.data) return false;
    struct stat st;
    return stat((const char*)path.data, &st) == 0 && (st.st_mode & S_IFDIR);
}

static void kl_fs_mkdir(kl_str path) {
#ifdef _WIN32
    if (_mkdir((const char*)path.data) != 0 && errno != EEXIST)
#else
    if (mkdir((const char*)path.data, 0777) != 0 && errno != EEXIST)
#endif
        kl_fs_fail("ডিরেক্টরি তৈরি ব্যর্থ", (const char*)path.data);
}

static void kl_fs_remove(kl_str path) {
    if (remove((const char*)path.data) != 0)
        kl_fs_fail("মুছতে ব্যর্থ", (const char*)path.data);
}

static kl_arr_as kl_fs_list(kl_str dir) {
    kl_arr_as out = kl_arr_as_new(16);
    out.len = 0;
#ifdef _WIN32
    WIN32_FIND_DATAA fd;
    char pattern[1024];
    snprintf(pattern, sizeof(pattern), "%s\\*", (const char*)dir.data);
    HANDLE h = FindFirstFileA(pattern, &fd);
    if (h == INVALID_HANDLE_VALUE) kl_fs_fail("তালিকা পড়া যায়নি", (const char*)dir.data);
    int64_t cnt = 0;
    do {
        const char* name = fd.cFileName;
        if (strcmp(name, ".") == 0 || strcmp(name, "..") == 0) continue;
        if (cnt >= 16) break;
        int64_t n = (int64_t)strlen(name);
        kl_str s = kl_str_alloc(n);
        memcpy(s.data, name, (size_t)n);
        s.len = kl_cpcount(s.data, n);
        out.data[cnt++] = s;
    } while (FindNextFileA(h, &fd));
    FindClose(h);
    out.len = cnt;
#else
    DIR* d = opendir((const char*)dir.data);
    if (!d) kl_fs_fail("তালিকা পড়া যায়নি", (const char*)dir.data);
    struct dirent* ent;
    while ((ent = readdir(d)) != NULL) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0) continue;
        if (out.len >= 4096) break;
        int64_t n = (int64_t)strlen(ent->d_name);
        kl_str s = kl_str_alloc(n);
        memcpy(s.data, ent->d_name, (size_t)n);
        s.len = kl_cpcount(s.data, n);
        out.data[out.len++] = s;
    }
    closedir(d);
#endif
    return out;
}

static void kl_fs_copy(kl_str from, kl_str to) {
    FILE* a = fopen((const char*)from.data, "rb");
    if (!a) kl_fs_fail("কপি ব্যর্থ (উৎস)", (const char*)from.data);
    FILE* b = fopen((const char*)to.data, "wb");
    if (!b) { fclose(a); kl_fs_fail("কপি ব্যর্থ (লক্ষ্য)", (const char*)to.data); }
    char buf[8192];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), a)) > 0) fwrite(buf, 1, n, b);
    fclose(a); fclose(b);
}

static void kl_fs_move(kl_str from, kl_str to) {
    if (rename((const char*)from.data, (const char*)to.data) != 0)
        kl_fs_fail("সরানো ব্যর্থ", (const char*)from.data);
}

static int64_t kl_fs_size(kl_str path) {
    struct stat st;
    if (stat((const char*)path.data, &st) != 0) kl_fs_fail("আকার পড়া যায়নি", (const char*)path.data);
    return (int64_t)st.st_size;
}

static kl_str kl_fs_cwd(void) {
    char buf[1024];
#ifdef _WIN32
    if (!_getcwd(buf, sizeof(buf))) kl_panic("বর্তমান ডিরেক্টরি পড়া যায়নি");
#else
    if (!getcwd(buf, sizeof(buf))) kl_panic("বর্তমান ডিরেক্টরি পড়া যায়নি");
#endif
    return kl_str_lit(buf);
}
"#
}

/// পাথ — লেক্সিক্যাল পাথ ম্যানিপুলেশন। `fs_runtime()`-এর মতো ডিস্ক-নির্ভর নয়
/// (শুধু `পরম_পাথ`-এর জন্য cwd লাগে, `ensure_fs_runtime()` টেনে না এনে
/// নিজস্ব ছোট cwd-লুকআপ রাখা হয়েছে যাতে `পাথ` মডিউল স্বনির্ভর থাকে)।
fn path_runtime() -> &'static str {
    r#"
#ifdef _WIN32
#include <direct.h>
#else
#include <unistd.h>
#endif

static bool kl_path_is_sep(char c) { return c == '/' || c == '\\'; }

static kl_str kl_path_join(kl_str a, kl_str b) {
    size_t na = a.data ? strlen((const char*)a.data) : 0;
    if (na == 0) return kl_str_copy(b);
    size_t nb = b.data ? strlen((const char*)b.data) : 0;
    if (nb == 0) return kl_str_copy(a);
    if (kl_path_is_sep((char)a.data[na - 1])) return kl_str_concat(a, b);
#ifdef _WIN32
    kl_str sep = kl_str_lit("\\");
#else
    kl_str sep = kl_str_lit("/");
#endif
    kl_str r = kl_str_concat(a, sep);
    return kl_str_concat(r, b);
}

static kl_str kl_path_basename(kl_str path) {
    if (!path.data) return kl_str_lit("");
    const char* s = (const char*)path.data;
    size_t n = strlen(s);
    while (n > 0 && kl_path_is_sep(s[n - 1])) n--;
    size_t end = n, start = end;
    while (start > 0 && !kl_path_is_sep(s[start - 1])) start--;
    if (start == end) return kl_str_lit("");
    kl_str r = kl_str_alloc(end - start);
    memcpy(r.data, s + start, end - start);
    r.len = kl_cpcount(r.data, end - start);
    return r;
}

static kl_str kl_path_dirname(kl_str path) {
    if (!path.data) return kl_str_lit("");
    const char* s = (const char*)path.data;
    size_t n = strlen(s);
    while (n > 0 && kl_path_is_sep(s[n - 1])) n--;
    size_t end = n;
    while (end > 0 && !kl_path_is_sep(s[end - 1])) end--;
    while (end > 0 && kl_path_is_sep(s[end - 1])) end--;
    kl_str r = kl_str_alloc(end);
    if (end) memcpy(r.data, s, end);
    r.len = kl_cpcount(r.data, end);
    return r;
}

static kl_str kl_path_extension(kl_str path) {
    if (!path.data) return kl_str_lit("");
    const char* s = (const char*)path.data;
    size_t n = strlen(s);
    size_t end = n;
    while (end > 0 && kl_path_is_sep(s[end - 1])) end--;
    size_t base_start = end;
    while (base_start > 0 && !kl_path_is_sep(s[base_start - 1])) base_start--;
    size_t dot = end;
    for (size_t i = end; i > base_start; i--) {
        if (s[i - 1] == '.') { dot = i - 1; break; }
    }
    // dot == base_start মানে একটা dotfile (".gitignore")-এর নিজের leading dot
    // — সেটাকে extension হিসেবে গোনা হয় না।
    if (dot == end || dot == base_start) return kl_str_lit("");
    size_t ext_start = dot + 1;
    kl_str r = kl_str_alloc(end - ext_start);
    if (end > ext_start) memcpy(r.data, s + ext_start, end - ext_start);
    r.len = kl_cpcount(r.data, end - ext_start);
    return r;
}

static kl_str kl_path_abs(kl_str path) {
    const char* s = path.data ? (const char*)path.data : "";
#ifdef _WIN32
    bool is_abs = (s[0] && s[1] == ':' && (s[2] == '\\' || s[2] == '/')) || s[0] == '\\' || s[0] == '/';
#else
    bool is_abs = s[0] == '/';
#endif
    if (is_abs) return kl_str_copy(path);
    char buf[1024];
#ifdef _WIN32
    if (!_getcwd(buf, sizeof(buf))) kl_panic("বর্তমান ডিরেক্টরি পড়া যায়নি");
#else
    if (!getcwd(buf, sizeof(buf))) kl_panic("বর্তমান ডিরেক্টরি পড়া যায়নি");
#endif
    kl_str cwd = kl_str_lit(buf);
    return kl_path_join(cwd, path);
}
"#
}

fn json_runtime() -> &'static str {
    r#"
static const char* kl_json_ws(const char* p) { while (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') p++; return p; }

static const char* kl_json_value(const char* p);

static const char* kl_json_string(const char* p) {
    if (*p != '"') return NULL;
    p++;
    while (*p && *p != '"') {
        if (*p == '\\' && p[1]) p += 2; else p++;
    }
    return *p == '"' ? p + 1 : NULL;
}

static const char* kl_json_number(const char* p) {
    if (*p == '-') p++;
    if (!(*p >= '0' && *p <= '9')) return NULL;
    while (*p >= '0' && *p <= '9') p++;
    if (*p == '.') { p++; while (*p >= '0' && *p <= '9') p++; }
    if (*p == 'e' || *p == 'E') { p++; if (*p == '+' || *p == '-') p++; while (*p >= '0' && *p <= '9') p++; }
    return p;
}

static const char* kl_json_value(const char* p) {
    p = kl_json_ws(p);
    if (*p == '{') {
        p++;
        p = kl_json_ws(p);
        if (*p == '}') return p + 1;
        for (;;) {
            p = kl_json_ws(p);
            p = kl_json_string(p);
            if (!p) return NULL;
            p = kl_json_ws(p);
            if (*p != ':') return NULL;
            p = kl_json_value(p + 1);
            if (!p) return NULL;
            p = kl_json_ws(p);
            if (*p == ',') { p++; continue; }
            if (*p == '}') return p + 1;
            return NULL;
        }
    }
    if (*p == '[') {
        p++;
        p = kl_json_ws(p);
        if (*p == ']') return p + 1;
        for (;;) {
            p = kl_json_value(p);
            if (!p) return NULL;
            p = kl_json_ws(p);
            if (*p == ',') { p++; continue; }
            if (*p == ']') return p + 1;
            return NULL;
        }
    }
    if (*p == '"') return kl_json_string(p);
    if (strncmp(p, "true", 4) == 0) return p + 4;
    if (strncmp(p, "false", 5) == 0) return p + 5;
    if (strncmp(p, "null", 4) == 0) return p + 4;
    return kl_json_number(p);
}

static bool kl_json_valid(kl_str text) {
    if (!text.data) return false;
    const char* end = kl_json_value((const char*)text.data);
    if (!end) return false;
    end = kl_json_ws(end);
    return *end == 0;
}

static kl_str kl_json_escape(kl_str s) {
    kl_str r = kl_str_lit("\"");
    const char* p = s.data ? (const char*)s.data : "";
    for (; *p; p++) {
        unsigned char c = (unsigned char)*p;
        char one[8];
        switch (c) {
        case '"': r = kl_str_concat(r, kl_str_lit("\\\"")); break;
        case '\\': r = kl_str_concat(r, kl_str_lit("\\\\")); break;
        case '\n': r = kl_str_concat(r, kl_str_lit("\\n")); break;
        case '\r': r = kl_str_concat(r, kl_str_lit("\\r")); break;
        case '\t': r = kl_str_concat(r, kl_str_lit("\\t")); break;
        default:
            if (c < 0x20) { snprintf(one, sizeof(one), "\\u%04x", c); r = kl_str_concat(r, kl_str_lit(one)); }
            else { one[0] = (char)c; one[1] = 0; r = kl_str_concat(r, kl_str_lit(one)); }
        }
    }
    return kl_str_concat(r, kl_str_lit("\""));
}

static const char* kl_json_after_key(const char* text, const char* key) {
    char pat[256];
    snprintf(pat, sizeof(pat), "\"%s\"", key);
    const char* hit = strstr(text, pat);
    if (!hit) return NULL;
    hit += strlen(pat);
    hit = kl_json_ws(hit);
    if (*hit != ':') return NULL;
    return kl_json_ws(hit + 1);
}

static kl_str kl_json_get_string(kl_str text, kl_str key) {
    if (!text.data || !key.data) return kl_str_lit("");
    const char* p = kl_json_after_key((const char*)text.data, (const char*)key.data);
    if (!p || *p != '"') return kl_str_lit("");
    p++;
    kl_str r = kl_str_lit("");
    while (*p && *p != '"') {
        if (*p == '\\' && p[1]) {
            p++;
            char one[2] = { *p, 0 };
            switch (*p) {
            case 'n': r = kl_str_concat(r, kl_str_lit("\n")); break;
            case 't': r = kl_str_concat(r, kl_str_lit("\t")); break;
            case 'r': r = kl_str_concat(r, kl_str_lit("\r")); break;
            default: r = kl_str_concat(r, kl_str_lit(one));
            }
            p++;
        } else {
            char one[2] = { *p, 0 };
            r = kl_str_concat(r, kl_str_lit(one));
            p++;
        }
    }
    return r;
}

static int64_t kl_json_get_int(kl_str text, kl_str key) {
    if (!text.data || !key.data) return 0;
    const char* p = kl_json_after_key((const char*)text.data, (const char*)key.data);
    if (!p) return 0;
    int neg = 0;
    if (*p == '-') { neg = 1; p++; }
    if (!(*p >= '0' && *p <= '9')) return 0;
    int64_t v = 0;
    while (*p >= '0' && *p <= '9') { v = v * 10 + (*p - '0'); p++; }
    return neg ? -v : v;
}
"#
}

fn graphics_runtime() -> &'static str {
    r#"
#ifdef _WIN32
#include <windows.h>
#endif
enum { KL_C_COLOR = 0, KL_C_PIXEL, KL_C_LINE, KL_C_RECT, KL_C_FILLRECT,
       KL_C_CIRCLE, KL_C_FILLCIRCLE, KL_C_TEXT, KL_C_FONT };

typedef struct { int op; int a, b, c, d; wchar_t* txt; } kl_gcmd;

static kl_gcmd kl_gbuf[4096];
static int kl_ng = 0;
static COLORREF kl_g_color_cur = RGB(0, 0, 0);

static void kl_g_push(int op, int a, int b, int c, int d) {
    if (kl_ng >= 4096) return;
    kl_gbuf[kl_ng].op = op;
    kl_gbuf[kl_ng].a = a; kl_gbuf[kl_ng].b = b;
    kl_gbuf[kl_ng].c = c; kl_gbuf[kl_ng].d = d;
    kl_ng++;
}

static void kl_g_color(int r, int g, int b) {
    kl_g_color_cur = RGB(r, g, b);
    kl_g_push(KL_C_COLOR, r, g, b, 0);
}

static void kl_g_pixel(int x, int y) { kl_g_push(KL_C_PIXEL, x, y, 0, 0); }
static void kl_g_line(int x1, int y1, int x2, int y2) { kl_g_push(KL_C_LINE, x1, y1, x2, y2); }
static void kl_g_rect(int x, int y, int w, int h) { kl_g_push(KL_C_RECT, x, y, w, h); }
static void kl_g_fillrect(int x, int y, int w, int h) { kl_g_push(KL_C_FILLRECT, x, y, w, h); }
static void kl_g_circle(int cx, int cy, int r) { kl_g_push(KL_C_CIRCLE, cx, cy, r, 0); }
static void kl_g_fillcircle(int cx, int cy, int r) { kl_g_push(KL_C_FILLCIRCLE, cx, cy, r, 0); }

static void kl_g_text(int x, int y, const char* utf8) {
    if (kl_ng >= 4096) return;
    int n = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, NULL, 0);
    wchar_t* w = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, w, n);
    kl_g_push(KL_C_TEXT, x, y, 0, 0);
    if (kl_ng > 0) kl_gbuf[kl_ng - 1].txt = w;
}

static void kl_g_font(const char* name_utf8, int size) {
    if (kl_ng >= 4096) return;
    int n = MultiByteToWideChar(CP_UTF8, 0, name_utf8, -1, NULL, 0);
    wchar_t* w = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    MultiByteToWideChar(CP_UTF8, 0, name_utf8, -1, w, n);
    kl_g_push(KL_C_FONT, size, 0, 0, 0);
    if (kl_ng > 0) kl_gbuf[kl_ng - 1].txt = w;
}
"#
}

fn net_runtime() -> &'static str {    r#"
#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#pragma warning(disable: 4996)

#define KL_MAXSOCK 32
static SOCKET kl_socks[KL_MAXSOCK];
static int kl_sock_init = 0;

static void kl_net_startup(void) {
    if (kl_sock_init) return;
    WSADATA w;
    if (WSAStartup(MAKEWORD(2, 2), &w) != 0) kl_panic("নেটওয়ার্ক চালু ব্যর্থ");
    for (int i = 0; i < KL_MAXSOCK; i++) kl_socks[i] = INVALID_SOCKET;
    kl_sock_init = 1;
}

static int64_t kl_net_slot(void) {
    for (int i = 0; i < KL_MAXSOCK; i++) if (kl_socks[i] == INVALID_SOCKET) return i;
    kl_panic("সংযোগের সীমা অতিক্রম হয়েছে");
    return -1;
}
#else
#include <sys/socket.h>
#include <netdb.h>
#include <unistd.h>
typedef int SOCKET;
#define INVALID_SOCKET (-1)
#define KL_MAXSOCK 32
static SOCKET kl_socks[KL_MAXSOCK];
static int kl_sock_init = 1;
static void kl_net_startup(void) {}
static int64_t kl_net_slot(void) {
    for (int i = 0; i < KL_MAXSOCK; i++) if (kl_socks[i] < 0) return i;
    kl_panic("সংযোগের সীমা অতিক্রম হয়েছে");
    return -1;
}
#endif

static int64_t kl_net_connect(kl_str host, int64_t port) {
    kl_net_startup();
#ifdef _WIN32
    char portbuf[8];
    snprintf(portbuf, sizeof(portbuf), "%lld", (long long)port);
    struct addrinfo hints = {0};
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo* res = NULL;
    if (getaddrinfo((const char*)host.data, portbuf, &hints, &res) != 0 || !res)
        kl_panic("সংযোগ ব্যর্থ — হোস্ট পাওয়া যায়নি");
    SOCKET s = socket(res->ai_family, res->ai_socktype, res->ai_protocol);
    if (s == INVALID_SOCKET || connect(s, res->ai_addr, (int)res->ai_addrlen) != 0) {
        int err = WSAGetLastError();
        freeaddrinfo(res);
        char b[128];
        snprintf(b, sizeof(b), "সংযোগ ব্যর্থ (WSA code %d)", err);
        kl_panic(b);
    }
    freeaddrinfo(res);
#else
    char portbuf[8];
    snprintf(portbuf, sizeof(portbuf), "%lld", (long long)port);
    struct addrinfo hints = {0};
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo* res = NULL;
    if (getaddrinfo((const char*)host.data, portbuf, &hints, &res) != 0 || !res)
        kl_panic("সংযোগ ব্যর্থ — হোস্ট পাওয়া যায়নি");
    SOCKET s = socket(res->ai_family, res->ai_socktype, res->ai_protocol);
    if (s < 0 || connect(s, res->ai_addr, res->ai_addrlen) != 0) {
        freeaddrinfo(res);
        kl_panic("সংযোগ ব্যর্থ");
    }
    freeaddrinfo(res);
#endif
    int64_t slot = kl_net_slot();
    kl_socks[slot] = s;
    return slot;
}

static void kl_net_send(int64_t slot, kl_str data) {
#ifdef _WIN32
    if (slot < 0 || slot >= KL_MAXSOCK || kl_socks[slot] == INVALID_SOCKET) kl_panic("অবৈধ সংযোগ");
    size_t len = data.data ? strlen((const char*)data.data) : 0;
    if (send(kl_socks[slot], (const char*)data.data, (int)len, 0) == SOCKET_ERROR)
        kl_panic("পাঠাতে ব্যর্থ");
#else
    if (slot < 0 || slot >= KL_MAXSOCK || kl_socks[slot] < 0) kl_panic("অবৈধ সংযোগ");
    size_t len = data.data ? strlen((const char*)data.data) : 0;
    if (write(kl_socks[slot], data.data, len) < 0) kl_panic("পাঠাতে ব্যর্থ");
#endif
}

static kl_str kl_net_recv(int64_t slot, int64_t maxbytes) {
#ifdef _WIN32
    if (slot < 0 || slot >= KL_MAXSOCK || kl_socks[slot] == INVALID_SOCKET) kl_panic("অবৈধ সংযোগ");
#else
    if (slot < 0 || slot >= KL_MAXSOCK || kl_socks[slot] < 0) kl_panic("অবৈধ সংযোগ");
#endif
    if (maxbytes < 1) maxbytes = 1;
    if (maxbytes > 1048576) maxbytes = 1048576;
    char buf[4096];
    kl_str r = kl_str_alloc(maxbytes);
    int64_t total = 0;
#ifdef _WIN32
    int n;
    while (total < maxbytes) {
        int want = (int)((maxbytes - total) < 4096 ? (maxbytes - total) : 4096);
        n = recv(kl_socks[slot], buf, want, 0);
        if (n <= 0) break;
        memcpy(r.data + total, buf, (size_t)n);
        total += n;
    }
#else
    ssize_t n;
    while (total < maxbytes) {
        size_t want = (size_t)((maxbytes - total) < 4096 ? (maxbytes - total) : 4096);
        n = read(kl_socks[slot], buf, want);
        if (n <= 0) break;
        memcpy(r.data + total, buf, (size_t)n);
        total += n;
    }
#endif
    r.data[total] = 0;
    r.len = kl_cpcount(r.data, total);
    return r;
}

static void kl_net_close(int64_t slot) {
#ifdef _WIN32
    if (slot >= 0 && slot < KL_MAXSOCK && kl_socks[slot] != INVALID_SOCKET) {
        closesocket(kl_socks[slot]);
        kl_socks[slot] = INVALID_SOCKET;
    }
#else
    if (slot >= 0 && slot < KL_MAXSOCK && kl_socks[slot] >= 0) {
        close(kl_socks[slot]);
        kl_socks[slot] = -1;
    }
#endif
}
"#
}
