use crate::token::Span;

pub type ExprId = u32;
pub type DeclId = u32;

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
    Global(VarDecl),
    Struct(StructDecl),
    Impl(ImplBlock),
    Enum(EnumDecl),
    Use(UseDecl),
    Extern(ExternBlock),
}

/// `extern "C" { fn symbol(a: c_int) -> c_int; }`
#[derive(Debug, Clone)]
pub struct ExternBlock {
    pub abi: String,
    pub abi_span: Span,
    pub functions: Vec<ExternFn>,
}

#[derive(Debug, Clone)]
pub struct ExternFn {
    pub name: String,
    pub name_span: Span,
    /// External symbol name (defaults to `name`; `as` allows an override).
    pub symbol: String,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub is_pub: bool,
    pub span: Span,
    /// Canonical name assigned by the checker.
    pub canonical: String,
    /// Calling convention as resolved by the checker.
    pub is_system_abi: bool,
}

#[derive(Debug, Clone)]
pub struct UseDecl {
    pub path: Vec<String>,
    pub path_span: Span,
    pub is_pub: bool,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<String>,
    pub variants: Vec<EnumVariantDecl>,
    pub span: Span,
    pub is_pub: bool,
    pub canonical: String,
}

#[derive(Debug, Clone)]
pub struct EnumVariantDecl {
    pub name: String,
    pub name_span: Span,
    pub payload: EnumPayload,
}

#[derive(Debug, Clone)]
pub enum EnumPayload {
    Unit,
    Tuple(Vec<TypeExpr>),
    Named(Vec<StructFieldDecl>),
}

#[derive(Debug, Clone)]
pub struct ImplBlock {
    pub params: Vec<String>,
    pub target: TypeExpr,
    pub ty_span: Span,
    pub canonical_type: String,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    Value,
    Mutable,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    pub name_span: Span,
    pub self_id: DeclId,
    pub receiver: Option<ReceiverKind>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Block,
    pub is_pub: bool,
    pub c_name: String,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<String>,
    pub fields: Vec<StructFieldDecl>,
    pub span: Span,
    pub is_pub: bool,
    pub canonical: String,
}

#[derive(Debug, Clone)]
pub struct StructFieldDecl {
    pub name: String,
    pub name_span: Span,
    pub ty: TypeExpr,
}

#[derive(Debug, Clone)]
pub struct StructLitField {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub name_span: Span,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Block,
    pub span: Span,
    pub is_pub: bool,
    pub c_name: String,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub id: DeclId,
    pub name: String,
    pub span: Span,
    pub ty: TypeExpr,
}

#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeExprKind {
    Named(String),
    Generic(String, Vec<TypeExpr>),
    Array(Box<TypeExpr>),
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Let,
    Var,
    Const,
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub id: DeclId,
    pub kind: DeclKind,
    pub name: String,
    pub name_span: Span,
    pub ty: Option<TypeExpr>,
    pub init: Option<Expr>,
    pub span: Span,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl(VarDecl),
    Expr {
        expr: Expr,
        span: Span,
    },
    If {
        cond: Expr,
        then_block: Block,
        else_branch: Option<ElseBranch>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    For {
        decl: VarDecl,
        range_start: Expr,
        range_end: Expr,
        body: Block,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Block(Block),
    /// `unsafe { ... }` — allows calls to `extern` functions.
    Unsafe(Block),
}

#[derive(Debug, Clone)]
pub enum ElseBranch {
    If(Box<Stmt>),
    Block(Block),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub id: ExprId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    Ident(String),
    Path(Vec<String>, Vec<TypeExpr>),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        type_args: Vec<TypeExpr>,
        args: Vec<Expr>,
    },
    Try(Box<Expr>),
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
        name_span: Span,
    },
    StructLit {
        name: String,
        name_span: Span,
        fields: Vec<StructLitField>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    ArrayLit(Vec<Expr>),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wild(Span),
    Binding(String, Span, DeclId),
    Literal(LiteralPattern, Span),
    Or(Vec<Pattern>, Span),
    Variant {
        path: Vec<String>,
        name_span: Span,
        args: Vec<Pattern>,
        named: Vec<NamedPatternField>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralPattern {
    Int(i64),
    Bool(bool),
    Char(char),
    Str(String),
}

#[derive(Debug, Clone)]
pub struct NamedPatternField {
    pub name: String,
    pub name_span: Span,
    pub pattern: Pattern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}
