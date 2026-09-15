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
use super::function::FunctionIdentity;
use super::typed::Ty;

/// A typed binary operation in LIR, produced by monomorphising an HIR
/// [`super::hir::HirBinOp`] against the static types of its operands.
///
/// The numeric operations are split into `Int`/`Float` variants so the
/// operation itself dictates the types of its source registers — the VM no
/// longer has to break open the runtime `Value` to pick an overload. Mixed
/// `Int`/`Float` operands are promoted to a common float pair before the
/// op is emitted, so each variant is homogeneous.
///
/// `Eq`/`Ne`/`In` stay polymorphic: they are defined over scalars and
/// collections of scalars, so the VM still resolves them against runtime
/// values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LirBinOp {
    // Int arithmetic (checked) and comparisons.
    AddInt,
    SubInt,
    MulInt,
    DivInt,
    ModInt,
    LtInt,
    LeInt,
    GtInt,
    GeInt,
    // Float arithmetic (IEEE) and comparisons.
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,
    ModFloat,
    LtFloat,
    LeFloat,
    GtFloat,
    GeFloat,
    // Polymorphic equality / membership; the VM dispatches on values.
    Eq,
    Ne,
    In,
}

impl std::fmt::Display for LirBinOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use LirBinOp::*;
        match self {
            AddInt => write!(f, "add_int"),
            SubInt => write!(f, "sub_int"),
            MulInt => write!(f, "mul_int"),
            DivInt => write!(f, "div_int"),
            ModInt => write!(f, "mod_int"),
            LtInt => write!(f, "lt_int"),
            LeInt => write!(f, "le_int"),
            GtInt => write!(f, "gt_int"),
            GeInt => write!(f, "ge_int"),
            AddFloat => write!(f, "add_float"),
            SubFloat => write!(f, "sub_float"),
            MulFloat => write!(f, "mul_float"),
            DivFloat => write!(f, "div_float"),
            ModFloat => write!(f, "mod_float"),
            LtFloat => write!(f, "lt_float"),
            LeFloat => write!(f, "le_float"),
            GtFloat => write!(f, "gt_float"),
            GeFloat => write!(f, "ge_float"),
            Eq => write!(f, "eq"),
            Ne => write!(f, "ne"),
            In => write!(f, "in"),
        }
    }
}

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
        op: LirBinOp,
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
    /// Reinterpret an `Int` register as the `F64` of the same value.
    ///
    /// Emitted when a mixed `Int`/`Float` binop is monomorphised: the int
    /// operand is promoted to a float register so the typed float operation
    /// reads homogeneous sources. Overflows the int's precision beyond
    /// `2^53`, matching the checker's rule that a `Float` contaminates.
    ToFloat {
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
        function: FunctionIdentity,
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
