//! Typed AST types for the HearthD Automations language.
//!
//! The typed AST is produced by the type checker from the lowered AST.
//! Every expression carries a resolved `Ty`, and entity constraints are
//! collected for runtime validation.

use super::function::FunctionIdentity;
use crate::automations::desugar::lowered::Origin;
use crate::automations::domain::Domain;
use crate::automations::lexer::UnitType;
use crate::automations::parser::ast;

/// Internal semantic type. Distinct from the syntactic `ast::Type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    // Primitives
    Int,
    Float,
    Bool,
    String,

    // Unit literal types
    Duration,
    Angle,
    Temperature,

    // Collections
    List(Box<Ty>),
    Set(Box<Ty>),
    Map {
        key: Box<Ty>,
        value: Box<Ty>,
    },
    Option(Box<Ty>),

    // Async
    Future(Box<Ty>),

    // Named type referencing the registry (e.g. "Event", "Light")
    Named(std::string::String),

    // The group of entities a deployment has in one domain, as `state.light`.
    // Its fields are slugs, each naming a `Node`; which slugs exist is
    // deployment knowledge the checker deliberately does not have, so any
    // slug type checks and the relocator decides whether it resolves.
    DomainGroup(Domain),

    // Enum variant (e.g. Event::LightOff)
    EnumVariant {
        enum_name: std::string::String,
        variant_name: std::string::String,
    },

    // Void-returning statements
    Unit,

    // Poison type to prevent cascading errors
    Error,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "Int"),
            Ty::Float => write!(f, "Float"),
            Ty::Bool => write!(f, "Bool"),
            Ty::String => write!(f, "String"),
            Ty::Duration => write!(f, "Duration"),
            Ty::Angle => write!(f, "Angle"),
            Ty::Temperature => write!(f, "Temperature"),
            Ty::List(t) => write!(f, "[{}]", t),
            Ty::Set(t) => write!(f, "Set<{}>", t),
            Ty::Map { key, value } => write!(f, "Map<{}, {}>", key, value),
            Ty::Option(t) => write!(f, "Option<{}>", t),
            Ty::Future(t) => write!(f, "Future<{}>", t),
            Ty::Named(n) => write!(f, "{}", n),
            Ty::DomainGroup(d) => write!(f, "state.{}", d),
            Ty::EnumVariant {
                enum_name,
                variant_name,
            } => write!(f, "{}::{}", enum_name, variant_name),
            Ty::Unit => write!(f, "()"),
            Ty::Error => write!(f, "<error>"),
        }
    }
}

/// A typed expression node. Mirrors `LoweredExpr` variants 1:1 with an
/// additional `ty` field on every expression.
#[derive(Debug, Clone)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: Ty,
    pub origin: Origin,
}

/// Expression kinds in the typed AST.
#[derive(Debug, Clone)]
pub enum TypedExprKind {
    // Literals
    Int(i64),
    Float(f64),
    String(std::string::String),
    Bool(bool),

    // Unit literals
    UnitLiteral {
        value: std::string::String,
        unit: UnitType,
    },

    // Identifiers and paths
    Ident(std::string::String),
    Path(Vec<std::string::String>),

    // Binary operations
    BinOp {
        op: ast::BinOp,
        left: Box<TypedExpr>,
        right: Box<TypedExpr>,
    },

    // Unary operations
    UnaryOp {
        op: ast::UnaryOp,
        expr: Box<TypedExpr>,
    },

    // Field access
    Field {
        expr: Box<TypedExpr>,
        field: std::string::String,
    },

    // Optional chaining
    OptionalField {
        expr: Box<TypedExpr>,
        field: std::string::String,
    },

    /// A call resolved to a known function. The callee is decided here, so
    /// no later stage looks it up by name.
    Call {
        function: FunctionIdentity,
        args: Vec<TypedArg>,
    },

    /// Enum variant construction, e.g. `Event::OnOffChanged(…)`. Shares call
    /// syntax with [`TypedExprKind::Call`] but is a different operation, so
    /// lowering does not have to tell them apart by inspecting the callee.
    VariantCtor {
        enum_name: String,
        variant: String,
        args: Vec<TypedArg>,
    },

    /// A call the checker could not resolve, already reported as an error.
    /// Retained so the tree stays walkable for further diagnostics.
    UnresolvedCall {
        func: Box<TypedExpr>,
        args: Vec<TypedArg>,
    },

    // If expression
    If {
        cond: Box<TypedExpr>,
        then_block: Vec<TypedStmt>,
        else_block: Option<Vec<TypedStmt>>,
    },

    // List literal
    List(Vec<TypedExpr>),

    // Struct literal
    StructLit {
        name: std::string::String,
        fields: Vec<TypedStructField>,
    },

    // Block expression (from desugared list comprehensions)
    Block {
        stmts: Vec<TypedStmt>,
        result: Box<TypedExpr>,
    },

    // Empty mutable list (from desugared list comprehensions)
    MutableList,
}

/// A typed statement.
#[derive(Debug, Clone)]
pub enum TypedStmt {
    Let {
        name: std::string::String,
        value: TypedExpr,
        origin: Origin,
    },
    LetMut {
        name: std::string::String,
        value: TypedExpr,
        origin: Origin,
    },
    Expr(TypedExpr),
    Return(TypedExpr, Origin),
    For {
        var: std::string::String,
        iter: TypedExpr,
        body: Vec<TypedStmt>,
        origin: Origin,
    },
    Push {
        list: std::string::String,
        value: TypedExpr,
        origin: Origin,
    },
}

/// A typed function argument.
#[derive(Debug, Clone)]
pub enum TypedArg {
    Positional(TypedExpr),
    Named {
        name: std::string::String,
        value: TypedExpr,
    },
}

/// A typed struct field.
#[derive(Debug, Clone)]
pub enum TypedStructField {
    Field {
        name: std::string::String,
        value: TypedExpr,
    },
    Inherit(std::string::String),
    Spread(std::string::String),
}

/// A typed automation definition.
#[derive(Debug, Clone)]
pub struct TypedAutomation {
    pub kind: ast::AutomationKind,
    pub pattern: ast::Spanned<ast::Pattern>,
    pub filter: Option<TypedExpr>,
    pub body: Vec<TypedStmt>,
}

/// A typed top-level program.
#[derive(Debug, Clone)]
pub enum TypedProgram {
    Automation(Box<TypedAutomation>),
    Template {
        params: Vec<ast::Spanned<ast::TemplateParam>>,
        automations: Vec<TypedAutomation>,
    },
}

/// A type error produced during type checking.
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: std::string::String,
    pub span: chumsky::span::SimpleSpan,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "type error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

/// The result of type checking a program.
#[derive(Debug)]
pub struct CheckResult {
    pub program: TypedProgram,
    pub errors: Vec<TypeError>,
}

impl CheckResult {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Render all type errors as pretty diagnostics with source context.
    pub fn format_errors(&self, source: &str, filename: &str) -> String {
        super::format_type_errors(&self.errors, source, filename)
    }
}
