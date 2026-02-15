//! HIR (High-Level IR) types for the HearthD Automations language.
//!
//! The HIR is a control-flow graph of basic blocks with linear instruction
//! sequences. It is produced by lowering the typed AST. Variable names are
//! replaced with numbered temporaries, but entity references remain symbolic
//! for later linking.

use super::ast;
use super::typed::Ty;

/// A numbered temporary value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tmp(pub usize);

/// A basic block identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub usize);

/// A parameter extracted from the automation's destructuring pattern.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub tmp: Tmp,
    pub ty: Ty,
}

/// A lowered automation in HIR form.
#[derive(Debug, Clone)]
pub struct HirAutomation {
    pub kind: ast::AutomationKind,
    pub params: Vec<Param>,
    pub blocks: Vec<BasicBlock>,
}

/// A lowered program in HIR form.
#[derive(Debug, Clone)]
pub enum HirProgram {
    Automation(HirAutomation),
    Template {
        params: Vec<ast::Spanned<ast::TemplateParam>>,
        automations: Vec<HirAutomation>,
    },
}

/// A basic block: a linear sequence of instructions followed by a terminator.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

/// A single instruction that computes a value and stores it in a temporary.
#[derive(Debug, Clone)]
pub struct Instruction {
    pub dst: Tmp,
    pub op: Op,
    pub ty: Ty,
}

/// Operations that compute values.
#[derive(Debug, Clone)]
pub enum Op {
    // === Constants ===
    ConstInt(i64),
    ConstFloat(f64),
    ConstString(String),
    ConstBool(bool),
    ConstUnit {
        value: String,
        unit: ast::UnitType,
    },

    /// The unit/void value.
    Unit,

    // === Binary (no &&/|| — those become branches) ===
    BinOp {
        op: HirBinOp,
        left: Tmp,
        right: Tmp,
    },

    // === Unary ===
    Neg(Tmp),
    Not(Tmp),
    Deref(Tmp),
    Await(Tmp),

    // === Field access ===
    Field {
        base: Tmp,
        field: String,
    },
    OptionalField {
        base: Tmp,
        field: String,
    },

    // === Function calls (all args positional) ===
    Call {
        name: String,
        args: Vec<Tmp>,
    },

    /// Enum variant construction (e.g. Event::LightStateChanged(l)).
    Variant {
        enum_name: String,
        variant: String,
        args: Vec<Tmp>,
    },

    // === Collections ===
    /// Empty list (from MutableList desugaring).
    EmptyList,
    /// List literal with known elements.
    List(Vec<Tmp>),
    /// Push a value onto a list.
    ListPush {
        list: Tmp,
        value: Tmp,
    },
    /// Create an iterator from a collection.
    IterInit(Tmp),

    // === Struct construction ===
    Struct {
        name: String,
        fields: Vec<HirStructField>,
    },

    // === Value ===
    /// Copy a temporary (for merge points and variable references).
    Copy(Tmp),
}

/// Binary operators in HIR. `And`/`Or` are excluded because they use
/// short-circuit branching instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirBinOp {
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
    In,
}

impl std::fmt::Display for HirBinOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HirBinOp::Add => write!(f, "add"),
            HirBinOp::Sub => write!(f, "sub"),
            HirBinOp::Mul => write!(f, "mul"),
            HirBinOp::Div => write!(f, "div"),
            HirBinOp::Mod => write!(f, "mod"),
            HirBinOp::Eq => write!(f, "eq"),
            HirBinOp::Ne => write!(f, "ne"),
            HirBinOp::Lt => write!(f, "lt"),
            HirBinOp::Le => write!(f, "le"),
            HirBinOp::Gt => write!(f, "gt"),
            HirBinOp::Ge => write!(f, "ge"),
            HirBinOp::In => write!(f, "in"),
        }
    }
}

/// A struct field in HIR form.
#[derive(Debug, Clone)]
pub enum HirStructField {
    /// An explicitly set field: `name: value`.
    Set { name: String, value: Tmp },
    /// A spread from another struct: `...source`.
    Spread(Tmp),
}

/// Block terminator — exactly one per basic block.
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Unconditional jump.
    Jump(BlockId),
    /// Conditional branch.
    Branch {
        cond: Tmp,
        then_block: BlockId,
        else_block: BlockId,
    },
    /// Return from automation.
    Return(Tmp),
    /// Iterator advance: try to get next element.
    /// If available, bind to `value` and jump to `body`.
    /// If exhausted, jump to `exit`.
    IterNext {
        iter: Tmp,
        value: Tmp,
        body: BlockId,
        exit: BlockId,
    },
}
