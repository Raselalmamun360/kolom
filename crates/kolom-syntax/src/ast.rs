use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Named(Ident),
    Array(Box<TypeExpr>),
    Shared(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
    /// `(টাইপ, ...) -> টাইপ` — a function value's type.
    Func(Vec<TypeExpr>, Box<TypeExpr>),
    /// `Name<Arg, ...>` — a generic `তথ্য`/`এনাম` instantiated with concrete
    /// type arguments, e.g. `বাক্স<সংখ্যা>`.
    Generic(Ident, Vec<TypeExpr>),
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: TypeExpr,
    pub name: Ident,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub name: Ident,
    /// `<T, ...>` — empty for an ordinary (non-generic) function.
    pub type_params: Vec<Ident>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub name: Ident,
    pub ty: TypeExpr,
    pub init: Expr,
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub name: Ident,
    pub ty: Option<TypeExpr>,
    pub init: Expr,
}

#[derive(Debug, Clone)]
pub struct AppDecl {
    pub name: Option<Ident>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum ElseBranch {
    If(Box<IfStmt>),
    Block(Block),
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub pos: Pos,
    pub cond: Expr,
    pub then: Block,
    pub els: Option<ElseBranch>,
}

#[derive(Debug, Clone)]
pub struct LoopStmt {
    pub pos: Pos,
    pub count: Expr,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub pos: Pos,
    pub cond: Expr,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct ForEachStmt {
    pub pos: Pos,
    pub var: Ident,
    pub iter: Expr,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub pos: Pos,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct WidgetNode {
    pub kw: String,
    pub pos: Pos,
    pub args: Vec<Expr>,
    pub body: Option<Block>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Var(VarDecl),
    Const(ConstDecl),
    If(IfStmt),
    Loop(LoopStmt),
    While(WhileStmt),
    ForEach(ForEachStmt),
    Return(ReturnStmt),
    Break(Pos),
    Continue(Pos),
    Expr(Expr),
    Nested(Block),
    TryCatch(TryCatchStmt),
    Widget(WidgetNode),
    Display(Block),
}

#[derive(Debug, Clone)]
pub enum Lit {
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    Null,
    Array(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Suffix {
    Call(Vec<Expr>, Pos),
    Index(Box<Expr>, Pos),
    Field(Ident),
}

#[derive(Debug, Clone)]
pub struct LValue {
    pub base: Ident,
    pub idx: Vec<Expr>,
    pub field: Option<Ident>,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Lit(Lit),
    Ident(Ident),
    Qualified {
        module: Ident,
        name: Ident,
    },
    Unary(UnaryOp, Box<Expr>),
    Postfix(Box<Expr>, Vec<Suffix>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(LValue, Box<Expr>),
    FieldAssign(Ident, Ident, Box<Expr>),
    Match(MatchExpr),
}

/// A `মিলাও` arm's pattern. `Variant` binds the scrutinee's payload (if any)
/// into fresh names visible only inside `body`; `Wildcard` (`_`) matches
/// anything and is not itself checked for exhaustiveness (it *provides* it).
#[derive(Debug, Clone)]
pub enum Pattern {
    Variant {
        name: Ident,
        binds: Vec<Ident>,
        pos: Pos,
    },
    Wildcard(Pos),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub struct MatchExpr {
    pub pos: Pos,
    pub scrutinee: Box<Expr>,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: Ident,
    /// `<T, ...>` — empty for an ordinary (non-generic) struct.
    pub type_params: Vec<Ident>,
    pub fields: Vec<(Ident, TypeExpr)>,
}

/// A variant's payload is a positional list of types (Rust
/// tuple-variant-style) — `বৃত্ত(দশমিক)` — never named fields.
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: Ident,
    /// `<T, ...>` — empty for an ordinary (non-generic) enum.
    pub type_params: Vec<Ident>,
    pub variants: Vec<(Ident, Vec<TypeExpr>)>,
}

#[derive(Debug, Clone)]
pub struct TryCatchStmt {
    pub body: Block,
    pub err_var: Ident,
    pub handler: Block,
}

/// One `ফাংশন` signature inside an `এক্সটার্ন` block — no body, since the
/// implementation is native code the final link supplies (already
/// statically linked into every Kolom binary via kolom-runtime, for the
/// symbols the toolchain itself exposes).
#[derive(Debug, Clone)]
pub struct ExternFn {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
}

/// `এক্সটার্ন "C" { ... }` — `abi` is the quoted string (only `"C"` is
/// meaningful today; anything else is a sema error).
#[derive(Debug, Clone)]
pub struct ExternBlock {
    pub abi: String,
    pub pos: Pos,
    pub funcs: Vec<ExternFn>,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub imports: Vec<Ident>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub funcs: Vec<Rc<FuncDecl>>,
    pub externs: Vec<ExternBlock>,
    pub consts: Vec<ConstDecl>,
    pub app: Option<AppDecl>,
}
