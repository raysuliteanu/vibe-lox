pub mod printer;

use serde::Serialize;

use crate::scanner::token::Span;

/// A unique identifier for each expression node, used by the resolver
/// to store variable resolution depths.
pub type ExprId = usize;

/// Top-level program: a list of declarations.
#[derive(Debug, Clone, Serialize)]
pub struct Program {
    pub declarations: Vec<Decl>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Decl {
    Class(ClassDecl),
    Fun(FunDecl),
    Var(VarDecl),
    Statement(Stmt),
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassDecl {
    pub name: String,
    pub superclass: Option<String>,
    pub methods: Vec<Function>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunDecl {
    pub function: Function,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct VarDecl {
    pub name: String,
    pub initializer: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Decl>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Stmt {
    Expression(ExprStmt),
    Print(PrintStmt),
    Return(ReturnStmt),
    Block(BlockStmt),
    If(IfStmt),
    While(WhileStmt),
}

#[derive(Debug, Clone, Serialize)]
pub struct ExprStmt {
    pub expression: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrintStmt {
    pub expression: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockStmt {
    pub declarations: Vec<Decl>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct IfStmt {
    pub condition: Expr,
    pub then_branch: Box<Stmt>,
    pub else_branch: Option<Box<Stmt>>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhileStmt {
    pub condition: Expr,
    pub body: Box<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Expr {
    Binary(BinaryExpr),
    Unary(UnaryExpr),
    Literal(LiteralExpr),
    Grouping(GroupingExpr),
    Variable(VariableExpr),
    Assign(AssignExpr),
    Logical(LogicalExpr),
    Call(CallExpr),
    Get(GetExpr),
    Set(SetExpr),
    This(ThisExpr),
    Super(SuperExpr),
}

impl Expr {
    pub fn id(&self) -> ExprId {
        match self {
            Self::Binary(e) => e.id,
            Self::Unary(e) => e.id,
            Self::Literal(e) => e.id,
            Self::Grouping(e) => e.id,
            Self::Variable(e) => e.id,
            Self::Assign(e) => e.id,
            Self::Logical(e) => e.id,
            Self::Call(e) => e.id,
            Self::Get(e) => e.id,
            Self::Set(e) => e.id,
            Self::This(e) => e.id,
            Self::Super(e) => e.id,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Self::Binary(e) => e.span,
            Self::Unary(e) => e.span,
            Self::Literal(e) => e.span,
            Self::Grouping(e) => e.span,
            Self::Variable(e) => e.span,
            Self::Assign(e) => e.span,
            Self::Logical(e) => e.span,
            Self::Call(e) => e.span,
            Self::Get(e) => e.span,
            Self::Set(e) => e.span,
            Self::This(e) => e.span,
            Self::Super(e) => e.span,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BinaryExpr {
    pub id: ExprId,
    pub left: Box<Expr>,
    pub operator: BinaryOp,
    pub right: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
pub enum BinaryOp {
    #[strum(serialize = "+")]
    Add,
    #[strum(serialize = "-")]
    Subtract,
    #[strum(serialize = "*")]
    Multiply,
    #[strum(serialize = "/")]
    Divide,
    #[strum(serialize = "==")]
    Equal,
    #[strum(serialize = "!=")]
    NotEqual,
    #[strum(serialize = "<")]
    Less,
    #[strum(serialize = "<=")]
    LessEqual,
    #[strum(serialize = ">")]
    Greater,
    #[strum(serialize = ">=")]
    GreaterEqual,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnaryExpr {
    pub id: ExprId,
    pub operator: UnaryOp,
    pub operand: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
pub enum UnaryOp {
    #[strum(serialize = "-")]
    Negate,
    #[strum(serialize = "!")]
    Not,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiteralExpr {
    pub id: ExprId,
    pub value: LiteralValue,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub enum LiteralValue {
    Number(f64),
    String(String),
    Bool(bool),
    Nil,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupingExpr {
    pub id: ExprId,
    pub expression: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct VariableExpr {
    pub id: ExprId,
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignExpr {
    pub id: ExprId,
    pub name: String,
    pub value: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogicalExpr {
    pub id: ExprId,
    pub left: Box<Expr>,
    pub operator: LogicalOp,
    pub right: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallExpr {
    pub id: ExprId,
    pub callee: Box<Expr>,
    pub arguments: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct GetExpr {
    pub id: ExprId,
    pub object: Box<Expr>,
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetExpr {
    pub id: ExprId,
    pub object: Box<Expr>,
    pub name: String,
    pub value: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThisExpr {
    pub id: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuperExpr {
    pub id: ExprId,
    pub method: String,
    pub span: Span,
}
