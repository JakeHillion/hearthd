//! Lowered AST types for the HearthD Automations language.
//!
//! The lowered AST is produced by desugaring the high-level AST. List comprehensions
//! are transformed into explicit loop constructs with mutable list operations.
//!
//! Each lowered node maintains a reference to its originating source AST node
//! via the `Origin` type, enabling accurate error reporting and debugging.

use std::rc::Rc;

use super::ast;
// Re-export shared types from ast.rs
pub use super::ast::{BinOp, UnaryOp, UnitType};

/// Reference to the original AST node that produced a lowered node.
/// The span is accessible via `origin.span()`.
#[derive(Debug, Clone)]
pub enum Origin {
    /// Direct mapping from source AST (1:1 correspondence).
    /// Owns the AST node directly since there's no sharing.
    Direct(ast::Spanned<ast::Expr>),
    /// Synthetic node generated from desugaring a ListComp.
    /// Uses Rc because multiple synthetic nodes share the same original ListComp.
    ListComp(Rc<ast::Spanned<ast::Expr>>),
}

impl Origin {
    /// Get the original AST node.
    pub fn ast_node(&self) -> &ast::Spanned<ast::Expr> {
        match self {
            Origin::Direct(expr) => expr,
            Origin::ListComp(rc) => rc,
        }
    }

    /// Get the source span (delegates to the original AST node's span).
    pub fn span(&self) -> chumsky::span::SimpleSpan {
        self.ast_node().span
    }

    /// Returns true if this is a synthetic node from desugaring.
    pub fn is_synthetic(&self) -> bool {
        matches!(self, Origin::ListComp(_))
    }
}

/// A lowered AST node with reference to its originating source.
/// No separate span field - use `origin.span()` to get the source location.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub origin: Origin,
}

impl<T> Spanned<T> {
    pub fn new(node: T, origin: Origin) -> Self {
        Self { node, origin }
    }

    /// Get the source span from the origin.
    pub fn span(&self) -> chumsky::span::SimpleSpan {
        self.origin.span()
    }
}

/// A lowered expression.
#[derive(Debug, Clone)]
pub enum LoweredExpr {
    // Literals
    Int(i64),
    Float(String),
    String(String),
    Bool(bool),

    // Unit literals
    UnitLiteral {
        value: String,
        unit: UnitType,
    },

    // Identifiers and paths
    Ident(String),
    Path(Vec<String>),

    // Binary operations
    BinOp {
        op: BinOp,
        left: Box<Spanned<LoweredExpr>>,
        right: Box<Spanned<LoweredExpr>>,
    },

    // Unary operations
    UnaryOp {
        op: UnaryOp,
        expr: Box<Spanned<LoweredExpr>>,
    },

    // Field access
    Field {
        expr: Box<Spanned<LoweredExpr>>,
        field: String,
    },

    // Optional chaining
    OptionalField {
        expr: Box<Spanned<LoweredExpr>>,
        field: String,
    },

    // Function call
    Call {
        func: Box<Spanned<LoweredExpr>>,
        args: Vec<Spanned<LoweredArg>>,
    },

    // If expression
    If {
        cond: Box<Spanned<LoweredExpr>>,
        then_block: Vec<Spanned<LoweredStmt>>,
        else_block: Option<Vec<Spanned<LoweredStmt>>>,
    },

    // List literal
    List(Vec<Spanned<LoweredExpr>>),

    // Struct literal
    StructLit {
        name: String,
        fields: Vec<Spanned<LoweredStructField>>,
    },

    // Block expression (synthetic, from ListComp desugaring)
    // Contains statements followed by a result expression.
    Block {
        stmts: Vec<Spanned<LoweredStmt>>,
        result: Box<Spanned<LoweredExpr>>,
    },

    // Create empty mutable list (synthetic, from ListComp desugaring)
    MutableList,
}

/// A lowered statement.
#[derive(Debug, Clone)]
pub enum LoweredStmt {
    /// Immutable let binding: `let x = expr;`
    Let {
        name: String,
        value: Spanned<LoweredExpr>,
    },

    /// Mutable let binding: `let mut x = expr;`
    /// Generated during desugaring of list comprehensions.
    LetMut {
        name: String,
        value: Spanned<LoweredExpr>,
    },

    /// Expression statement
    Expr(Spanned<LoweredExpr>),

    /// Return statement
    Return(Spanned<LoweredExpr>),

    /// For loop: `for var in iter { body }`
    /// Generated from list comprehensions.
    For {
        var: String,
        iter: Spanned<LoweredExpr>,
        body: Vec<Spanned<LoweredStmt>>,
    },

    /// Push value onto a mutable list variable (synthetic, from ListComp desugaring).
    /// References the list by variable name, not by expression.
    Push {
        list: String,
        value: Spanned<LoweredExpr>,
    },
}

/// A lowered automation definition.
#[derive(Debug, Clone)]
pub struct LoweredAutomation {
    pub kind: ast::AutomationKind,
    pub pattern: ast::Spanned<ast::Pattern>,
    pub filter: Option<Spanned<LoweredExpr>>,
    pub body: Vec<Spanned<LoweredStmt>>,
}

/// A lowered top-level program.
#[derive(Debug, Clone)]
pub enum LoweredProgram {
    Automation(LoweredAutomation),
    Template {
        params: Vec<ast::Spanned<ast::TemplateParam>>,
        automations: Vec<LoweredAutomation>,
    },
}

/// Lowered function argument.
#[derive(Debug, Clone)]
pub enum LoweredArg {
    Positional(Spanned<LoweredExpr>),
    Named {
        name: String,
        value: Spanned<LoweredExpr>,
    },
}

/// Lowered struct literal field.
#[derive(Debug, Clone)]
pub enum LoweredStructField {
    /// `field: value`
    Field {
        name: String,
        value: Spanned<LoweredExpr>,
    },
    /// `inherit field`
    Inherit(String),
    /// `...spread`
    Spread(String),
}
