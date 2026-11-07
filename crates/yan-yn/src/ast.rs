//! Yan syntax tree (spec 15.2). One node per production that carries meaning; spans are
//! 1-based line and UTF-8 byte column.

pub use crate::lexer::{StrKind, TemporalKind};
pub use yan_diag::Span;

pub type Name = String;

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub attrs: Vec<Attr>,
    pub items: Vec<Item>,
    /// `---` doc comments: line and text.
    pub docs: Vec<(u32, String)>,
    /// Comments and blank lines, for the printer; never compared.
    pub trivia: yan_diag::Trivia,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    pub name: Name,
    pub args: Vec<Arg>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub attrs: Vec<Attr>,
    pub public: bool,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Fn(FnDef),
    Type(TypeDef),
    Trait(TraitDef),
    Impl(ImplDef),
    Const(ConstDef),
    Extern(ExternDef),
    Use(UseDecl),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnKind {
    Plain,
    Gen,
    Sub,
    Lemma,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnDef {
    pub kind: FnKind,
    pub sig: Sig,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sig {
    pub name: Name,
    pub name_span: Span,
    pub tparams: Vec<TParam>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    /// `from p, q`: the parameters a returned view borrows from.
    pub from: Vec<Name>,
    pub fail: Option<Type>,
    pub uses: Option<Vec<CapItem>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Expr(Expr),
    Block {
        contracts: Vec<Contract>,
        stmts: Vec<Stmt>,
    },
    /// Clause form: the arms of a match on the parameters (R266, R370).
    Clauses {
        contracts: Vec<Contract>,
        arms: Vec<Arm>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractKind {
    Pre,
    Post,
    Ex,
    Decreases,
    Invariant,
    /// One `steps <= e` bound of a `cost` line (R610).
    CostSteps,
    /// One `alloc <= e` bound of a `cost` line.
    CostAlloc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Contract {
    pub kind: ContractKind,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CapItem {
    /// `io`, or `cache: Kv` for a renamed user capability.
    Named {
        name: Name,
        ty: Option<Name>,
    },
    /// A capability set alias (R431).
    Set(Name),
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TParam {
    Type { name: Name, bounds: Vec<Bound> },
    Value { name: Name, ty: Type },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Bound {
    Trait { path: Vec<Name>, args: Vec<Type> },
    Nat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamMode {
    Let,
    Var,
    Take,
    Ghost,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Value {
        mode: ParamMode,
        /// `self` for a receiver.
        name: Name,
        ty: Option<Type>,
        default: Option<Expr>,
        span: Span,
    },
    /// `/`: the parameters before it are positional-only.
    PosOnly(Span),
    /// `**name: T`
    Rest { name: Name, ty: Type, span: Span },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDef {
    pub name: Name,
    pub name_span: Span,
    pub tparams: Vec<TParam>,
    pub rhs: TypeRhs,
    pub refine: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeRhs {
    /// `positional`: the number of fields before a `/`, which are positional-only (R437).
    Record {
        fields: Vec<Field>,
        positional: Option<usize>,
    },
    Sum(Vec<Variant>),
    Alias(Type),
    Newtype(Type),
    CapSet(Vec<CapItem>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub attrs: Vec<Attr>,
    pub name: Name,
    pub ty: Type,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub attrs: Vec<Attr>,
    pub name: Name,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitDef {
    pub name: Name,
    pub tparams: Vec<TParam>,
    pub assoc: Vec<Name>,
    pub methods: Vec<TraitMethod>,
    pub derived: Option<Body>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitMethod {
    pub attrs: Vec<Attr>,
    pub sig: Sig,
    pub default: Option<Body>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImplDef {
    pub unsafe_: bool,
    pub trait_path: Vec<Name>,
    pub trait_args: Vec<Type>,
    pub for_type: Type,
    pub assoc: Vec<(Name, Type)>,
    /// Function items only.
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstDef {
    pub name: Name,
    pub ty: Type,
    pub value: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternDef {
    pub abi: String,
    pub lib: Name,
    pub link: Option<String>,
    pub items: Vec<XItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum XItem {
    Fn {
        attrs: Vec<Attr>,
        safe: bool,
        name: Name,
        params: Vec<XParam>,
        ret: Option<Type>,
        from: Vec<Name>,
        uses_none: bool,
        nonblocking: bool,
        span: Span,
    },
    Static {
        attrs: Vec<Attr>,
        safe: bool,
        name: Name,
        ty: Type,
        span: Span,
    },
    Type {
        name: Name,
        drop: Name,
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    Scoped,
    Escaping,
}

#[derive(Clone, Debug, PartialEq)]
pub enum XParam {
    Typed {
        name: Name,
        callback: Option<Callback>,
        ty: Type,
    },
    Variadic,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UseDecl {
    pub relative: bool,
    pub path: Vec<Name>,
    pub alias: Option<Name>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    /// A named type, scalars included: `int`, `List[T]`, `http.HttpError`.
    Path {
        path: Vec<Name>,
        args: Vec<TArg>,
    },
    Dyn {
        name: Name,
        args: Vec<Type>,
    },
    Unit,
    Tuple(Vec<Type>),
    Ref(Box<Type>),
    Ptr(Box<Type>),
    Vec {
        lanes: String,
        elem: Name,
    },
    Record(Vec<Field>),
    Option(Box<Type>),
    Union(Vec<Type>),
    Fn {
        par: bool,
        params: Vec<Type>,
        ret: Box<Type>,
        fail: Option<Box<Type>>,
        uses: Option<Vec<CapItem>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum TArg {
    Type(Type),
    Nat(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Expr(Expr),
    Assign {
        place: Expr,
        op: &'static str,
        value: Expr,
    },
    /// `let`/`var`. A let-else is a `Fallback` value under a non-name pattern (R374, R464).
    Let {
        mutable: bool,
        ghost: bool,
        pattern: Pattern,
        ty: Option<Type>,
        value: Expr,
    },
    Return(Option<Expr>),
    Fail(Expr),
    Yield(Expr),
    Reveal(Vec<Name>),
    Break(Option<Name>),
    Continue(Option<Name>),
    Assert(Expr),
    Unreachable,
    Become(Expr),
    If {
        branches: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Option<Vec<Stmt>>,
    },
    Match {
        scrutinee: Expr,
        arms: Vec<Arm>,
    },
    For {
        label: Option<Name>,
        mutable: bool,
        pattern: Pattern,
        iter: Expr,
        invariants: Vec<Contract>,
        body: Vec<Stmt>,
    },
    While {
        label: Option<Name>,
        cond: Expr,
        invariants: Vec<Contract>,
        body: Vec<Stmt>,
    },
    Loop {
        label: Option<Name>,
        invariants: Vec<Contract>,
        body: Vec<Stmt>,
    },
    Par {
        concurrent: bool,
        body: Vec<Stmt>,
    },
    Select(Vec<SelArm>),
    With {
        kind: WithKind,
        body: Vec<Stmt>,
    },
    Unsafe(Vec<Stmt>),
    StageFor {
        pattern: Pattern,
        iter: Expr,
        body: Vec<Stmt>,
    },
    StageIf {
        cond: Expr,
        then: Vec<Stmt>,
        otherwise: Option<Vec<Stmt>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum WithKind {
    Bind {
        name: Name,
        dynamic: bool,
        value: Expr,
    },
    Deadline(Expr),
    Timeout(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: ArmBody,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArmBody {
    Expr(Expr),
    Block(Vec<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelArm {
    pub op: Expr,
    pub pattern: Option<Pattern>,
    pub body: ArmBody,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    BitNot,
    Not,
    Copy,
    Share,
    Try,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    NotIn,
    BitOr,
    BitXor,
    BitAnd,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    MatMul,
    Pow,
}

impl BinOp {
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Eq
                | Self::Ne
                | Self::Lt
                | Self::Le
                | Self::Gt
                | Self::Ge
                | Self::In
                | Self::NotIn
        )
    }

    pub fn is_bitwise(self) -> bool {
        matches!(
            self,
            Self::BitOr | Self::BitXor | Self::BitAnd | Self::Shl | Self::Shr
        )
    }

    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            Self::Add
                | Self::Sub
                | Self::Mul
                | Self::Div
                | Self::FloorDiv
                | Self::Mod
                | Self::MatMul
                | Self::Pow
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrLit {
    pub kind: StrKind,
    pub parts: Vec<StrPart>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    Text(String),
    Interp { expr: Expr, spec: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Int(String),
    Float(String),
    UnitNum {
        num: String,
        unit: String,
    },
    Temporal {
        kind: TemporalKind,
        text: String,
    },
    Str(StrLit),
    Char(String),
    Bool(bool),
    NoneLit,
    Name(Name),
    TypeName(Name),
    Hole,
    SelfValue,
    /// `.name`, the implicit subject; the rest of the chain is ordinary postfix.
    Subject(Name),
    Paren(Box<Expr>),
    Unit,
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    Set(Vec<Expr>),
    Grid(Vec<Vec<Expr>>),
    /// `#e`: stage-0 evaluation.
    Stage(Box<Expr>),
    Asm {
        template: String,
        args: Vec<AsmArg>,
    },
    Field(Box<Expr>, Name),
    TypeField(Box<Expr>, Name),
    /// `e.(name)`
    Reflect(Box<Expr>, Name),
    Call(Box<Expr>, Vec<Arg>),
    Index(Box<Expr>, Vec<IxArg>),
    Question(Box<Expr>),
    Cast(Box<Expr>, Type),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Is {
        value: Box<Expr>,
        negated: bool,
        pattern: Pattern,
    },
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
    Fallback(Box<Expr>, Box<Fallback>),
    Lambda {
        params: Vec<Name>,
        body: Box<Expr>,
    },
    /// A `let x =` block value.
    Block(Vec<Stmt>),
    ParFor {
        concurrent: bool,
        limit: Option<Box<Expr>>,
        pattern: Pattern,
        iter: Box<Expr>,
        body: Vec<Stmt>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fallback {
    Value(Expr),
    /// `else err -> e`
    Bind(Name, Expr),
    /// `else return`, `else fail e`, `else break`, `else continue`, `else unreachable`
    Diverge(Stmt),
    /// An indented block whose last statement diverges, with an optional binder (R433).
    Block(Option<Name>, Vec<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub label: Option<Name>,
    pub kind: ArgKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArgKind {
    Value(Expr),
    /// `&place`
    Ref(Expr),
    Become(Expr),
    /// `name:` alone passes the variable of that name.
    Pun,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IxArg {
    Expr(Expr),
    Slice {
        lo: Option<Expr>,
        hi: Option<Expr>,
        inclusive: bool,
    },
    Labeled(Name, Box<IxArg>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AsmReg {
    /// A register class: `reg`.
    Class(Name),
    /// An explicit register: `"rax"`.
    Explicit(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AsmArg {
    Operand {
        dir: Name,
        reg: AsmReg,
        value: Expr,
    },
    Typed {
        dir: Name,
        reg: AsmReg,
        name: Name,
        ty: Type,
    },
    ClobberAbi(String),
    Option(Name),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatKind {
    Wild,
    Lit(Box<Expr>),
    Range {
        lo: Box<Expr>,
        hi: Box<Expr>,
        inclusive: bool,
    },
    Bind(Name),
    Ctor {
        name: Name,
        /// None for a bare name like `None` or `Red`.
        args: Option<Vec<PPat>>,
    },
    Tuple(Vec<Pattern>),
    Slice(Vec<SPat>),
    Or(Vec<Pattern>),
    As(Box<Pattern>, Name),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PPat {
    Pos(Pattern),
    Named(Name, Option<Pattern>),
    Rest,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SPat {
    Pat(Pattern),
    Rest(Option<Name>),
}
