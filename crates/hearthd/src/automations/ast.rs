//! Abstract Syntax Tree types for the HearthD Automations language.
//!
//! All AST nodes include source span information for error reporting.

use chumsky::span::SimpleSpan;
use strum::Display;

/// An AST node with an associated source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: SimpleSpan,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: SimpleSpan) -> Self {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "lowercase")]
pub enum AutomationKind {
    Observer,
    Mutator,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum BinOp {
    // Arithmetic
    #[strum(serialize = "+")]
    Add,
    #[strum(serialize = "-")]
    Sub,
    #[strum(serialize = "*")]
    Mul,
    #[strum(serialize = "/")]
    Div,
    #[strum(serialize = "%")]
    Mod,

    // Comparison
    #[strum(serialize = "==")]
    Eq,
    #[strum(serialize = "!=")]
    Ne,
    #[strum(serialize = "<")]
    Lt,
    #[strum(serialize = "<=")]
    Le,
    #[strum(serialize = ">")]
    Gt,
    #[strum(serialize = ">=")]
    Ge,

    // Logical
    #[strum(serialize = "&&")]
    And,
    #[strum(serialize = "||")]
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum UnaryOp {
    #[strum(serialize = "-")]
    Neg,
    #[strum(serialize = "!")]
    Not,
    #[strum(serialize = "*")]
    Deref,
    #[strum(serialize = "await")]
    Await,
}

/// Unit types for numeric literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum UnitType {
    // Time units
    #[strum(serialize = "s")]
    Seconds,
    #[strum(serialize = "min")]
    Minutes,
    #[strum(serialize = "h")]
    Hours,
    #[strum(serialize = "d")]
    Days,

    // Angle units
    #[strum(serialize = "deg")]
    Degrees,
    #[strum(serialize = "rad")]
    Radians,

    // Temperature units
    #[strum(serialize = "c")]
    Celsius,
    #[strum(serialize = "f")]
    Fahrenheit,
    #[strum(serialize = "k")]
    Kelvin,
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
