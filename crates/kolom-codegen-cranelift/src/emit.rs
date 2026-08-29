//! Cranelift codegen. Milestone 1 covered the core language (literals,
//! variables, arithmetic, control flow, functions, `লেখো`). Milestone 2
//! adds containers + ownership: arrays, structs (real identity — not the
//! "struct is secretly a string-keyed map" trick the C backend uses),
//! `শেয়ার` (শেয়ার) refcounted boxes, and the `দৈর্ঘ্য`/`কপি`/`লেখায়`/
//! `শেয়ার_করো` builtins. Still no stdlib modules or the UI engine
//! (M3/M4), and refcounting here is intentionally the same *uniform*
//! scheme the C backend uses (decref every owning scope-local at scope
//! exit, incref on every ident-to-new-binding copy) rather than a precise
//! move analysis — see the plan doc's note on this judgment call.
//!
//! Every Kolom value — scalar or heap pointer — now fits in exactly one
//! Cranelift value (i64/f64/i8/pointer), which is why `Sym`/`CVal` dropped
//! the two-value Txt representation M1 used. Heap-backed values (Txt/Arr/
//! Shared/Struct) share a leading `rc: i64` header; see kolom-runtime for
//! the actual heap layouts.
//!
//! Known M2 simplifications (documented, not exercised by golden tests):
//! array elements of owning types (Arr<Txt> etc.) don't get increfed by
//! `+`-concat; scope-exit decref is skipped on early return/break/continue
//! (leaks rather than double-frees); struct-typed array/শেয়ার elements
//! aren't supported yet.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature, StackSlotData, StackSlotKind, Type as ClifType, Value};
use cranelift_codegen::ir::MemFlagsData;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;

use kolom_sema::Ty;
use kolom_syntax::ast::*;

#[derive(Clone)]
enum CVal {
    Num(Value),
    Dec(Value),
    Bool(Value),
    Txt(Value),
    Arr(Value, Box<Ty>),
    Struct(Value, String),
    Shared(Value, Box<Ty>),
    Map(Value, Box<Ty>, Box<Ty>),
    Void,
}

fn cval_value(v: &CVal) -> Value {
    match v {
        CVal::Num(x) | CVal::Dec(x) | CVal::Bool(x) | CVal::Txt(x) | CVal::Arr(x, _) | CVal::Shared(x, _) | CVal::Struct(x, _) | CVal::Map(x, _, _) => *x,
        CVal::Void => panic!("internal: void has no value"),
    }
}

fn cval_ty(v: &CVal) -> Ty {
    match v {
        CVal::Num(_) => Ty::Num,
        CVal::Dec(_) => Ty::Dec,
        CVal::Bool(_) => Ty::Bool,
        CVal::Txt(_) => Ty::Txt,
        CVal::Arr(_, t) => Ty::Arr(t.clone()),
        CVal::Shared(_, t) => Ty::Shared(t.clone()),
        CVal::Struct(_, n) => Ty::Struct(n.clone()),
        CVal::Map(_, k, v) => Ty::Map(k.clone(), v.clone()),
        CVal::Void => Ty::Null,
    }
}

fn is_owning(ty: &Ty) -> bool {
    matches!(ty, Ty::Txt | Ty::Arr(_) | Ty::Shared(_) | Ty::Struct(_) | Ty::Map(_, _))
}

#[derive(Clone)]
struct Sym {
    ty: Ty,
    var: Variable,
}

#[derive(Clone)]
struct FuncInfo {
    id: FuncId,
    params: Vec<Ty>,
    ret: Ty,
}

#[derive(Clone)]
struct StructLayout {
    fields: Vec<(String, Ty)>,
    drop_id: FuncId,
}

struct LoopCtx {
    continue_block: cranelift_codegen::ir::Block,
    break_block: cranelift_codegen::ir::Block,
}

struct Env {
    scopes: Vec<HashMap<String, Sym>>,
}

impl Env {
    fn new() -> Self {
        Env { scopes: Vec::new() }
    }
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) -> HashMap<String, Sym> {
        self.scopes.pop().unwrap_or_default()
    }
    fn insert(&mut self, name: String, sym: Sym) {
        self.scopes.last_mut().expect("scope stack empty").insert(name, sym);
    }
    fn lookup(&self, name: &str) -> Option<Sym> {
        for s in self.scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return Some(v.clone());
            }
        }
        None
    }
}

fn resolve_type(te: &TypeExpr) -> Ty {
    match te {
        TypeExpr::Named(id) => match id.name.as_str() {
            "সংখ্যা" => Ty::Num,
            "দশমিক" => Ty::Dec,
            "লেখা" => Ty::Txt,
            "সত্যতা" => Ty::Bool,
            "অক্ষর" => Ty::Ch,
            "ফাঁকা" => Ty::Null,
            other => Ty::Struct(other.to_string()),
        },
        TypeExpr::Array(inner) => Ty::Arr(Box::new(resolve_type(inner))),
        TypeExpr::Shared(inner) => Ty::Shared(Box::new(resolve_type(inner))),
        TypeExpr::Map(k, v) => Ty::Map(Box::new(resolve_type(k)), Box::new(resolve_type(v))),
    }
}

fn is_map_new_call(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Postfix(base, suffixes)
        if matches!(&base.kind, ExprKind::Ident(id) if id.name == "ম্যাপ_তৈরি")
        && matches!(suffixes.as_slice(), [Suffix::Call(a, _)] if a.is_empty()))
}

fn hex(name: &str) -> String {
    let mut s = String::new();
    for b in name.as_bytes() {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// ASCII-mangles a (possibly non-ASCII) Kolom identifier into a safe object
/// symbol name.
fn mangle(name: &str) -> String {
    format!("kf_{}", hex(name))
}

fn mangle_drop(name: &str) -> String {
    format!("kd_{}", hex(name))
}

pub struct Gen {
    module: ObjectModule,
    ptr_ty: ClifType,
    funcs: HashMap<String, FuncInfo>,
    structs: HashMap<String, StructLayout>,
    /// Top-level `ধ্রুবক` initializers, keyed by name. Kolom constants are
    /// immutable, so rather than allocating globals these are re-lowered
    /// inline wherever the name is referenced — which also makes them
    /// visible inside function bodies without threading a global scope.
    consts: HashMap<String, Expr>,
    str_counter: u32,

    print_num: FuncId,
    print_bool: FuncId,
    print_text: FuncId,
    num_to_text: FuncId,
    rc_incref: FuncId,
    str_new: FuncId,
    str_len: FuncId,
    str_copy: FuncId,
    str_decref: FuncId,
    arr_new: FuncId,
    arr_len: FuncId,
    arr_get_ptr: FuncId,
    arr_push: FuncId,
    arr_concat: FuncId,
    arr_decref: FuncId,
    shared_new: FuncId,
    shared_payload_ptr: FuncId,
    shared_decref: FuncId,
    struct_new: FuncId,
    struct_decref: FuncId,

    /// M3+ stdlib/map runtime imports, keyed by kolom-runtime symbol name —
    /// avoids one named `Gen` field per function for the ~40-function
    /// standard library (see `emit()`'s `stdlib_imports` table).
    rt: HashMap<&'static str, FuncId>,
}

impl Gen {
    /// Calls a named import from `rt` and returns its (single) result value.
    fn call_rt(&mut self, b: &mut FunctionBuilder, name: &str, args: &[Value]) -> Value {
        let id = *self.rt.get(name).unwrap_or_else(|| panic!("internal: unknown runtime fn '{}'", name));
        let f = self.module.declare_func_in_func(id, b.func);
        let call = b.ins().call(f, args);
        b.inst_results(call)[0]
    }

    /// Like `call_rt` but for void-returning imports.
    fn call_rt_void(&mut self, b: &mut FunctionBuilder, name: &str, args: &[Value]) {
        let id = *self.rt.get(name).unwrap_or_else(|| panic!("internal: unknown runtime fn '{}'", name));
        let f = self.module.declare_func_in_func(id, b.func);
        b.ins().call(f, args);
    }
    fn clif_ty_of(&self, ty: &Ty) -> ClifType {
        match ty {
            Ty::Num => types::I64,
            Ty::Dec => types::F64,
            Ty::Bool => types::I8,
            _ => self.ptr_ty, // Txt/Arr/Shared/Struct: single heap pointer
        }
    }

    fn clif_types(&self, ty: &Ty) -> Result<Vec<ClifType>, String> {
        Ok(match ty {
            Ty::Null => vec![],
            Ty::Num | Ty::Dec | Ty::Bool | Ty::Txt | Ty::Arr(_) | Ty::Shared(_) | Ty::Struct(_) | Ty::Map(_, _) => vec![self.clif_ty_of(ty)],
            other => return Err(format!("M3 codegen: টাইপ '{}' এখনো সমর্থিত নয়", other)),
        })
    }

    fn make_signature(&self, params: &[Ty], ret: &Ty) -> Result<Signature, String> {
        let mut sig = self.module.make_signature();
        for p in params {
            for t in self.clif_types(p)? {
                sig.params.push(AbiParam::new(t));
            }
        }
        for t in self.clif_types(ret)? {
            sig.returns.push(AbiParam::new(t));
        }
        Ok(sig)
    }

    fn wrap_cval(&self, ty: &Ty, v: Value) -> CVal {
        match ty {
            Ty::Num => CVal::Num(v),
            Ty::Dec => CVal::Dec(v),
            Ty::Bool => CVal::Bool(v),
            Ty::Txt => CVal::Txt(v),
            Ty::Arr(inner) => CVal::Arr(v, inner.clone()),
            Ty::Shared(inner) => CVal::Shared(v, inner.clone()),
            Ty::Struct(name) => CVal::Struct(v, name.clone()),
            Ty::Map(k, val) => CVal::Map(v, k.clone(), val.clone()),
            _ => unreachable!("M3 codegen: unsupported wrapped type"),
        }
    }

    fn load_cval(&mut self, b: &mut FunctionBuilder, ty: &Ty, addr: Value, offset: i32) -> CVal {
        let cty = self.clif_ty_of(ty);
        let v = b.ins().load(cty, MemFlagsData::trusted(), addr, offset);
        self.wrap_cval(ty, v)
    }

    fn store_cval(&mut self, b: &mut FunctionBuilder, val: &CVal, addr: Value, offset: i32) {
        b.ins().store(MemFlagsData::trusted(), cval_value(val), addr, offset);
    }

    /// Generic refcount bump — every heap type's allocation starts with the
    /// same `rc: i64` field, so one function works for all of them.
    fn emit_incref(&mut self, b: &mut FunctionBuilder, val: Value) {
        let f = self.module.declare_func_in_func(self.rc_incref, b.func);
        b.ins().call(f, &[val]);
    }

    fn emit_decref(&mut self, b: &mut FunctionBuilder, ty: &Ty, val: Value) -> Result<(), String> {
        match ty {
            Ty::Txt => {
                let f = self.module.declare_func_in_func(self.str_decref, b.func);
                b.ins().call(f, &[val]);
            }
            Ty::Arr(_) => {
                let f = self.module.declare_func_in_func(self.arr_decref, b.func);
                b.ins().call(f, &[val]);
            }
            Ty::Shared(_) => {
                let f = self.module.declare_func_in_func(self.shared_decref, b.func);
                b.ins().call(f, &[val]);
            }
            Ty::Struct(name) => {
                let layout = self.structs.get(name).cloned().ok_or_else(|| format!("অজানা struct '{}'", name))?;
                let field_count = layout.fields.len() as i64;
                let drop_fref = self.module.declare_func_in_func(layout.drop_id, b.func);
                let drop_addr = b.ins().func_addr(self.ptr_ty, drop_fref);
                let fc_const = b.ins().iconst(types::I64, field_count);
                let f = self.module.declare_func_in_func(self.struct_decref, b.func);
                b.ins().call(f, &[val, fc_const, drop_addr]);
            }
            Ty::Map(_, _) => {
                self.call_rt_void(b, "kl_map_decref", &[val]);
            }
            _ => {}
        }
        Ok(())
    }

    /// Address of the 1-arg `extern "C" fn(*mut u8)` that fully decrefs one
    /// value of `ty`, or a null constant for non-owning types. Used as the
    /// `drop_elem`/`drop_payload` callback stored in array/shared headers.
    fn drop_addr_for(&mut self, b: &mut FunctionBuilder, ty: &Ty) -> Result<Value, String> {
        Ok(match ty {
            Ty::Num | Ty::Dec | Ty::Bool => b.ins().iconst(self.ptr_ty, 0),
            Ty::Txt => {
                let f = self.module.declare_func_in_func(self.str_decref, b.func);
                b.ins().func_addr(self.ptr_ty, f)
            }
            Ty::Arr(_) => {
                let f = self.module.declare_func_in_func(self.arr_decref, b.func);
                b.ins().func_addr(self.ptr_ty, f)
            }
            Ty::Shared(_) => {
                let f = self.module.declare_func_in_func(self.shared_decref, b.func);
                b.ins().func_addr(self.ptr_ty, f)
            }
            Ty::Struct(_) => return Err("M2 codegen: struct-উপাদানসহ array/শেয়ার এখনো সমর্থিত নয়".into()),
            _ => return Err("M2 codegen: এই উপাদান টাইপ এখনো সমর্থিত নয়".into()),
        })
    }

    fn bind_param(&self, b: &mut FunctionBuilder, ty: &Ty, block_params: &[Value], pidx: &mut usize) -> Sym {
        let cty = self.clif_ty_of(ty);
        let var = b.declare_var(cty);
        b.def_var(var, block_params[*pidx]);
        *pidx += 1;
        Sym { ty: ty.clone(), var }
    }

    fn lower_lit(&mut self, b: &mut FunctionBuilder, lit: &Lit, env: &mut Env) -> Result<CVal, String> {
        match lit {
            Lit::Int(i) => Ok(CVal::Num(b.ins().iconst(types::I64, *i))),
            Lit::Float(f) => Ok(CVal::Dec(b.ins().f64const(*f))),
            Lit::Bool(v) => Ok(CVal::Bool(b.ins().iconst(types::I8, if *v { 1 } else { 0 }))),
            Lit::Str(s) => self.make_str(b, s.as_bytes()),
            Lit::Array(elems) => self.lower_array_lit(b, elems, env),
            Lit::Char(_) | Lit::Null => Err("M2 codegen: এই ধরনের literal এখনো সমর্থিত নয়".into()),
        }
    }

    /// Copies `bytes` (e.g. a literal's static rodata) into a fresh heap
    /// `kl_str` box (rc=1).
    fn make_str(&mut self, b: &mut FunctionBuilder, bytes: &[u8]) -> Result<CVal, String> {
        let name = format!("str${}", self.str_counter);
        self.str_counter += 1;
        let data_id = self.module.declare_data(&name, Linkage::Local, false, false).map_err(|e| e.to_string())?;
        let mut dd = DataDescription::new();
        dd.define(bytes.to_vec().into_boxed_slice());
        self.module.define_data(data_id, &dd).map_err(|e| e.to_string())?;
        let local = self.module.declare_data_in_func(data_id, b.func);
        let static_ptr = b.ins().symbol_value(self.ptr_ty, local);
        let len = b.ins().iconst(types::I64, bytes.len() as i64);
        let f = self.module.declare_func_in_func(self.str_new, b.func);
        let call = b.ins().call(f, &[static_ptr, len]);
        Ok(CVal::Txt(b.inst_results(call)[0]))
    }

    fn lower_array_lit(&mut self, b: &mut FunctionBuilder, elems: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if elems.is_empty() {
            return Err("M2 codegen: খালি array literal এখনো সমর্থিত নয়".into());
        }
        let mut vals = Vec::with_capacity(elems.len());
        for e in elems {
            vals.push(self.lower_expr_for_binding(b, e, env)?);
        }
        let elem_ty = cval_ty(&vals[0]);
        let drop_addr = self.drop_addr_for(b, &elem_ty)?;
        let es_const = b.ins().iconst(types::I64, 8);
        let len_const = b.ins().iconst(types::I64, elems.len() as i64);
        let newf = self.module.declare_func_in_func(self.arr_new, b.func);
        let call = b.ins().call(newf, &[es_const, len_const, drop_addr]);
        let arr_ptr = b.inst_results(call)[0];

        let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 0));
        for v in &vals {
            let addr = b.ins().stack_addr(self.ptr_ty, slot, 0);
            self.store_cval(b, v, addr, 0);
            let pushf = self.module.declare_func_in_func(self.arr_push, b.func);
            b.ins().call(pushf, &[arr_ptr, addr]);
        }
        Ok(CVal::Arr(arr_ptr, Box::new(elem_ty)))
    }

    fn lower_unary(&mut self, b: &mut FunctionBuilder, op: UnaryOp, inner: &Expr, env: &mut Env) -> Result<CVal, String> {
        let v = self.lower_expr(b, inner, env)?;
        match (op, v) {
            (UnaryOp::Neg, CVal::Num(x)) => Ok(CVal::Num(b.ins().ineg(x))),
            (UnaryOp::Neg, CVal::Dec(x)) => Ok(CVal::Dec(b.ins().fneg(x))),
            (UnaryOp::Not, CVal::Bool(x)) => {
                let one = b.ins().iconst(types::I8, 1);
                Ok(CVal::Bool(b.ins().bxor(x, one)))
            }
            _ => Err("M2 codegen: ইউনারি অপারেটর টাইপ অমিল".into()),
        }
    }

    fn lower_binary(&mut self, b: &mut FunctionBuilder, op: BinOp, l: &Expr, r: &Expr, env: &mut Env) -> Result<CVal, String> {
        let lv = self.lower_expr(b, l, env)?;
        let rv = self.lower_expr(b, r, env)?;
        match (lv, rv) {
            (CVal::Num(a), CVal::Num(c)) => num_binop(b, op, a, c),
            (CVal::Dec(a), CVal::Dec(c)) => dec_binop(b, op, a, c),
            (CVal::Bool(a), CVal::Bool(c)) => bool_binop(b, op, a, c),
            (CVal::Txt(a), CVal::Txt(c)) => match op {
                BinOp::Add => Ok(CVal::Txt(self.call_rt(b, "kl_str_concat", &[a, c]))),
                _ => Err("M3 codegen: 'লেখা'-এ শুধু + (জোড়া লাগানো) সমর্থিত".into()),
            },
            (CVal::Arr(a, ta), CVal::Arr(c, tc)) => {
                if ta != tc {
                    return Err("M2 codegen: ভিন্ন এলিমেন্ট টাইপের array যোগ করা যায় না".into());
                }
                if op != BinOp::Add {
                    return Err("M2 codegen: array-এ শুধু + সমর্থিত".into());
                }
                let f = self.module.declare_func_in_func(self.arr_concat, b.func);
                let call = b.ins().call(f, &[a, c]);
                Ok(CVal::Arr(b.inst_results(call)[0], ta))
            }
            _ => Err("M2 codegen: বাইনারি অপারেটরের দুই পাশের টাইপ মেলেনি (বা এখনো সমর্থিত নয়)".into()),
        }
    }

    /// Evaluates `e`. If `e` is itself a plain variable read of an owning
    /// type, bumps its refcount first — this is the ONE place the uniform
    /// "incref on every copy-into-a-new-binding" rule is applied, used by
    /// every call site that stores a value into new persistent storage
    /// (`ধরি`/reassignment/array-element-write/field-write/`শেয়ার_করো`/
    /// struct-construction args/array-literal elements).
    fn lower_expr_for_binding(&mut self, b: &mut FunctionBuilder, e: &Expr, env: &mut Env) -> Result<CVal, String> {
        let v = self.lower_expr(b, e, env)?;
        if matches!(e.kind, ExprKind::Ident(_)) {
            let ty = cval_ty(&v);
            if is_owning(&ty) {
                self.emit_incref(b, cval_value(&v));
            }
        }
        Ok(v)
    }

    fn lower_length(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 1 {
            return Err("M2 codegen: দৈর্ঘ্য() ঠিক একটি আর্গুমেন্ট নেয়".into());
        }
        let v = self.lower_expr(b, &args[0], env)?;
        match v {
            CVal::Arr(p, _) => {
                let f = self.module.declare_func_in_func(self.arr_len, b.func);
                let call = b.ins().call(f, &[p]);
                Ok(CVal::Num(b.inst_results(call)[0]))
            }
            // Codepoints, not bytes — matches the interpreter and C backend.
            CVal::Txt(p) => Ok(CVal::Num(self.call_rt(b, "kl_str_cplen", &[p]))),
            CVal::Map(p, ..) => Ok(CVal::Num(self.call_rt(b, "kl_map_len", &[p]))),
            _ => Err("M3 codegen: দৈর্ঘ্য() শুধু array/text/map-এ কাজ করে".into()),
        }
    }

    fn lower_map_new(&mut self, b: &mut FunctionBuilder, key_ty: &Ty, val_ty: &Ty) -> Result<CVal, String> {
        let key_kind = match key_ty {
            Ty::Txt => 0i64,
            Ty::Num => 1i64,
            other => return Err(format!("M3 codegen: ম্যাপ key '{}' সমর্থিত নয়", other)),
        };
        let kk = b.ins().iconst(types::I64, key_kind);
        let ptr = self.call_rt(b, "kl_map_new", &[kk]);
        Ok(CVal::Map(ptr, Box::new(key_ty.clone()), Box::new(val_ty.clone())))
    }

    fn map_key_bits(&mut self, _b: &mut FunctionBuilder, key: &CVal) -> Result<Value, String> {
        match key {
            CVal::Txt(p) => Ok(*p),
            CVal::Num(v) => Ok(*v),
            _ => Err("M3 codegen: ম্যাপ key 'লেখা' বা 'সংখ্যা' হতে হবে".into()),
        }
    }

    fn lower_map_contains(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 2 {
            return Err("M3 codegen: আছে_কি() ২টি আর্গুমেন্ট নেয়".into());
        }
        let mv = self.lower_expr(b, &args[0], env)?;
        let (map_ptr, _, _) = match mv {
            CVal::Map(p, k, v) => (p, k, v),
            _ => return Err("M3 codegen: আছে_কি() প্রথম আর্গুমেন্টে ম্যাপ প্রত্যাশিত".into()),
        };
        let kv = self.lower_expr(b, &args[1], env)?;
        let key_bits = self.map_key_bits(b, &kv)?;
        let slot = self.call_rt(b, "kl_map_find", &[map_ptr, key_bits]);
        let zero = b.ins().iconst(self.ptr_ty, 0);
        let found = b.ins().icmp(IntCC::NotEqual, slot, zero);
        Ok(CVal::Bool(found))
    }

    fn lower_map_keys(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 1 {
            return Err("M3 codegen: চাবি_গুলো() ১টি আর্গুমেন্ট নেয়".into());
        }
        let mv = self.lower_expr(b, &args[0], env)?;
        let map_ptr = match mv {
            CVal::Map(p, ..) => p,
            _ => return Err("M3 codegen: চাবি_গুলো() ম্যাপ প্রত্যাশিত".into()),
        };
        let arr_ptr = self.call_rt(b, "kl_map_keys", &[map_ptr]);
        Ok(CVal::Arr(arr_ptr, Box::new(Ty::Txt)))
    }

    fn lower_map_delete_key(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 2 {
            return Err("M3 codegen: চাবি_মুছো() ২টি আর্গুমেন্ট নেয়".into());
        }
        let mv = self.lower_expr(b, &args[0], env)?;
        let map_ptr = match mv {
            CVal::Map(p, ..) => p,
            _ => return Err("M3 codegen: চাবি_মুছো() প্রথম আর্গুমেন্টে ম্যাপ প্রত্যাশিত".into()),
        };
        let kv = self.lower_expr(b, &args[1], env)?;
        let key_bits = self.map_key_bits(b, &kv)?;
        self.call_rt_void(b, "kl_map_delete", &[map_ptr, key_bits]);
        Ok(CVal::Void)
    }

    fn lower_copy(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 1 {
            return Err("M2 codegen: কপি() ঠিক একটি আর্গুমেন্ট নেয়".into());
        }
        let v = self.lower_expr(b, &args[0], env)?;
        match v {
            CVal::Txt(p) => {
                let f = self.module.declare_func_in_func(self.str_copy, b.func);
                let call = b.ins().call(f, &[p]);
                Ok(CVal::Txt(b.inst_results(call)[0]))
            }
            _ => Err("M2 codegen: কপি() এখনো শুধু 'লেখা'-এ সমর্থিত".into()),
        }
    }

    fn lower_to_text(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 1 {
            return Err("M2 codegen: লেখায়() ঠিক একটি আর্গুমেন্ট নেয়".into());
        }
        let v = self.lower_expr(b, &args[0], env)?;
        match v {
            CVal::Num(x) => {
                let f = self.module.declare_func_in_func(self.num_to_text, b.func);
                let call = b.ins().call(f, &[x]);
                Ok(CVal::Txt(b.inst_results(call)[0]))
            }
            CVal::Dec(x) => Ok(CVal::Txt(self.call_rt(b, "kl_dec_to_text", &[x]))),
            CVal::Bool(x) => Ok(CVal::Txt(self.call_rt(b, "kl_bool_to_text", &[x]))),
            _ => Err("M3 codegen: লেখায়() এখনো শুধু 'সংখ্যা'/'দশমিক'/'সত্যতা'-এ সমর্থিত".into()),
        }
    }

    fn lower_share(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 1 {
            return Err("M2 codegen: শেয়ার_করো() ঠিক একটি আর্গুমেন্ট নেয়".into());
        }
        let v = self.lower_expr_for_binding(b, &args[0], env)?;
        let inner_ty = cval_ty(&v);
        let drop_addr = self.drop_addr_for(b, &inner_ty)?;
        let size_c = b.ins().iconst(types::I64, 8);
        let newf = self.module.declare_func_in_func(self.shared_new, b.func);
        let call = b.ins().call(newf, &[size_c, drop_addr]);
        let box_ptr = b.inst_results(call)[0];
        let pf = self.module.declare_func_in_func(self.shared_payload_ptr, b.func);
        let pcall = b.ins().call(pf, &[box_ptr]);
        let payload_addr = b.inst_results(pcall)[0];
        b.ins().store(MemFlagsData::trusted(), cval_value(&v), payload_addr, 0);
        Ok(CVal::Shared(box_ptr, Box::new(inner_ty)))
    }

    fn lower_struct_new(&mut self, b: &mut FunctionBuilder, name: &str, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        let layout = self.structs.get(name).cloned().ok_or_else(|| format!("অজানা struct '{}'", name))?;
        if args.len() != layout.fields.len() {
            return Err(format!("M2 codegen: '{}' গঠনকারীতে {}টি আর্গুমেন্ট দরকার", name, layout.fields.len()));
        }
        let fc_const = b.ins().iconst(types::I64, layout.fields.len() as i64);
        let newf = self.module.declare_func_in_func(self.struct_new, b.func);
        let call = b.ins().call(newf, &[fc_const]);
        let ptr = b.inst_results(call)[0];
        for (idx, arg) in args.iter().enumerate() {
            let v = self.lower_expr_for_binding(b, arg, env)?;
            let offset = (8 + idx * 8) as i32;
            self.store_cval(b, &v, ptr, offset);
        }
        Ok(CVal::Struct(ptr, name.to_string()))
    }

    fn lower_field_read(&mut self, b: &mut FunctionBuilder, base: &Expr, fld: &Ident, env: &mut Env) -> Result<CVal, String> {
        let bv = self.lower_expr(b, base, env)?;
        let (ptr, name) = match bv {
            CVal::Struct(p, n) => (p, n),
            _ => return Err("M2 codegen: '.' শুধু struct-এ ব্যবহার করা যায়".into()),
        };
        let layout = self.structs.get(&name).cloned().ok_or_else(|| format!("অজানা struct '{}'", name))?;
        let idx = layout
            .fields
            .iter()
            .position(|(fname, _)| fname == &fld.name)
            .ok_or_else(|| format!("'{}'-এ '{}' ফিল্ড নেই", name, fld.name))?;
        let fty = layout.fields[idx].1.clone();
        let offset = (8 + idx * 8) as i32;
        Ok(self.load_cval(b, &fty, ptr, offset))
    }

    fn lower_index_read(&mut self, b: &mut FunctionBuilder, base: &Expr, ix: &Expr, env: &mut Env) -> Result<CVal, String> {
        let bv = self.lower_expr(b, base, env)?;
        match bv {
            CVal::Arr(ptr, elem_ty) => {
                let idx_val = self.lower_expr_num(b, ix, env)?;
                let f = self.module.declare_func_in_func(self.arr_get_ptr, b.func);
                let call = b.ins().call(f, &[ptr, idx_val]);
                let addr = b.inst_results(call)[0];
                Ok(self.load_cval(b, &elem_ty, addr, 0))
            }
            CVal::Map(map_ptr, _key_ty, val_ty) => {
                let kv = self.lower_expr(b, ix, env)?;
                let key_bits = self.map_key_bits(b, &kv)?;
                let slot = self.call_rt(b, "kl_map_find", &[map_ptr, key_bits]);
                let zero = b.ins().iconst(self.ptr_ty, 0);
                let found = b.ins().icmp(IntCC::NotEqual, slot, zero);
                let ok_blk = b.create_block();
                let bad_blk = b.create_block();
                b.ins().brif(found, ok_blk, &[], bad_blk, &[]);
                b.switch_to_block(bad_blk);
                b.seal_block(bad_blk);
                self.call_rt_void(b, "kl_map_missing_key", &[]);
                b.ins().trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
                b.switch_to_block(ok_blk);
                b.seal_block(ok_blk);
                Ok(self.load_cval(b, &val_ty, slot, 0))
            }
            _ => Err("M3 codegen: '[]' শুধু array/map-এ ব্যবহার করা যায়".into()),
        }
    }

    fn lower_print_cval(&mut self, b: &mut FunctionBuilder, v: CVal) -> Result<CVal, String> {
        match v {
            CVal::Num(val) => {
                let f = self.module.declare_func_in_func(self.print_num, b.func);
                b.ins().call(f, &[val]);
            }
            CVal::Bool(val) => {
                let f = self.module.declare_func_in_func(self.print_bool, b.func);
                b.ins().call(f, &[val]);
            }
            CVal::Txt(p) => {
                let f = self.module.declare_func_in_func(self.print_text, b.func);
                b.ins().call(f, &[p]);
            }
            CVal::Shared(ptr, inner_ty) => {
                let pf = self.module.declare_func_in_func(self.shared_payload_ptr, b.func);
                let call = b.ins().call(pf, &[ptr]);
                let payload_addr = b.inst_results(call)[0];
                let inner = self.load_cval(b, &inner_ty, payload_addr, 0);
                return self.lower_print_cval(b, inner);
            }
            CVal::Dec(val) => {
                let txt = self.call_rt(b, "kl_dec_to_text", &[val]);
                let f = self.module.declare_func_in_func(self.print_text, b.func);
                b.ins().call(f, &[txt]);
            }
            CVal::Arr(..) | CVal::Struct(..) | CVal::Map(..) => return Err("M3 codegen: array/struct/map সরাসরি প্রিন্ট এখনো সমর্থিত নয়".into()),
            CVal::Void => return Err("M3 codegen: ফাঁকা মান প্রিন্ট করা যায় না".into()),
        }
        Ok(CVal::Void)
    }

    fn lower_print(&mut self, b: &mut FunctionBuilder, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        if args.len() != 1 {
            return Err("M2 codegen: লেখো() ঠিক একটি আর্গুমেন্ট নেয়".into());
        }
        let v = self.lower_expr(b, &args[0], env)?;
        self.lower_print_cval(b, v)
    }

    fn lower_call(&mut self, b: &mut FunctionBuilder, name: &str, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        let info = self.funcs.get(name).cloned().ok_or_else(|| format!("অজানা ফাংশন '{}'", name))?;
        let mut cargs: Vec<Value> = Vec::new();
        for a in args {
            let v = self.lower_expr_for_binding(b, a, env)?;
            cargs.push(cval_value(&v));
        }
        let local = self.module.declare_func_in_func(info.id, b.func);
        let call = b.ins().call(local, &cargs);
        let results = b.inst_results(call).to_vec();
        Ok(match info.ret {
            Ty::Null => CVal::Void,
            other => self.wrap_cval(&other, results[0]),
        })
    }

    fn lower_expr(&mut self, b: &mut FunctionBuilder, e: &Expr, env: &mut Env) -> Result<CVal, String> {
        match &e.kind {
            ExprKind::Lit(l) => self.lower_lit(b, l, env),
            ExprKind::Ident(id) => {
                if let Some(sym) = env.lookup(&id.name) {
                    let v = b.use_var(sym.var);
                    return Ok(self.wrap_cval(&sym.ty, v));
                }
                // Not a local — may be a top-level `ধ্রুবক`, whose
                // initializer is re-lowered here (see `Gen::consts`).
                if let Some(init) = self.consts.get(&id.name).cloned() {
                    return self.lower_expr(b, &init, env);
                }
                Err(format!("অজানা ভ্যারিয়েবল '{}'", id.name))
            }
            ExprKind::Unary(op, inner) => self.lower_unary(b, *op, inner, env),
            ExprKind::Binary(op, l, r) => self.lower_binary(b, *op, l, r, env),
            ExprKind::Assign(lv, rhs) => self.lower_assign(b, lv, rhs, env),
            ExprKind::Postfix(base, suffixes) => {
                if let ExprKind::Ident(id) = &base.kind {
                    if let [Suffix::Call(args, _)] = suffixes.as_slice() {
                        match id.name.as_str() {
                            "লেখো" => return self.lower_print(b, args, env),
                            "দৈর্ঘ্য" => return self.lower_length(b, args, env),
                            "কপি" => return self.lower_copy(b, args, env),
                            "লেখায়" => return self.lower_to_text(b, args, env),
                            "শেয়ার_করো" => return self.lower_share(b, args, env),
                            // Reads a line from stdin — a builtin, not a
                            // `ফাইল` member, since it reads from the user.
                            "পড়ো_লাইন" => {
                                if !args.is_empty() {
                                    return Err("M3 codegen: পড়ো_লাইন() কোনো আর্গুমেন্ট নেয় না".into());
                                }
                                return Ok(CVal::Txt(self.call_rt(b, "kl_io_read_line", &[])));
                            }
                            "আছে_কি" => return self.lower_map_contains(b, args, env),
                            "চাবি_গুলো" => return self.lower_map_keys(b, args, env),
                            "চাবি_মুছো" => return self.lower_map_delete_key(b, args, env),
                            _ => {}
                        }
                        if self.structs.contains_key(&id.name) {
                            return self.lower_struct_new(b, &id.name, args, env);
                        }
                        if self.funcs.contains_key(&id.name) {
                            return self.lower_call(b, &id.name, args, env);
                        }
                    }
                }
                if let ExprKind::Qualified { module, name } = &base.kind {
                    if let [Suffix::Call(args, _)] = suffixes.as_slice() {
                        // struct_var.field(...) isn't a thing in Kolom (no
                        // first-class functions), so Qualified+Call is
                        // unambiguously a stdlib module call.
                        return self.lower_stdlib_call(b, &module.name, &name.name, args, env);
                    }
                }
                if let [Suffix::Field(fld)] = suffixes.as_slice() {
                    return self.lower_field_read(b, base, fld, env);
                }
                if let [Suffix::Index(ix, _)] = suffixes.as_slice() {
                    return self.lower_index_read(b, base, ix, env);
                }
                Err("M3 codegen: এই postfix expression এখনো সমর্থিত নয়".into())
            }
            ExprKind::Qualified { module, name } => {
                // The parser can't distinguish `struct_var.field` from
                // `stdlib_module.item` syntactically — both are `Ident.Ident`.
                // If `module` resolves to a local Struct-typed variable,
                // treat this as a field read; otherwise it's an actual
                // stdlib access (only calls are supported — see the Postfix
                // arm above; a bare qualified constant read like `গণিত.পাই`
                // isn't wired up yet).
                if let Some(sym) = env.lookup(&module.name) {
                    if let Ty::Struct(sname) = &sym.ty {
                        let layout = self.structs.get(sname).cloned().ok_or_else(|| format!("অজানা struct '{}'", sname))?;
                        let idx = layout
                            .fields
                            .iter()
                            .position(|(fname, _)| fname == &name.name)
                            .ok_or_else(|| format!("'{}'-এ '{}' ফিল্ড নেই", sname, name.name))?;
                        let fty = layout.fields[idx].1.clone();
                        let ptr = b.use_var(sym.var);
                        let offset = (8 + idx * 8) as i32;
                        return Ok(self.load_cval(b, &fty, ptr, offset));
                    }
                }
                Err("M2 codegen: module-qualified কল (স্ট্যান্ডার্ড লাইব্রেরি) এখনো সমর্থিত নয়".into())
            }
            ExprKind::FieldAssign(base_id, fld, rhs) => self.lower_field_assign(b, base_id, fld, rhs, env),
        }
    }

    fn lower_assign(&mut self, b: &mut FunctionBuilder, lv: &LValue, rhs: &Expr, env: &mut Env) -> Result<CVal, String> {
        if let Some(fld) = &lv.field {
            if !lv.idx.is_empty() {
                return Err("M2 codegen: ইনডেক্স+ফিল্ড একসাথে assign সমর্থিত নয়".into());
            }
            return self.lower_field_assign(b, &lv.base, fld, rhs, env);
        }
        if lv.idx.len() > 1 {
            return Err("M2 codegen: মাল্টি-ডাইমেনশনাল ইনডেক্স সমর্থিত নয়".into());
        }
        let sym = env.lookup(&lv.base.name).ok_or_else(|| format!("অজানা ভ্যারিয়েবল '{}'", lv.base.name))?;

        if let Some(ix_expr) = lv.idx.first() {
            match sym.ty.clone() {
                Ty::Arr(elem_ty) => {
                    let elem_ty = *elem_ty;
                    let arr_ptr = b.use_var(sym.var);
                    let idx_val = self.lower_expr_num(b, ix_expr, env)?;
                    let rv = self.lower_expr_for_binding(b, rhs, env)?;
                    let f = self.module.declare_func_in_func(self.arr_get_ptr, b.func);
                    let call = b.ins().call(f, &[arr_ptr, idx_val]);
                    let addr = b.inst_results(call)[0];
                    if is_owning(&elem_ty) {
                        let old = self.load_cval(b, &elem_ty, addr, 0);
                        let oldv = cval_value(&old);
                        self.emit_decref(b, &elem_ty, oldv)?;
                    }
                    self.store_cval(b, &rv, addr, 0);
                    return Ok(rv);
                }
                Ty::Map(_key_ty, val_ty) => {
                    let val_ty = *val_ty;
                    let map_ptr = b.use_var(sym.var);
                    let kv = self.lower_expr(b, ix_expr, env)?;
                    let key_bits = self.map_key_bits(b, &kv)?;
                    let rv = self.lower_expr_for_binding(b, rhs, env)?;
                    let slot_ss = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 0));
                    let out_addr = b.ins().stack_addr(self.ptr_ty, slot_ss, 0);
                    let addr = self.call_rt(b, "kl_map_set_slot", &[map_ptr, key_bits, out_addr]);
                    let new_map_ptr = b.ins().stack_load(self.ptr_ty, self.ptr_ty, slot_ss, 0);
                    b.def_var(sym.var, new_map_ptr);
                    if is_owning(&val_ty) {
                        let old = self.load_cval(b, &val_ty, addr, 0);
                        let oldv = cval_value(&old);
                        self.emit_decref(b, &val_ty, oldv)?;
                    }
                    self.store_cval(b, &rv, addr, 0);
                    return Ok(rv);
                }
                _ => return Err("M3 codegen: ইনডেক্স assign শুধু array/map-এ প্রযোজ্য".into()),
            }
        }

        let rv = self.lower_expr_for_binding(b, rhs, env)?;
        if is_owning(&sym.ty) {
            let old = b.use_var(sym.var);
            self.emit_decref(b, &sym.ty, old)?;
        }
        b.def_var(sym.var, cval_value(&rv));
        Ok(rv)
    }

    fn lower_field_assign(&mut self, b: &mut FunctionBuilder, base_id: &Ident, fld: &Ident, rhs: &Expr, env: &mut Env) -> Result<CVal, String> {
        let sym = env.lookup(&base_id.name).ok_or_else(|| format!("অজানা ভ্যারিয়েবল '{}'", base_id.name))?;
        let name = match &sym.ty {
            Ty::Struct(n) => n.clone(),
            _ => return Err("M2 codegen: '.' assign শুধু struct-এ প্রযোজ্য".into()),
        };
        let layout = self.structs.get(&name).cloned().ok_or_else(|| format!("অজানা struct '{}'", name))?;
        let idx = layout
            .fields
            .iter()
            .position(|(fname, _)| fname == &fld.name)
            .ok_or_else(|| format!("'{}'-এ '{}' ফিল্ড নেই", name, fld.name))?;
        let fty = layout.fields[idx].1.clone();
        let ptr = b.use_var(sym.var);
        let offset = (8 + idx * 8) as i32;
        let rv = self.lower_expr_for_binding(b, rhs, env)?;
        if is_owning(&fty) {
            let old = self.load_cval(b, &fty, ptr, offset);
            let oldv = cval_value(&old);
            self.emit_decref(b, &fty, oldv)?;
        }
        self.store_cval(b, &rv, ptr, offset);
        Ok(rv)
    }

    fn lower_expr_bool(&mut self, b: &mut FunctionBuilder, e: &Expr, env: &mut Env) -> Result<Value, String> {
        match self.lower_expr(b, e, env)? {
            CVal::Bool(v) => Ok(v),
            _ => Err("M2 codegen: শর্ত অবশ্যই 'সত্যতা' টাইপ হতে হবে".into()),
        }
    }

    fn lower_expr_num(&mut self, b: &mut FunctionBuilder, e: &Expr, env: &mut Env) -> Result<Value, String> {
        match self.lower_expr(b, e, env)? {
            CVal::Num(v) => Ok(v),
            _ => Err("M2 codegen: এখানে 'সংখ্যা' টাইপ প্রত্যাশিত".into()),
        }
    }

    fn lower_expr_dec(&mut self, b: &mut FunctionBuilder, e: &Expr, env: &mut Env) -> Result<Value, String> {
        match self.lower_expr(b, e, env)? {
            CVal::Dec(v) => Ok(v),
            _ => Err("M3 codegen: এখানে 'দশমিক' টাইপ প্রত্যাশিত".into()),
        }
    }

    fn lower_expr_txt(&mut self, b: &mut FunctionBuilder, e: &Expr, env: &mut Env) -> Result<Value, String> {
        match self.lower_expr(b, e, env)? {
            CVal::Txt(v) => Ok(v),
            _ => Err("M3 codegen: এখানে 'লেখা' টাইপ প্রত্যাশিত".into()),
        }
    }

    fn lower_expr_arr(&mut self, b: &mut FunctionBuilder, e: &Expr, env: &mut Env) -> Result<Value, String> {
        match self.lower_expr(b, e, env)? {
            CVal::Arr(v, _) => Ok(v),
            _ => Err("M3 codegen: এখানে array টাইপ প্রত্যাশিত".into()),
        }
    }

    fn lower_stdlib_call(&mut self, b: &mut FunctionBuilder, module: &str, item: &str, args: &[Expr], env: &mut Env) -> Result<CVal, String> {
        macro_rules! num1 {
            ($rt:expr) => {{
                let a = self.lower_expr_num(b, &args[0], env)?;
                Ok(CVal::Num(self.call_rt(b, $rt, &[a])))
            }};
        }
        macro_rules! dec1 {
            ($rt:expr) => {{
                let a = self.lower_expr_dec(b, &args[0], env)?;
                Ok(CVal::Dec(self.call_rt(b, $rt, &[a])))
            }};
        }
        macro_rules! dec1_num {
            ($rt:expr) => {{
                let a = self.lower_expr_dec(b, &args[0], env)?;
                Ok(CVal::Num(self.call_rt(b, $rt, &[a])))
            }};
        }
        macro_rules! txt1_txt {
            ($rt:expr) => {{
                let a = self.lower_expr_txt(b, &args[0], env)?;
                Ok(CVal::Txt(self.call_rt(b, $rt, &[a])))
            }};
        }
        match (module, item) {
            ("গণিত", "পরম") => num1!("kl_math_abs"),
            ("গণিত", "পরমদ") => dec1!("kl_math_fabs"),
            ("গণিত", "বর্গমূল") => dec1!("kl_math_sqrt"),
            ("গণিত", "বেস") => dec1_num!("kl_math_floor"),
            ("গণিত", "আপার") => dec1_num!("kl_math_ceil"),
            ("গণিত", "রাউন্ডঅফ") => dec1_num!("kl_math_round"),
            ("গণিত", "সাইন") => dec1!("kl_math_sin"),
            ("গণিত", "কোসাইন") => dec1!("kl_math_cos"),
            ("গণিত", "ট্যান") => dec1!("kl_math_tan"),
            ("গণিত", "লগ") => dec1!("kl_math_ln"),
            ("গণিত", "লগ১০") => dec1!("kl_math_log10"),
            ("গণিত", "শক্তি") => {
                let a = self.lower_expr_dec(b, &args[0], env)?;
                let c = self.lower_expr_dec(b, &args[1], env)?;
                Ok(CVal::Dec(self.call_rt(b, "kl_math_pow", &[a, c])))
            }
            ("গণিত", "ছোটসংখ্যা") => {
                let a = self.lower_expr_num(b, &args[0], env)?;
                let c = self.lower_expr_num(b, &args[1], env)?;
                Ok(CVal::Num(self.call_rt(b, "kl_math_min_i", &[a, c])))
            }
            ("গণিত", "বড়সংখ্যা") => {
                let a = self.lower_expr_num(b, &args[0], env)?;
                let c = self.lower_expr_num(b, &args[1], env)?;
                Ok(CVal::Num(self.call_rt(b, "kl_math_max_i", &[a, c])))
            }
            ("গণিত", "ছোটদশমিক") => {
                let a = self.lower_expr_dec(b, &args[0], env)?;
                let c = self.lower_expr_dec(b, &args[1], env)?;
                Ok(CVal::Dec(self.call_rt(b, "kl_math_min_f", &[a, c])))
            }
            ("গণিত", "বড়দশমিক") => {
                let a = self.lower_expr_dec(b, &args[0], env)?;
                let c = self.lower_expr_dec(b, &args[1], env)?;
                Ok(CVal::Dec(self.call_rt(b, "kl_math_max_f", &[a, c])))
            }

            ("লেখা", "বড়হাতের") => txt1_txt!("kl_str_upper"),
            ("লেখা", "ছোটহাতের") => txt1_txt!("kl_str_lower"),
            ("লেখা", "ছাঁটো") => txt1_txt!("kl_str_trim"),
            ("লেখা", "সেপারেট") => {
                let a = self.lower_expr_txt(b, &args[0], env)?;
                let sep = self.lower_expr_txt(b, &args[1], env)?;
                Ok(CVal::Arr(self.call_rt(b, "kl_str_split", &[a, sep]), Box::new(Ty::Txt)))
            }
            ("লেখা", "জুড়াও") => {
                let a = self.lower_expr_arr(b, &args[0], env)?;
                let sep = self.lower_expr_txt(b, &args[1], env)?;
                Ok(CVal::Txt(self.call_rt(b, "kl_str_join", &[a, sep])))
            }
            ("লেখা", "বদলাও") => {
                let a = self.lower_expr_txt(b, &args[0], env)?;
                let from = self.lower_expr_txt(b, &args[1], env)?;
                let to = self.lower_expr_txt(b, &args[2], env)?;
                Ok(CVal::Txt(self.call_rt(b, "kl_str_replace", &[a, from, to])))
            }
            ("লেখা", "খুঁজো") => {
                let a = self.lower_expr_txt(b, &args[0], env)?;
                let n = self.lower_expr_txt(b, &args[1], env)?;
                Ok(CVal::Num(self.call_rt(b, "kl_str_find", &[a, n])))
            }
            ("লেখা", "স্লাইস") => {
                let a = self.lower_expr_txt(b, &args[0], env)?;
                let start = self.lower_expr_num(b, &args[1], env)?;
                let end = self.lower_expr_num(b, &args[2], env)?;
                Ok(CVal::Txt(self.call_rt(b, "kl_str_slice", &[a, start, end])))
            }
            ("লেখা", "শুরুতে_আছে") => {
                let a = self.lower_expr_txt(b, &args[0], env)?;
                let pre = self.lower_expr_txt(b, &args[1], env)?;
                Ok(CVal::Bool(self.call_rt(b, "kl_str_starts_with", &[a, pre])))
            }
            ("লেখা", "শেষে_আছে") => {
                let a = self.lower_expr_txt(b, &args[0], env)?;
                let suf = self.lower_expr_txt(b, &args[1], env)?;
                Ok(CVal::Bool(self.call_rt(b, "kl_str_ends_with", &[a, suf])))
            }

            ("ফাইল", "লেখো") => {
                let p = self.lower_expr_txt(b, &args[0], env)?;
                let c = self.lower_expr_txt(b, &args[1], env)?;
                self.call_rt_void(b, "kl_io_write_file", &[p, c]);
                Ok(CVal::Void)
            }
            ("ফাইল", "যোগ") => {
                let p = self.lower_expr_txt(b, &args[0], env)?;
                let c = self.lower_expr_txt(b, &args[1], env)?;
                self.call_rt_void(b, "kl_io_append_file", &[p, c]);
                Ok(CVal::Void)
            }
            ("ফাইল", "পড়ো") => txt1_txt!("kl_io_read_file"),

            ("র‍্যান্ডম", "বীজ") => {
                let s = self.lower_expr_num(b, &args[0], env)?;
                self.call_rt_void(b, "kl_rand_seed", &[s]);
                Ok(CVal::Void)
            }
            ("র‍্যান্ডম", "সংখ্যা") => Ok(CVal::Num(self.call_rt(b, "kl_rand_num", &[]))),
            ("র‍্যান্ডম", "দশমিক") => Ok(CVal::Dec(self.call_rt(b, "kl_rand_dec", &[]))),
            ("র‍্যান্ডম", "মধ্যে") => {
                let lo = self.lower_expr_num(b, &args[0], env)?;
                let hi = self.lower_expr_num(b, &args[1], env)?;
                Ok(CVal::Num(self.call_rt(b, "kl_rand_between", &[lo, hi])))
            }

            ("ফাইলসিস্টেম", "ফাইল_আছে") => {
                let p = self.lower_expr_txt(b, &args[0], env)?;
                Ok(CVal::Bool(self.call_rt(b, "kl_fs_exists", &[p])))
            }
            ("ফাইলসিস্টেম", "ডিরেক্টরি_আছে") => {
                let p = self.lower_expr_txt(b, &args[0], env)?;
                Ok(CVal::Bool(self.call_rt(b, "kl_fs_dir_exists", &[p])))
            }
            ("ফাইলসিস্টেম", "ডিরেক্টরি_বানাও") => {
                let p = self.lower_expr_txt(b, &args[0], env)?;
                self.call_rt_void(b, "kl_fs_mkdir", &[p]);
                Ok(CVal::Void)
            }
            ("ফাইলসিস্টেম", "মুছো") => {
                let p = self.lower_expr_txt(b, &args[0], env)?;
                self.call_rt_void(b, "kl_fs_remove", &[p]);
                Ok(CVal::Void)
            }
            ("ফাইলসিস্টেম", "তালিকা") => {
                let p = self.lower_expr_txt(b, &args[0], env)?;
                Ok(CVal::Arr(self.call_rt(b, "kl_fs_list", &[p]), Box::new(Ty::Txt)))
            }
            ("ফাইলসিস্টেম", "কপি") => {
                let s = self.lower_expr_txt(b, &args[0], env)?;
                let d = self.lower_expr_txt(b, &args[1], env)?;
                self.call_rt_void(b, "kl_fs_copy", &[s, d]);
                Ok(CVal::Void)
            }
            ("ফাইলসিস্টেম", "সরাও") => {
                let s = self.lower_expr_txt(b, &args[0], env)?;
                let d = self.lower_expr_txt(b, &args[1], env)?;
                self.call_rt_void(b, "kl_fs_rename", &[s, d]);
                Ok(CVal::Void)
            }

            ("জেসন", "বৈধকি") => {
                let t = self.lower_expr_txt(b, &args[0], env)?;
                Ok(CVal::Bool(self.call_rt(b, "kl_json_valid", &[t])))
            }
            ("জেসন", "বের_হও") => txt1_txt!("kl_json_escape"),
            ("জেসন", "লেখা_বের_করো") => {
                let t = self.lower_expr_txt(b, &args[0], env)?;
                let k = self.lower_expr_txt(b, &args[1], env)?;
                Ok(CVal::Txt(self.call_rt(b, "kl_json_get_str", &[t, k])))
            }
            ("জেসন", "সংখ্যা_বের_করো") => {
                let t = self.lower_expr_txt(b, &args[0], env)?;
                let k = self.lower_expr_txt(b, &args[1], env)?;
                Ok(CVal::Num(self.call_rt(b, "kl_json_get_num", &[t, k])))
            }

            ("গ্রাফিক্স", "রঙ") => {
                let r = self.lower_expr_num(b, &args[0], env)?;
                let g = self.lower_expr_num(b, &args[1], env)?;
                let bl = self.lower_expr_num(b, &args[2], env)?;
                self.call_rt_void(b, "kl_g_color", &[r, g, bl]);
                Ok(CVal::Void)
            }
            ("গ্রাফিক্স", "বিন্দু") => {
                let x = self.lower_expr_num(b, &args[0], env)?;
                let y = self.lower_expr_num(b, &args[1], env)?;
                self.call_rt_void(b, "kl_g_pixel", &[x, y]);
                Ok(CVal::Void)
            }
            ("গ্রাফিক্স", "রেখা") => {
                let a0 = self.lower_expr_num(b, &args[0], env)?;
                let a1 = self.lower_expr_num(b, &args[1], env)?;
                let a2 = self.lower_expr_num(b, &args[2], env)?;
                let a3 = self.lower_expr_num(b, &args[3], env)?;
                self.call_rt_void(b, "kl_g_line", &[a0, a1, a2, a3]);
                Ok(CVal::Void)
            }
            ("গ্রাফিক্স", "আয়ত") | ("গ্রাফিক্স", "ভরাট_আয়ত") => {
                let a0 = self.lower_expr_num(b, &args[0], env)?;
                let a1 = self.lower_expr_num(b, &args[1], env)?;
                let a2 = self.lower_expr_num(b, &args[2], env)?;
                let a3 = self.lower_expr_num(b, &args[3], env)?;
                let rt = if item == "আয়ত" { "kl_g_rect" } else { "kl_g_fillrect" };
                self.call_rt_void(b, rt, &[a0, a1, a2, a3]);
                Ok(CVal::Void)
            }
            ("গ্রাফিক্স", "বৃত্ত") | ("গ্রাফিক্স", "ভরাট_বৃত্ত") => {
                let a0 = self.lower_expr_num(b, &args[0], env)?;
                let a1 = self.lower_expr_num(b, &args[1], env)?;
                let a2 = self.lower_expr_num(b, &args[2], env)?;
                let rt = if item == "বৃত্ত" { "kl_g_circle" } else { "kl_g_fillcircle" };
                self.call_rt_void(b, rt, &[a0, a1, a2]);
                Ok(CVal::Void)
            }
            ("গ্রাফিক্স", "লেখা") => {
                let x = self.lower_expr_num(b, &args[0], env)?;
                let y = self.lower_expr_num(b, &args[1], env)?;
                let t = self.lower_expr_txt(b, &args[2], env)?;
                self.call_rt_void(b, "kl_g_text", &[x, y, t]);
                Ok(CVal::Void)
            }
            ("গ্রাফিক্স", "ফন্ট") => {
                let name = self.lower_expr_txt(b, &args[0], env)?;
                let size = self.lower_expr_num(b, &args[1], env)?;
                self.call_rt_void(b, "kl_g_font", &[name, size]);
                Ok(CVal::Void)
            }
            ("গ্রাফিক্স", "টিক") => {
                let ms = self.lower_expr_num(b, &args[0], env)?;
                let ExprKind::Ident(id) = &args[1].kind else {
                    return Err("M4 codegen: গ্রাফিক্স.টিক()-এর হ্যান্ডলার একটি ফাংশনের নাম হতে হবে".into());
                };
                let info = self.funcs.get(&id.name).cloned().ok_or_else(|| format!("অজানা ফাংশন '{}'", id.name))?;
                let fref = self.module.declare_func_in_func(info.id, b.func);
                let addr = b.ins().func_addr(self.ptr_ty, fref);
                self.call_rt_void(b, "kl_ui_tick", &[ms, addr]);
                Ok(CVal::Void)
            }

            // নেটওয়ার্ক — TCP client. Sockets live in a registry inside
            // kolom-runtime; Kolom sees only integer handles.
            ("নেটওয়ার্ক", "কানেক্ট") => {
                let host = self.lower_expr_txt(b, &args[0], env)?;
                let port = self.lower_expr_num(b, &args[1], env)?;
                Ok(CVal::Num(self.call_rt(b, "kl_net_connect", &[host, port])))
            }
            ("নেটওয়ার্ক", "সেন্ড") => {
                let h = self.lower_expr_num(b, &args[0], env)?;
                let data = self.lower_expr_txt(b, &args[1], env)?;
                self.call_rt_void(b, "kl_net_send", &[h, data]);
                Ok(CVal::Void)
            }
            ("নেটওয়ার্ক", "রিসিভ") => {
                let h = self.lower_expr_num(b, &args[0], env)?;
                let max = self.lower_expr_num(b, &args[1], env)?;
                Ok(CVal::Txt(self.call_rt(b, "kl_net_recv", &[h, max])))
            }
            ("নেটওয়ার্ক", "ক্লোজ") => {
                let h = self.lower_expr_num(b, &args[0], env)?;
                self.call_rt_void(b, "kl_net_close", &[h]);
                Ok(CVal::Void)
            }

            _ => Err(format!(
                "M4 codegen: '{}.{}' এখনো সমর্থিত নয় (গ্রাফিক্স UI ইঞ্জিনের অংশ)",
                module, item
            )),
        }
    }

    fn lower_let(&mut self, b: &mut FunctionBuilder, name: &Ident, ty_hint: Option<&TypeExpr>, init: &Expr, env: &mut Env) -> Result<(), String> {
        // `ম্যাপ_তৈরি()` carries no type info of its own — unlike array
        // literals (inferred from their elements), an empty map's key/value
        // types can only come from the `ধরি x: ম্যাপ[K, V] = ...` annotation.
        if is_map_new_call(init) {
            let TypeExpr::Map(k, val) = ty_hint.ok_or("M3 codegen: 'ম্যাপ_তৈরি()' ব্যবহারে টাইপ টীকা আবশ্যক, যেমন `ধরি x: ম্যাপ[লেখা, সংখ্যা] = ...`")? else {
                return Err("M3 codegen: 'ম্যাপ_তৈরি()'-এর টাইপ টীকা অবশ্যই 'ম্যাপ[K, V]' হতে হবে".into());
            };
            let v = self.lower_map_new(b, &resolve_type(k), &resolve_type(val))?;
            let ty = cval_ty(&v);
            let cty = self.clif_ty_of(&ty);
            let var = b.declare_var(cty);
            b.def_var(var, cval_value(&v));
            env.insert(name.name.clone(), Sym { ty, var });
            return Ok(());
        }
        let v = self.lower_expr_for_binding(b, init, env)?;
        let ty = cval_ty(&v);
        if matches!(ty, Ty::Null) {
            return Err("M2 codegen: ফাঁকা মান ভ্যারিয়েবলে বসানো যায় না".into());
        }
        let cty = self.clif_ty_of(&ty);
        let var = b.declare_var(cty);
        b.def_var(var, cval_value(&v));
        env.insert(name.name.clone(), Sym { ty, var });
        Ok(())
    }

    fn decref_scope(&mut self, b: &mut FunctionBuilder, scope: &HashMap<String, Sym>) -> Result<(), String> {
        for sym in scope.values() {
            if is_owning(&sym.ty) {
                let v = b.use_var(sym.var);
                self.emit_decref(b, &sym.ty, v)?;
            }
        }
        Ok(())
    }

    fn lower_stmts(&mut self, b: &mut FunctionBuilder, stmts: &[Stmt], env: &mut Env, loops: &mut Vec<LoopCtx>) -> Result<bool, String> {
        env.push();
        let mut term = false;
        for s in stmts {
            if term {
                break;
            }
            term = self.lower_stmt(b, s, env, loops)?;
        }
        let scope = env.pop_scope();
        if !term {
            self.decref_scope(b, &scope)?;
        }
        Ok(term)
    }

    fn lower_stmt(&mut self, b: &mut FunctionBuilder, s: &Stmt, env: &mut Env, loops: &mut Vec<LoopCtx>) -> Result<bool, String> {
        match s {
            Stmt::Var(v) => {
                self.lower_let(b, &v.name, v.ty.as_ref(), &v.init, env)?;
                Ok(false)
            }
            Stmt::Const(c) => {
                self.lower_let(b, &c.name, Some(&c.ty), &c.init, env)?;
                Ok(false)
            }
            Stmt::Expr(e) => {
                self.lower_expr(b, e, env)?;
                Ok(false)
            }
            Stmt::Nested(blk) => self.lower_stmts(b, &blk.stmts, env, loops),
            Stmt::If(ifs) => self.lower_if(b, ifs, env, loops),
            Stmt::While(w) => self.lower_while(b, w, env, loops),
            Stmt::Loop(l) => self.lower_count_loop(b, l, env, loops),
            Stmt::ForEach(fe) => self.lower_foreach(b, fe, env, loops),
            Stmt::Return(r) => {
                match &r.value {
                    Some(e) => {
                        let v = self.lower_expr(b, e, env)?;
                        match &v {
                            CVal::Void => {
                                b.ins().return_(&[]);
                            }
                            _ => {
                                let val = cval_value(&v);
                                b.ins().return_(&[val]);
                            }
                        }
                    }
                    None => {
                        b.ins().return_(&[]);
                    }
                }
                Ok(true)
            }
            Stmt::Break(_) => {
                let ctx = loops.last().ok_or("M2 codegen: break লুপের বাইরে ব্যবহার করা যায় না")?;
                b.ins().jump(ctx.break_block, &[]);
                Ok(true)
            }
            Stmt::Continue(_) => {
                let ctx = loops.last().ok_or("M2 codegen: continue লুপের বাইরে ব্যবহার করা যায় না")?;
                b.ins().jump(ctx.continue_block, &[]);
                Ok(true)
            }
            // `ডিসপ্লে { ... }` inside the app body: the widget tree is
            // (re)built by a separate generated `kl_build_ui` function, so
            // here it's a no-op — see `lower_display_body`/`generate_build_ui`.
            Stmt::Display(_) => Ok(false),
            Stmt::Widget(w) => self.lower_widget(b, w, env, loops).map(|_| false),
            other => Err(format!("M4 codegen: এই statement এখনো সমর্থিত নয়: {:?}", other)),
        }
    }

    /// Lowers one `ডিসপ্লে` widget node into `kl_ui_*` runtime calls.
    /// Container widgets (সারি/কলাম/কার্ড/ডায়ালগ/স্ক্রল) push/pop the
    /// runtime's build stack around their children.
    fn lower_widget(&mut self, b: &mut FunctionBuilder, w: &WidgetNode, env: &mut Env, loops: &mut Vec<LoopCtx>) -> Result<(), String> {
        let container_kind = match w.kw.as_str() {
            "সারি" => Some("kl_ui_row_kind"),
            "কলাম" => Some("kl_ui_col_kind"),
            "কার্ড" => Some("kl_ui_card_kind"),
            "ডায়ালগ" => Some("kl_ui_dialog_kind"),
            "স্ক্রল" => Some("kl_ui_scroll_kind"),
            _ => None,
        };
        if let Some(kind_fn) = container_kind {
            let kind = self.call_rt(b, kind_fn, &[]);
            self.call_rt_void(b, "kl_ui_push", &[kind]);
            if let Some(body) = &w.body {
                for s in &body.stmts {
                    self.lower_stmt(b, s, env, loops)?;
                }
            }
            self.call_rt_void(b, "kl_ui_pop", &[]);
            return Ok(());
        }

        match w.kw.as_str() {
            "টেক্সট" => {
                let t = self.lower_expr_txt(b, w.args.first().ok_or("M4 codegen: টেক্সট() একটি আর্গুমেন্ট নেয়")?, env)?;
                self.call_rt_void(b, "kl_ui_text", &[t]);
            }
            "বাটন" => {
                let t = self.lower_expr_txt(b, w.args.first().ok_or("M4 codegen: বাটন() অন্তত একটি আর্গুমেন্ট নেয়")?, env)?;
                // Optional 2nd arg is a bare function name used as a click
                // handler — Kolom has no first-class functions, so this is
                // resolved statically to that function's address here.
                let handler = match w.args.get(1) {
                    Some(h) => {
                        let ExprKind::Ident(id) = &h.kind else {
                            return Err("M4 codegen: বাটন()-এর হ্যান্ডলার একটি ফাংশনের নাম হতে হবে".into());
                        };
                        let info = self.funcs.get(&id.name).cloned().ok_or_else(|| format!("অজানা ফাংশন '{}'", id.name))?;
                        let fref = self.module.declare_func_in_func(info.id, b.func);
                        b.ins().func_addr(self.ptr_ty, fref)
                    }
                    None => b.ins().iconst(self.ptr_ty, 0),
                };
                self.call_rt_void(b, "kl_ui_button", &[t, handler]);
            }
            "ইনপুট" => self.call_rt_void(b, "kl_ui_input", &[]),
            "ক্যানভাস" => {
                let cw = self.lower_expr_num(b, w.args.first().ok_or("M4 codegen: ক্যানভাস() দুটি আর্গুমেন্ট নেয়")?, env)?;
                let ch = self.lower_expr_num(b, w.args.get(1).ok_or("M4 codegen: ক্যানভাস() দুটি আর্গুমেন্ট নেয়")?, env)?;
                self.call_rt_void(b, "kl_ui_canvas", &[cw, ch]);
            }
            "ছবি" => {
                let p = self.lower_expr_txt(b, w.args.first().ok_or("M4 codegen: ছবি() একটি আর্গুমেন্ট নেয়")?, env)?;
                self.call_rt_void(b, "kl_ui_image", &[p]);
            }
            other => return Err(format!("M4 codegen: '{}' উইজেট এখনো সমর্থিত নয়", other)),
        }
        Ok(())
    }

    fn lower_if(&mut self, b: &mut FunctionBuilder, ifs: &IfStmt, env: &mut Env, loops: &mut Vec<LoopCtx>) -> Result<bool, String> {
        let cond = self.lower_expr_bool(b, &ifs.cond, env)?;
        let then_blk = b.create_block();
        let merge_blk = b.create_block();
        let else_blk = if ifs.els.is_some() { b.create_block() } else { merge_blk };
        b.ins().brif(cond, then_blk, &[], else_blk, &[]);

        b.switch_to_block(then_blk);
        b.seal_block(then_blk);
        let then_term = self.lower_stmts(b, &ifs.then.stmts, env, loops)?;
        if !then_term {
            b.ins().jump(merge_blk, &[]);
        }

        if let Some(els) = &ifs.els {
            b.switch_to_block(else_blk);
            b.seal_block(else_blk);
            let else_term = match els {
                ElseBranch::Block(blk) => self.lower_stmts(b, &blk.stmts, env, loops)?,
                ElseBranch::If(inner) => self.lower_if(b, inner, env, loops)?,
            };
            if !else_term {
                b.ins().jump(merge_blk, &[]);
            }
        }

        b.switch_to_block(merge_blk);
        b.seal_block(merge_blk);
        Ok(false)
    }

    fn lower_while(&mut self, b: &mut FunctionBuilder, w: &WhileStmt, env: &mut Env, loops: &mut Vec<LoopCtx>) -> Result<bool, String> {
        let header = b.create_block();
        let body_blk = b.create_block();
        let merge = b.create_block();
        b.ins().jump(header, &[]);

        b.switch_to_block(header);
        let cond = self.lower_expr_bool(b, &w.cond, env)?;
        b.ins().brif(cond, body_blk, &[], merge, &[]);

        b.switch_to_block(body_blk);
        b.seal_block(body_blk);
        loops.push(LoopCtx { continue_block: header, break_block: merge });
        let term = self.lower_stmts(b, &w.body.stmts, env, loops)?;
        loops.pop();
        if !term {
            b.ins().jump(header, &[]);
        }
        b.seal_block(header);

        b.switch_to_block(merge);
        b.seal_block(merge);
        Ok(false)
    }

    fn lower_count_loop(&mut self, b: &mut FunctionBuilder, l: &LoopStmt, env: &mut Env, loops: &mut Vec<LoopCtx>) -> Result<bool, String> {
        let count_val = self.lower_expr_num(b, &l.count, env)?;
        let counter = b.declare_var(types::I64);
        let zero = b.ins().iconst(types::I64, 0);
        b.def_var(counter, zero);

        let header = b.create_block();
        let body_blk = b.create_block();
        let incr_blk = b.create_block();
        let merge = b.create_block();
        b.ins().jump(header, &[]);

        b.switch_to_block(header);
        let cur = b.use_var(counter);
        let cond = b.ins().icmp(IntCC::SignedLessThan, cur, count_val);
        b.ins().brif(cond, body_blk, &[], merge, &[]);

        b.switch_to_block(body_blk);
        b.seal_block(body_blk);
        loops.push(LoopCtx { continue_block: incr_blk, break_block: merge });
        let term = self.lower_stmts(b, &l.body.stmts, env, loops)?;
        loops.pop();
        if !term {
            b.ins().jump(incr_blk, &[]);
        }

        b.switch_to_block(incr_blk);
        b.seal_block(incr_blk);
        let cur2 = b.use_var(counter);
        let one = b.ins().iconst(types::I64, 1);
        let next = b.ins().iadd(cur2, one);
        b.def_var(counter, next);
        b.ins().jump(header, &[]);
        b.seal_block(header);

        b.switch_to_block(merge);
        b.seal_block(merge);
        Ok(false)
    }

    fn lower_foreach(&mut self, b: &mut FunctionBuilder, fe: &ForEachStmt, env: &mut Env, loops: &mut Vec<LoopCtx>) -> Result<bool, String> {
        let iter_val = self.lower_expr(b, &fe.iter, env)?;
        let (arr_ptr, elem_ty) = match iter_val {
            CVal::Arr(p, t) => (p, *t),
            _ => return Err("M2 codegen: 'প্রতি' শুধু array-এর উপর কাজ করে".into()),
        };
        let lenf = self.module.declare_func_in_func(self.arr_len, b.func);
        let lencall = b.ins().call(lenf, &[arr_ptr]);
        let len_val = b.inst_results(lencall)[0];

        let idx_var = b.declare_var(types::I64);
        let zero = b.ins().iconst(types::I64, 0);
        b.def_var(idx_var, zero);

        let header = b.create_block();
        let body_blk = b.create_block();
        let incr_blk = b.create_block();
        let merge = b.create_block();
        b.ins().jump(header, &[]);

        b.switch_to_block(header);
        let cur = b.use_var(idx_var);
        let cond = b.ins().icmp(IntCC::SignedLessThan, cur, len_val);
        b.ins().brif(cond, body_blk, &[], merge, &[]);

        b.switch_to_block(body_blk);
        b.seal_block(body_blk);
        env.push();
        let getf = self.module.declare_func_in_func(self.arr_get_ptr, b.func);
        let idx_read = b.use_var(idx_var);
        let getcall = b.ins().call(getf, &[arr_ptr, idx_read]);
        let elem_addr = b.inst_results(getcall)[0];
        let elem_cval = self.load_cval(b, &elem_ty, elem_addr, 0);
        if is_owning(&elem_ty) {
            // Elements are owned by the array; the loop variable borrows a
            // reference for the duration of its own scope, symmetric with
            // the decref below.
            self.emit_incref(b, cval_value(&elem_cval));
        }
        let loop_cty = self.clif_ty_of(&elem_ty);
        let loop_var = b.declare_var(loop_cty);
        b.def_var(loop_var, cval_value(&elem_cval));
        env.insert(fe.var.name.clone(), Sym { ty: elem_ty, var: loop_var });

        loops.push(LoopCtx { continue_block: incr_blk, break_block: merge });
        let term = self.lower_stmts(b, &fe.body.stmts, env, loops)?;
        loops.pop();
        let loopvar_scope = env.pop_scope();
        if !term {
            self.decref_scope(b, &loopvar_scope)?;
            b.ins().jump(incr_blk, &[]);
        }

        b.switch_to_block(incr_blk);
        b.seal_block(incr_blk);
        let cur2 = b.use_var(idx_var);
        let one = b.ins().iconst(types::I64, 1);
        let next = b.ins().iadd(cur2, one);
        b.def_var(idx_var, next);
        b.ins().jump(header, &[]);
        b.seal_block(header);

        b.switch_to_block(merge);
        b.seal_block(merge);
        Ok(false)
    }

    fn zero_return(&mut self, b: &mut FunctionBuilder, ret: &Ty) -> Result<(), String> {
        match ret {
            Ty::Null => {
                b.ins().return_(&[]);
            }
            Ty::Num => {
                let z = b.ins().iconst(types::I64, 0);
                b.ins().return_(&[z]);
            }
            Ty::Dec => {
                let z = b.ins().f64const(0.0);
                b.ins().return_(&[z]);
            }
            Ty::Bool => {
                let z = b.ins().iconst(types::I8, 0);
                b.ins().return_(&[z]);
            }
            Ty::Txt | Ty::Arr(_) | Ty::Shared(_) | Ty::Struct(_) => {
                let z = b.ins().iconst(self.ptr_ty, 0);
                b.ins().return_(&[z]);
            }
            other => return Err(format!("M2 codegen: রিটার্ন টাইপ '{}' এখনো সমর্থিত নয়", other)),
        }
        Ok(())
    }

    fn lower_func(&mut self, name: &str, params: &[Param], body: &Block) -> Result<(), String> {
        let info = self.funcs.get(name).cloned().expect("function was pre-declared");
        let sig = self.make_signature(&info.params, &info.ret)?;
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut fbx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            b.seal_block(entry);
            let block_params: Vec<Value> = b.block_params(entry).to_vec();

            let mut env = Env::new();
            env.push();
            let mut pidx = 0usize;
            for (p, pty) in params.iter().zip(info.params.iter()) {
                let sym = self.bind_param(&mut b, pty, &block_params, &mut pidx);
                env.insert(p.name.name.clone(), sym);
            }

            let mut loops: Vec<LoopCtx> = Vec::new();
            let term = self.lower_stmts(&mut b, &body.stmts, &mut env, &mut loops)?;
            env.pop_scope();
            if !term {
                self.zero_return(&mut b, &info.ret)?;
            }
            let cfg = self.module.target_config();
            b.finalize(cfg);
        }
        self.module.define_function(info.id, &mut ctx).map_err(|e| e.to_string())?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    /// Emits `kl_build_ui()`, which clears and re-populates the runtime's
    /// widget tree from the app's `ডিসপ্লে` block. Registered with the
    /// runtime as the rebuild hook, so state changes from a button/tick
    /// handler re-run this and repaint (`engine.md`'s rebuild cycle).
    fn generate_build_ui(&mut self, build_id: FuncId, display: &Block) -> Result<(), String> {
        let sig = self.module.make_signature();
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut fbx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbx);
            let entry = b.create_block();
            b.switch_to_block(entry);
            b.seal_block(entry);
            let mut env = Env::new();
            env.push();
            let mut loops: Vec<LoopCtx> = Vec::new();
            self.call_rt_void(&mut b, "kl_ui_begin", &[]);
            for s in &display.stmts {
                self.lower_stmt(&mut b, s, &mut env, &mut loops)?;
            }
            env.pop_scope();
            b.ins().return_(&[]);
            let cfg = self.module.target_config();
            b.finalize(cfg);
        }
        self.module.define_function(build_id, &mut ctx).map_err(|e| e.to_string())?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    fn lower_main(&mut self, main_id: FuncId, app: &AppDecl, build_ui: Option<FuncId>) -> Result<(), String> {
        let mut sig = self.module.make_signature();
        sig.returns.push(AbiParam::new(types::I32));
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut fbx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbx);
            let entry = b.create_block();
            b.switch_to_block(entry);
            b.seal_block(entry);
            let mut env = Env::new();
            env.push();
            let mut loops: Vec<LoopCtx> = Vec::new();

            // A UI app opens its window BEFORE running the app body, because
            // body statements like `গ্রাফিক্স.টিক(...)` install timers on
            // that window.
            if build_ui.is_some() {
                let title = app.name.as_ref().map(|n| n.name.clone()).unwrap_or_else(|| "কলম".to_string());
                let title_val = self.make_str(&mut b, title.as_bytes())?;
                let tv = cval_value(&title_val);
                self.call_rt_void(&mut b, "kl_ui_init", &[tv]);
            }

            let term = self.lower_stmts(&mut b, &app.body.stmts, &mut env, &mut loops)?;
            env.pop_scope();

            if !term {
                if let Some(build_id) = build_ui {
                    let fref = self.module.declare_func_in_func(build_id, b.func);
                    let addr = b.ins().func_addr(self.ptr_ty, fref);
                    self.call_rt_void(&mut b, "kl_ui_set_rebuild", &[addr]);
                    b.ins().call(fref, &[]);
                    self.call_rt_void(&mut b, "kl_ui_show_and_run", &[]);
                }
                let zero = b.ins().iconst(types::I32, 0);
                b.ins().return_(&[zero]);
            }
            let cfg = self.module.target_config();
            b.finalize(cfg);
        }
        self.module.define_function(main_id, &mut ctx).map_err(|e| e.to_string())?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    fn generate_struct_drop(&mut self, name: &str) -> Result<(), String> {
        let layout = self.structs.get(name).cloned().expect("struct pre-declared");
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(self.ptr_ty));
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut fbx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            b.seal_block(entry);
            let self_ptr = b.block_params(entry)[0];
            for (idx, (_fname, fty)) in layout.fields.iter().enumerate() {
                if is_owning(fty) {
                    let offset = (8 + idx * 8) as i32;
                    let cty = self.clif_ty_of(fty);
                    let fval = b.ins().load(cty, MemFlagsData::trusted(), self_ptr, offset);
                    self.emit_decref(&mut b, fty, fval)?;
                }
            }
            b.ins().return_(&[]);
            let cfg = self.module.target_config();
            b.finalize(cfg);
        }
        self.module.define_function(layout.drop_id, &mut ctx).map_err(|e| e.to_string())?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }
}

fn num_binop(b: &mut FunctionBuilder, op: BinOp, a: Value, c: Value) -> Result<CVal, String> {
    Ok(match op {
        BinOp::Add => CVal::Num(b.ins().iadd(a, c)),
        BinOp::Sub => CVal::Num(b.ins().isub(a, c)),
        BinOp::Mul => CVal::Num(b.ins().imul(a, c)),
        BinOp::Div => CVal::Num(b.ins().sdiv(a, c)),
        BinOp::Mod => CVal::Num(b.ins().srem(a, c)),
        BinOp::Eq => CVal::Bool(b.ins().icmp(IntCC::Equal, a, c)),
        BinOp::Neq => CVal::Bool(b.ins().icmp(IntCC::NotEqual, a, c)),
        BinOp::Lt => CVal::Bool(b.ins().icmp(IntCC::SignedLessThan, a, c)),
        BinOp::Gt => CVal::Bool(b.ins().icmp(IntCC::SignedGreaterThan, a, c)),
        BinOp::Le => CVal::Bool(b.ins().icmp(IntCC::SignedLessThanOrEqual, a, c)),
        BinOp::Ge => CVal::Bool(b.ins().icmp(IntCC::SignedGreaterThanOrEqual, a, c)),
        BinOp::And | BinOp::Or => return Err("M2 codegen: and/or শুধু 'সত্যতা'-এ প্রযোজ্য".into()),
    })
}

fn dec_binop(b: &mut FunctionBuilder, op: BinOp, a: Value, c: Value) -> Result<CVal, String> {
    Ok(match op {
        BinOp::Add => CVal::Dec(b.ins().fadd(a, c)),
        BinOp::Sub => CVal::Dec(b.ins().fsub(a, c)),
        BinOp::Mul => CVal::Dec(b.ins().fmul(a, c)),
        BinOp::Div => CVal::Dec(b.ins().fdiv(a, c)),
        BinOp::Eq => CVal::Bool(b.ins().fcmp(FloatCC::Equal, a, c)),
        BinOp::Neq => CVal::Bool(b.ins().fcmp(FloatCC::NotEqual, a, c)),
        BinOp::Lt => CVal::Bool(b.ins().fcmp(FloatCC::LessThan, a, c)),
        BinOp::Gt => CVal::Bool(b.ins().fcmp(FloatCC::GreaterThan, a, c)),
        BinOp::Le => CVal::Bool(b.ins().fcmp(FloatCC::LessThanOrEqual, a, c)),
        BinOp::Ge => CVal::Bool(b.ins().fcmp(FloatCC::GreaterThanOrEqual, a, c)),
        BinOp::Mod => return Err("M2 codegen: দশমিকের mod এখনো সমর্থিত নয়".into()),
        BinOp::And | BinOp::Or => return Err("M2 codegen: and/or শুধু 'সত্যতা'-এ প্রযোজ্য".into()),
    })
}

fn bool_binop(b: &mut FunctionBuilder, op: BinOp, a: Value, c: Value) -> Result<CVal, String> {
    Ok(match op {
        BinOp::Eq => CVal::Bool(b.ins().icmp(IntCC::Equal, a, c)),
        BinOp::Neq => CVal::Bool(b.ins().icmp(IntCC::NotEqual, a, c)),
        // TODO(M3+): short-circuit evaluation.
        BinOp::And => CVal::Bool(b.ins().band(a, c)),
        BinOp::Or => CVal::Bool(b.ins().bor(a, c)),
        _ => return Err("M2 codegen: এই অপারেটর 'সত্যতা'-এ প্রযোজ্য নয়".into()),
    })
}

fn declare_import(module: &mut ObjectModule, name: &str, params: &[ClifType], rets: &[ClifType]) -> Result<FuncId, String> {
    let mut sig = module.make_signature();
    for p in params {
        sig.params.push(AbiParam::new(*p));
    }
    for r in rets {
        sig.returns.push(AbiParam::new(*r));
    }
    module.declare_function(name, Linkage::Import, &sig).map_err(|e| e.to_string())
}

/// Compiles `prog` to a native object file's bytes. Runs kolom-sema's
/// validator first (mirroring what kolom-cli's `check_program` does before
/// invoking the existing C backend) so this can be called standalone.
pub fn emit(prog: &Program) -> Result<Vec<u8>, String> {
    emit_for(prog, crate::link::Target::host())
}

/// As `emit`, but generating code for an explicit target.
pub fn emit_for(prog: &Program, target: crate::link::Target) -> Result<Vec<u8>, String> {
    let diags = kolom_sema::analyze(prog);
    if !diags.is_empty() {
        let msgs: Vec<String> = diags.iter().map(|d| format!("{}:{}: {}", d.line, d.col, d.message)).collect();
        return Err(format!("সিমান্টিক ত্রুটি:\n{}", msgs.join("\n")));
    }

    let mut flag_builder = settings::builder();
    flag_builder.set("is_pic", "false").map_err(|e| e.to_string())?;
    // Cranelift defaults to `none`, which disables essentially all
    // optimization — measured ~2x slower on compute-bound Kolom code.
    // `কলম বিল্ড` produces release artifacts, so optimize by default.
    flag_builder.set("opt_level", "speed").map_err(|e| e.to_string())?;
    let isa_builder = crate::link::isa_builder(target)?;
    let isa = isa_builder.finish(settings::Flags::new(flag_builder)).map_err(|e| e.to_string())?;
    let obj_builder = ObjectBuilder::new(isa, "kolom_module", cranelift_module::default_libcall_names()).map_err(|e| e.to_string())?;
    let mut module = ObjectModule::new(obj_builder);
    let ptr_ty = module.target_config().pointer_type();
    let i64t = types::I64;

    let print_num = declare_import(&mut module, "kl_print_num", &[i64t], &[])?;
    let print_bool = declare_import(&mut module, "kl_print_bool", &[types::I8], &[])?;
    let print_text = declare_import(&mut module, "kl_print_text", &[ptr_ty], &[])?;
    let num_to_text = declare_import(&mut module, "kl_num_to_text", &[i64t], &[ptr_ty])?;
    let rc_incref = declare_import(&mut module, "kl_rc_incref", &[ptr_ty], &[])?;
    let str_new = declare_import(&mut module, "kl_str_new", &[ptr_ty, i64t], &[ptr_ty])?;
    let str_len = declare_import(&mut module, "kl_str_len", &[ptr_ty], &[i64t])?;
    let str_copy = declare_import(&mut module, "kl_str_copy", &[ptr_ty], &[ptr_ty])?;
    let str_decref = declare_import(&mut module, "kl_str_decref", &[ptr_ty], &[])?;
    let arr_new = declare_import(&mut module, "kl_arr_new", &[i64t, i64t, ptr_ty], &[ptr_ty])?;
    let arr_len = declare_import(&mut module, "kl_arr_len", &[ptr_ty], &[i64t])?;
    let arr_get_ptr = declare_import(&mut module, "kl_arr_get_ptr", &[ptr_ty, i64t], &[ptr_ty])?;
    let arr_push = declare_import(&mut module, "kl_arr_push", &[ptr_ty, ptr_ty], &[])?;
    let arr_concat = declare_import(&mut module, "kl_arr_concat", &[ptr_ty, ptr_ty], &[ptr_ty])?;
    let arr_decref = declare_import(&mut module, "kl_arr_decref", &[ptr_ty], &[])?;
    let shared_new = declare_import(&mut module, "kl_shared_new", &[i64t, ptr_ty], &[ptr_ty])?;
    let shared_payload_ptr = declare_import(&mut module, "kl_shared_payload_ptr", &[ptr_ty], &[ptr_ty])?;
    let shared_decref = declare_import(&mut module, "kl_shared_decref", &[ptr_ty], &[])?;
    let struct_new = declare_import(&mut module, "kl_struct_new", &[i64t], &[ptr_ty])?;
    let struct_decref = declare_import(&mut module, "kl_struct_decref", &[ptr_ty, i64t, ptr_ty], &[])?;

    let f64t = types::F64;
    let i8t = types::I8;
    #[rustfmt::skip]
    let stdlib_imports: &[(&str, &[ClifType], &[ClifType])] = &[
        // গণিত
        ("kl_math_abs", &[i64t], &[i64t]),
        ("kl_math_fabs", &[f64t], &[f64t]),
        ("kl_math_sqrt", &[f64t], &[f64t]),
        ("kl_math_pow", &[f64t, f64t], &[f64t]),
        ("kl_math_floor", &[f64t], &[i64t]),
        ("kl_math_ceil", &[f64t], &[i64t]),
        ("kl_math_round", &[f64t], &[i64t]),
        ("kl_math_sin", &[f64t], &[f64t]),
        ("kl_math_cos", &[f64t], &[f64t]),
        ("kl_math_tan", &[f64t], &[f64t]),
        ("kl_math_ln", &[f64t], &[f64t]),
        ("kl_math_log10", &[f64t], &[f64t]),
        ("kl_math_min_i", &[i64t, i64t], &[i64t]),
        ("kl_math_max_i", &[i64t, i64t], &[i64t]),
        ("kl_math_min_f", &[f64t, f64t], &[f64t]),
        ("kl_math_max_f", &[f64t, f64t], &[f64t]),
        ("kl_dec_to_text", &[f64t], &[ptr_ty]),
        ("kl_bool_to_text", &[i8t], &[ptr_ty]),
        // লেখা
        ("kl_str_concat", &[ptr_ty, ptr_ty], &[ptr_ty]),
        ("kl_str_cplen", &[ptr_ty], &[i64t]),
        ("kl_str_upper", &[ptr_ty], &[ptr_ty]),
        ("kl_str_lower", &[ptr_ty], &[ptr_ty]),
        ("kl_str_trim", &[ptr_ty], &[ptr_ty]),
        ("kl_str_split", &[ptr_ty, ptr_ty], &[ptr_ty]),
        ("kl_str_join", &[ptr_ty, ptr_ty], &[ptr_ty]),
        ("kl_str_replace", &[ptr_ty, ptr_ty, ptr_ty], &[ptr_ty]),
        ("kl_str_find", &[ptr_ty, ptr_ty], &[i64t]),
        ("kl_str_slice", &[ptr_ty, i64t, i64t], &[ptr_ty]),
        ("kl_str_starts_with", &[ptr_ty, ptr_ty], &[i8t]),
        ("kl_str_ends_with", &[ptr_ty, ptr_ty], &[i8t]),
        // ফাইল / ফাইলসিস্টেম
        ("kl_io_write_file", &[ptr_ty, ptr_ty], &[]),
        ("kl_io_append_file", &[ptr_ty, ptr_ty], &[]),
        ("kl_io_read_file", &[ptr_ty], &[ptr_ty]),
        ("kl_io_read_line", &[], &[ptr_ty]),
        ("kl_fs_exists", &[ptr_ty], &[i8t]),
        ("kl_fs_dir_exists", &[ptr_ty], &[i8t]),
        ("kl_fs_mkdir", &[ptr_ty], &[]),
        ("kl_fs_remove", &[ptr_ty], &[]),
        ("kl_fs_copy", &[ptr_ty, ptr_ty], &[]),
        ("kl_fs_rename", &[ptr_ty, ptr_ty], &[]),
        ("kl_fs_list", &[ptr_ty], &[ptr_ty]),
        // র‍্যান্ডম
        ("kl_rand_seed", &[i64t], &[]),
        ("kl_rand_num", &[], &[i64t]),
        ("kl_rand_between", &[i64t, i64t], &[i64t]),
        ("kl_rand_dec", &[], &[f64t]),
        // জেসন
        ("kl_json_valid", &[ptr_ty], &[i8t]),
        ("kl_json_get_str", &[ptr_ty, ptr_ty], &[ptr_ty]),
        ("kl_json_get_num", &[ptr_ty, ptr_ty], &[i64t]),
        ("kl_json_escape", &[ptr_ty], &[ptr_ty]),
        // নেটওয়ার্ক
        ("kl_net_connect", &[ptr_ty, i64t], &[i64t]),
        ("kl_net_send", &[i64t, ptr_ty], &[]),
        ("kl_net_recv", &[i64t, i64t], &[ptr_ty]),
        ("kl_net_close", &[i64t], &[]),
        // ম্যাপ
        ("kl_map_new", &[i64t], &[ptr_ty]),
        ("kl_map_len", &[ptr_ty], &[i64t]),
        ("kl_map_find", &[ptr_ty, i64t], &[ptr_ty]),
        ("kl_map_set_slot", &[ptr_ty, i64t, ptr_ty], &[ptr_ty]),
        ("kl_map_delete", &[ptr_ty, i64t], &[]),
        ("kl_map_keys", &[ptr_ty], &[ptr_ty]),
        ("kl_map_decref", &[ptr_ty], &[]),
        ("kl_map_missing_key", &[], &[]),
        // UI engine (M4)
        ("kl_ui_begin", &[], &[]),
        ("kl_ui_text", &[ptr_ty], &[]),
        ("kl_ui_button", &[ptr_ty, ptr_ty], &[]),
        ("kl_ui_input", &[], &[]),
        ("kl_ui_canvas", &[i64t, i64t], &[]),
        ("kl_ui_image", &[ptr_ty], &[]),
        ("kl_ui_push", &[i64t], &[]),
        ("kl_ui_pop", &[], &[]),
        ("kl_ui_set_rebuild", &[ptr_ty], &[]),
        ("kl_ui_tick", &[i64t, ptr_ty], &[]),
        ("kl_ui_init", &[ptr_ty], &[]),
        ("kl_ui_show_and_run", &[], &[]),
        ("kl_ui_row_kind", &[], &[i64t]),
        ("kl_ui_col_kind", &[], &[i64t]),
        ("kl_ui_card_kind", &[], &[i64t]),
        ("kl_ui_dialog_kind", &[], &[i64t]),
        ("kl_ui_scroll_kind", &[], &[i64t]),
        // গ্রাফিক্স
        ("kl_g_color", &[i64t, i64t, i64t], &[]),
        ("kl_g_pixel", &[i64t, i64t], &[]),
        ("kl_g_line", &[i64t, i64t, i64t, i64t], &[]),
        ("kl_g_rect", &[i64t, i64t, i64t, i64t], &[]),
        ("kl_g_fillrect", &[i64t, i64t, i64t, i64t], &[]),
        ("kl_g_circle", &[i64t, i64t, i64t], &[]),
        ("kl_g_fillcircle", &[i64t, i64t, i64t], &[]),
        ("kl_g_text", &[i64t, i64t, ptr_ty], &[]),
        ("kl_g_font", &[ptr_ty, i64t], &[]),
    ];
    let mut rt = HashMap::new();
    for (name, params, rets) in stdlib_imports {
        rt.insert(*name, declare_import(&mut module, name, params, rets)?);
    }

    let mut gen = Gen {
        module,
        ptr_ty,
        funcs: HashMap::new(),
        structs: HashMap::new(),
        consts: HashMap::new(),
        str_counter: 0,
        print_num,
        print_bool,
        print_text,
        num_to_text,
        rc_incref,
        str_new,
        str_len,
        str_copy,
        str_decref,
        arr_new,
        arr_len,
        arr_get_ptr,
        arr_push,
        arr_concat,
        arr_decref,
        shared_new,
        shared_payload_ptr,
        shared_decref,
        struct_new,
        struct_decref,
        rt,
    };

    for sdecl in &prog.structs {
        let fields: Vec<(String, Ty)> = sdecl.fields.iter().map(|(fid, fte)| (fid.name.clone(), resolve_type(fte))).collect();
        let mut dsig = gen.module.make_signature();
        dsig.params.push(AbiParam::new(gen.ptr_ty));
        let drop_id = gen
            .module
            .declare_function(&mangle_drop(&sdecl.name.name), Linkage::Local, &dsig)
            .map_err(|e| e.to_string())?;
        gen.structs.insert(sdecl.name.name.clone(), StructLayout { fields, drop_id });
    }
    for sdecl in &prog.structs {
        gen.generate_struct_drop(&sdecl.name.name)?;
    }

    for c in &prog.consts {
        gen.consts.insert(c.name.name.clone(), c.init.clone());
    }

    for f in &prog.funcs {
        let params: Vec<Ty> = f.params.iter().map(|p| resolve_type(&p.ty)).collect();
        let ret = resolve_type(&f.ret);
        let sig = gen.make_signature(&params, &ret)?;
        let id = gen
            .module
            .declare_function(&mangle(&f.name.name), Linkage::Local, &sig)
            .map_err(|e| e.to_string())?;
        gen.funcs.insert(f.name.name.clone(), FuncInfo { id, params, ret });
    }

    let mut main_sig = gen.module.make_signature();
    main_sig.returns.push(AbiParam::new(types::I32));
    let main_id = gen.module.declare_function("main", Linkage::Export, &main_sig).map_err(|e| e.to_string())?;

    for f in &prog.funcs {
        gen.lower_func(&f.name.name, &f.params, &f.body)?;
    }

    let app = prog.app.as_ref().ok_or("M4 codegen: `অ্যাপ` ব্লক আবশ্যক")?;

    // A `ডিসপ্লে` block anywhere in the app body makes this a UI program:
    // its widgets move into a generated `kl_build_ui` (re-runnable on state
    // change) and main gains the window/message-loop calls.
    let display = app.body.stmts.iter().find_map(|s| match s {
        Stmt::Display(blk) => Some(blk),
        _ => None,
    });
    let build_ui = match display {
        Some(blk) => {
            let sig = gen.module.make_signature();
            let id = gen.module.declare_function("kl_build_ui", Linkage::Local, &sig).map_err(|e| e.to_string())?;
            gen.generate_build_ui(id, blk)?;
            Some(id)
        }
        None => None,
    };
    gen.lower_main(main_id, app, build_ui)?;

    let product = gen.module.finish();
    product.object.write().map_err(|e| e.to_string())
}
