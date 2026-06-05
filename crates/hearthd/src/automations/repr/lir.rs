//! LIR (Low-level IR) types for the HearthD Automations language.
//!
//! The LIR is a flat, labeled, register-based instruction stream produced
//! by lowering each [`super::hir::HirFunction`]. Basic block terminators
//! become regular instructions (`Jump`, `JumpIf`, `IterNext`, `Return`),
//! and `Tmp`s become numbered `Reg`s in a per-function namespace.
//!
//! LIR is the last stage that retains human-readable structure; it is
//! subsequently encoded to bytecode for VM execution.
//!
//! Registers are unbounded and intended as scratch slots. The lowering
//! pass does not attempt single-use enforcement or coalescing — it
//! preserves the HIR `Tmp` numbering 1:1 so each function reports
//! `num_regs = max_tmp + 1`.

use super::ast;
use super::hir::HirBinOp;
use super::typed::Ty;

/// A numbered register within a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg(pub usize);

/// A label naming a position in the instruction stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Label(pub usize);

/// A parameter passed in to a function in a specific register.
#[derive(Debug, Clone)]
pub struct LirParam {
    pub name: String,
    pub reg: Reg,
    pub ty: Ty,
}

/// A single LIR function: a flat stream of instructions over a fresh
/// register namespace sized `num_regs`.
#[derive(Debug, Clone)]
pub struct LirFunction {
    pub params: Vec<LirParam>,
    pub num_regs: usize,
    pub instrs: Vec<LirInstr>,
}

/// A lowered automation in LIR form.
#[derive(Debug, Clone)]
pub struct LirAutomation {
    pub kind: ast::AutomationKind,
    pub filter: Option<LirFunction>,
    pub body: LirFunction,
}

/// A lowered program in LIR form.
#[derive(Debug, Clone)]
pub enum LirProgram {
    Automation(LirAutomation),
    Template {
        params: Vec<ast::Spanned<ast::TemplateParam>>,
        automations: Vec<LirAutomation>,
    },
}

/// A struct field in LIR form. Mirrors `HirStructField`.
#[derive(Debug, Clone)]
pub enum LirStructField {
    Set { name: String, value: Reg },
    Spread(Reg),
}

/// One LIR instruction. Terminators are encoded as regular variants
/// (`Jump`, `JumpIf`, `IterNext`, `Return`) so the stream is uniform.
///
/// `Await` suspends the current task on the future value held in `src`.
/// The VM inspects the value's kind (e.g. `Sleep`, `SleepUnique`)
/// produced by a prior `Call` to dispatch the actual await.
#[derive(Debug, Clone)]
pub enum LirInstr {
    /// Marks a position in the stream that other instructions jump to.
    Label(Label),

    // === Constants ===
    ConstInt {
        dst: Reg,
        value: i64,
    },
    ConstFloat {
        dst: Reg,
        value: f64,
    },
    ConstString {
        dst: Reg,
        value: String,
    },
    ConstBool {
        dst: Reg,
        value: bool,
    },
    ConstUnit {
        dst: Reg,
        value: String,
        unit: ast::UnitType,
    },
    Unit {
        dst: Reg,
    },

    // === Binary / unary ===
    BinOp {
        dst: Reg,
        op: HirBinOp,
        lhs: Reg,
        rhs: Reg,
    },
    Neg {
        dst: Reg,
        src: Reg,
    },
    Not {
        dst: Reg,
        src: Reg,
    },
    Deref {
        dst: Reg,
        src: Reg,
    },

    // === Field access ===
    Field {
        dst: Reg,
        base: Reg,
        field: String,
    },
    OptionalField {
        dst: Reg,
        base: Reg,
        field: String,
    },

    // === Calls / variants ===
    Call {
        dst: Reg,
        name: String,
        args: Vec<Reg>,
    },
    Variant {
        dst: Reg,
        enum_name: String,
        variant: String,
        args: Vec<Reg>,
    },

    // === Collections ===
    EmptyList {
        dst: Reg,
    },
    List {
        dst: Reg,
        elems: Vec<Reg>,
    },
    ListPush {
        list: Reg,
        value: Reg,
    },
    IterInit {
        dst: Reg,
        src: Reg,
    },

    // === Struct construction ===
    Struct {
        dst: Reg,
        name: String,
        fields: Vec<LirStructField>,
    },

    /// Copy a register's value into another (used at merge points where
    /// HIR's `emit_into` writes into the same destination from
    /// multiple predecessors).
    Copy {
        dst: Reg,
        src: Reg,
    },

    // === Terminators ===
    Jump(Label),
    JumpIf {
        cond: Reg,
        then_lbl: Label,
        else_lbl: Label,
    },
    IterNext {
        iter: Reg,
        value: Reg,
        body_lbl: Label,
        exit_lbl: Label,
    },
    Return(Reg),

    /// Suspend on a future value produced by a prior `Call`. The VM
    /// dispatches based on the value's runtime kind.
    Await {
        dst: Reg,
        src: Reg,
    },
}

impl std::fmt::Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "L{}", self.0)
    }
}
