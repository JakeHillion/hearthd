//! Abstract Syntax Tree types for the HearthD Automations language.
//!
//! All AST nodes include source span information for error reporting.

use std::fmt;

use chumsky::span::SimpleSpan;

/// A source span representing a range in the input.
pub type Span = SimpleSpan;

/// An AST node with an associated source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

/// Top-level program: either a single automation or a template.
#[derive(Debug, Clone, PartialEq)]
pub enum Program {
    Automation(Automation),
    Template(Template),
}

/// A template with parameters that returns multiple automations.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub params: Vec<Spanned<TemplateParam>>,
    pub automations: Vec<Spanned<Automation>>,
}

/// A template parameter with a name and type.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateParam {
    pub name: String,
    pub ty: Type,
}

/// An automation definition (observer or mutator).
#[derive(Debug, Clone, PartialEq)]
pub struct Automation {
    pub kind: AutomationKind,
    pub pattern: Spanned<Pattern>,
    pub filter: Spanned<Expr>,
    pub body: Vec<Spanned<Stmt>>,
}

/// The kind of automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationKind {
    Observer,
    Mutator,
}

impl fmt::Display for AutomationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutomationKind::Observer => write!(f, "observer"),
            AutomationKind::Mutator => write!(f, "mutator"),
        }
    }
}

/// A destructuring pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Simple identifier binding.
    Ident(String),
    /// Struct destructuring pattern.
    Struct {
        fields: Vec<Spanned<FieldPattern>>,
        has_rest: bool,
    },
}

/// A field in a struct pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPattern {
    pub name: String,
    pub pattern: Option<Spanned<Pattern>>,
}

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { name: String, value: Spanned<Expr> },
    Expr(Spanned<Expr>),
}

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // Literals
    Int(i64),
    Float(String), // Store as string to match Token
    String(String),
    Bool(bool),

    // Unit literals
    UnitLiteral {
        value: String, // Store as string to match Token
        unit: UnitType,
    },

    // Identifiers
    Ident(String),

    // Binary operations
    BinOp {
        op: BinOp,
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },

    // Unary operations
    UnaryOp {
        op: UnaryOp,
        expr: Box<Spanned<Expr>>,
    },

    // Field access
    Field {
        expr: Box<Spanned<Expr>>,
        field: String,
    },

    // Optional chaining
    OptionalField {
        expr: Box<Spanned<Expr>>,
        field: String,
    },

    // Function call
    Call {
        func: Box<Spanned<Expr>>,
        args: Vec<Spanned<Arg>>,
    },

    // If expression
    If {
        cond: Box<Spanned<Expr>>,
        then_block: Vec<Spanned<Stmt>>,
        else_block: Vec<Spanned<Stmt>>,
    },

    // List literal
    List(Vec<Spanned<Expr>>),

    // List comprehension
    ListComp {
        expr: Box<Spanned<Expr>>,
        var: String,
        iter: Box<Spanned<Expr>>,
        filter: Option<Box<Spanned<Expr>>>,
    },

    // Struct literal
    StructLit {
        name: String,
        fields: Vec<Spanned<StructField>>,
    },
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Mod => write!(f, "%"),
            BinOp::Eq => write!(f, "=="),
            BinOp::Ne => write!(f, "!="),
            BinOp::Lt => write!(f, "<"),
            BinOp::Le => write!(f, "<="),
            BinOp::Gt => write!(f, ">"),
            BinOp::Ge => write!(f, ">="),
            BinOp::And => write!(f, "&&"),
            BinOp::Or => write!(f, "||"),
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,   // -
    Not,   // !
    Deref, // *
    Await, // await
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOp::Neg => write!(f, "-"),
            UnaryOp::Not => write!(f, "!"),
            UnaryOp::Deref => write!(f, "*"),
            UnaryOp::Await => write!(f, "await"),
        }
    }
}

/// Unit types for numeric literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitType {
    // Time units
    Seconds,
    Minutes,
    Hours,
    Days,

    // Angle units
    Degrees,
    Radians,

    // Temperature units
    Celsius,
    Fahrenheit,
    Kelvin,
}

impl fmt::Display for UnitType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnitType::Seconds => write!(f, "s"),
            UnitType::Minutes => write!(f, "min"),
            UnitType::Hours => write!(f, "h"),
            UnitType::Days => write!(f, "d"),
            UnitType::Degrees => write!(f, "deg"),
            UnitType::Radians => write!(f, "rad"),
            UnitType::Celsius => write!(f, "c"),
            UnitType::Fahrenheit => write!(f, "f"),
            UnitType::Kelvin => write!(f, "k"),
        }
    }
}

/// Function call argument.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Positional(Spanned<Expr>),
    Named { name: String, value: Spanned<Expr> },
}

/// Struct literal field.
#[derive(Debug, Clone, PartialEq)]
pub enum StructField {
    /// `field: value`
    Field { name: String, value: Spanned<Expr> },
    /// `inherit field`
    Inherit(String),
    /// `...spread`
    Spread(String),
}

/// Type annotation.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Named(String),
    List(Box<Type>),
    Set(Box<Type>),
    Map { key: Box<Type>, value: Box<Type> },
    Option(Box<Type>),
}
