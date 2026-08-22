//! LIR (Low-level IR) types for the HearthD Automations language.
//!
//! The LIR is a linear instruction stream with a constant pool and symbol
//! table. It is produced by lowering the HIR. Symbolic names are replaced
//! with indices, and basic blocks are flattened into a single instruction
//! sequence with explicit jump targets.

use super::ast;

// ============================================================================
// Index newtypes — all u16
// ============================================================================

/// Index into the constant pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstIdx(pub u16);

/// Index into the symbol table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymIdx(pub u16);

/// A virtual register (mapped 1:1 from HIR temporaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg(pub u16);

/// An instruction offset used as a jump target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Label(pub u16);

// ============================================================================
// Constant pool
// ============================================================================

/// A compile-time constant value.
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Unit { value: String, unit: ast::UnitType },
    Void,
}

// Manual Hash/Eq using f64::to_bits() so floats can be deduped.
impl PartialEq for Constant {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Constant::Int(a), Constant::Int(b)) => a == b,
            (Constant::Float(a), Constant::Float(b)) => a.to_bits() == b.to_bits(),
            (Constant::String(a), Constant::String(b)) => a == b,
            (Constant::Bool(a), Constant::Bool(b)) => a == b,
            (
                Constant::Unit {
                    value: va,
                    unit: ua,
                },
                Constant::Unit {
                    value: vb,
                    unit: ub,
                },
            ) => va == vb && ua == ub,
            (Constant::Void, Constant::Void) => true,
            _ => false,
        }
    }
}

impl Eq for Constant {}

impl std::hash::Hash for Constant {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Constant::Int(n) => n.hash(state),
            Constant::Float(f) => f.to_bits().hash(state),
            Constant::String(s) => s.hash(state),
            Constant::Bool(b) => b.hash(state),
            Constant::Unit { value, unit } => {
                value.hash(state);
                unit.hash(state);
            }
            Constant::Void => {}
        }
    }
}

// ============================================================================
// Symbol table
// ============================================================================

/// A symbolic name (function, field, struct, enum, variant, parameter).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(pub String);

// ============================================================================
// Program structure
// ============================================================================

/// A fully lowered LIR program.
pub struct LirProgram {
    pub constant_pool: Vec<Constant>,
    pub symbol_table: Vec<Symbol>,
    pub automations: Vec<LirAutomation>,
}

/// A single automation in LIR form.
pub struct LirAutomation {
    pub kind: ast::AutomationKind,
    pub params: Vec<LirParam>,
    pub instructions: Vec<LirInstruction>,
    pub register_count: u16,
}

/// An automation parameter bound to a register.
pub struct LirParam {
    pub name: SymIdx,
    pub reg: Reg,
}

/// A struct field in LIR form.
pub enum LirStructField {
    /// An explicitly set field.
    Set { name: SymIdx, value: Reg },
    /// A spread from another struct.
    Spread(Reg),
}

// ============================================================================
// Instruction set
// ============================================================================

/// A single LIR instruction. All constants go through `LoadConst`, all
/// symbolic names are resolved to pool/table indices, and all control-flow
/// targets are `Label` instruction offsets.
pub enum LirInstruction {
    // === Constants ===
    LoadConst { dst: Reg, idx: ConstIdx },

    // === Arithmetic/Logic ===
    BinOp {
        dst: Reg,
        op: super::hir::HirBinOp,
        left: Reg,
        right: Reg,
    },
    Neg { dst: Reg, src: Reg },
    Not { dst: Reg, src: Reg },
    Deref { dst: Reg, src: Reg },
    Await { dst: Reg, src: Reg },

    // === Field access (field name → SymIdx) ===
    Field { dst: Reg, base: Reg, field: SymIdx },
    OptionalField { dst: Reg, base: Reg, field: SymIdx },

    // === Calls (func/variant names → SymIdx) ===
    Call { dst: Reg, func: SymIdx, args: Vec<Reg> },
    Variant {
        dst: Reg,
        enum_name: SymIdx,
        variant: SymIdx,
        args: Vec<Reg>,
    },

    // === Collections ===
    EmptyList { dst: Reg },
    List { dst: Reg, elements: Vec<Reg> },
    ListPush { dst: Reg, list: Reg, value: Reg },
    IterInit { dst: Reg, src: Reg },

    // === Struct construction ===
    Struct {
        dst: Reg,
        name: SymIdx,
        fields: Vec<LirStructField>,
    },

    // === Value ===
    Copy { dst: Reg, src: Reg },

    // === Control flow (from HIR terminators) ===
    Jump { target: Label },
    Branch {
        cond: Reg,
        then_target: Label,
        else_target: Label,
    },
    Return { src: Reg },
    IterNext {
        iter: Reg,
        value: Reg,
        body: Label,
        exit: Label,
    },
}
