//! Bytecode encoding for the HearthD Automations language.
//!
//! `Bytecode` is the compact, encoded form of a [`super::lir::LirFunction`]
//! ready for VM consumption. Opcodes are a single byte; operands are
//! fixed-width little-endian `u32` register indices and constant-pool
//! indices. Jumps store the absolute byte offset of their target instead
//! of a label id, so the VM only needs `code` and `consts` to execute.
//!
//! Constants (ints, floats, strings, identifier names, unit literals) are
//! interned into a per-function pool keyed by the underlying value so
//! repeated literals don't bloat the stream.
//!
//! A disassembler (see `bytecode_pretty_print`) expands the byte stream
//! back into a readable form for snapshot tests.

use strum::FromRepr;

use super::ast;
use super::function::FunctionIdentity;
use super::lir::LirBinOp;
use super::typed::Ty;

// ============================================================================
// Opcode tags
// ============================================================================

/// One byte per opcode. Numeric values are stable — they are written into
/// the byte stream and decoded by the VM and disassembler.
///
/// The high nibble groups opcodes by category, so a raw byte in a dump is
/// categorisable at a glance. New opcodes are appended within their own
/// group to keep related values adjacent; the gaps exist to make that
/// possible without renumbering anything already encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum Opcode {
    // === 0x0_: load a constant or the unit value into a register ===
    LoadConstInt = 0x01,
    LoadConstFloat = 0x02,
    LoadConstString = 0x03,
    LoadConstBool = 0x04,
    LoadConstUnit = 0x05,
    Unit = 0x06,

    // === 0x1_: unary and binary operators ===
    BinOp = 0x10,
    Neg = 0x11,
    Not = 0x12,
    Deref = 0x13,
    ToFloat = 0x14,

    // === 0x2_: field access ===
    Field = 0x20,
    OptionalField = 0x21,

    // === 0x3_: calls and construction ===
    /// Callee is a `FunctionTag`, resolved by the checker.
    Call = 0x30,
    Variant = 0x31,

    // === 0x4_: list construction and iteration ===
    EmptyList = 0x40,
    List = 0x41,
    ListPush = 0x42,
    IterInit = 0x43,

    // === 0x5_: struct construction ===
    Struct = 0x50,

    // === 0x6_: register-to-register movement ===
    Copy = 0x60,

    // === 0x7_: control flow (the former LIR terminators) ===
    Jump = 0x70,
    JumpIf = 0x71,
    IterNext = 0x72,
    Return = 0x73,

    // === 0x8_: suspension ===
    Await = 0x80,
}

/// Tag byte for `BinOp` instructions. Stable values, mirroring the LIR
/// [`LirBinOp`]: the numeric operations are already monomorphised into
/// `Int`/`Float` pairs, so a tag fully dictates the operand types and the
/// VM need not inspect runtime `Value` tags to pick an overload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum BinOpTag {
    AddInt = 0,
    SubInt = 1,
    MulInt = 2,
    DivInt = 3,
    ModInt = 4,
    AddFloat = 5,
    SubFloat = 6,
    MulFloat = 7,
    DivFloat = 8,
    ModFloat = 9,
    LtInt = 10,
    LeInt = 11,
    GtInt = 12,
    GeInt = 13,
    LtFloat = 14,
    LeFloat = 15,
    GtFloat = 16,
    GeFloat = 17,
    Eq = 18,
    Ne = 19,
    In = 20,
}

impl From<LirBinOp> for BinOpTag {
    fn from(op: LirBinOp) -> Self {
        use LirBinOp::*;
        match op {
            AddInt => BinOpTag::AddInt,
            SubInt => BinOpTag::SubInt,
            MulInt => BinOpTag::MulInt,
            DivInt => BinOpTag::DivInt,
            ModInt => BinOpTag::ModInt,
            AddFloat => BinOpTag::AddFloat,
            SubFloat => BinOpTag::SubFloat,
            MulFloat => BinOpTag::MulFloat,
            DivFloat => BinOpTag::DivFloat,
            ModFloat => BinOpTag::ModFloat,
            LtInt => BinOpTag::LtInt,
            LeInt => BinOpTag::LeInt,
            GtInt => BinOpTag::GtInt,
            GeInt => BinOpTag::GeInt,
            LtFloat => BinOpTag::LtFloat,
            LeFloat => BinOpTag::LeFloat,
            GtFloat => BinOpTag::GtFloat,
            GeFloat => BinOpTag::GeFloat,
            Eq => BinOpTag::Eq,
            Ne => BinOpTag::Ne,
            In => BinOpTag::In,
        }
    }
}

impl From<BinOpTag> for LirBinOp {
    fn from(tag: BinOpTag) -> Self {
        use BinOpTag::*;
        match tag {
            AddInt => LirBinOp::AddInt,
            SubInt => LirBinOp::SubInt,
            MulInt => LirBinOp::MulInt,
            DivInt => LirBinOp::DivInt,
            ModInt => LirBinOp::ModInt,
            AddFloat => LirBinOp::AddFloat,
            SubFloat => LirBinOp::SubFloat,
            MulFloat => LirBinOp::MulFloat,
            DivFloat => LirBinOp::DivFloat,
            ModFloat => LirBinOp::ModFloat,
            LtInt => LirBinOp::LtInt,
            LeInt => LirBinOp::LeInt,
            GtInt => LirBinOp::GtInt,
            GeInt => LirBinOp::GeInt,
            LtFloat => LirBinOp::LtFloat,
            LeFloat => LirBinOp::LeFloat,
            GtFloat => LirBinOp::GtFloat,
            GeFloat => LirBinOp::GeFloat,
            Eq => LirBinOp::Eq,
            Ne => LirBinOp::Ne,
            In => LirBinOp::In,
        }
    }
}

/// Tag byte identifying the function a `Call` instruction targets. Stable
/// values, mirroring [`super::function::FunctionIdentity`] the way
/// [`BinOpTag`] mirrors [`LirBinOp`].
///
/// Calls are resolved by the checker, so the callee is a tag rather than a
/// constant-pool name: nothing below the checker looks a function up by
/// name, and the VM cannot fail to resolve one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum FunctionTag {
    Len = 0,
    Abs = 1,
    Min = 2,
    Max = 3,
    Clamp = 4,
    Keys = 5,
    Values = 6,
    Sleep = 7,
    SleepUnique = 8,
}

impl From<FunctionIdentity> for FunctionTag {
    fn from(function: FunctionIdentity) -> Self {
        match function {
            FunctionIdentity::Len => FunctionTag::Len,
            FunctionIdentity::Abs => FunctionTag::Abs,
            FunctionIdentity::Min => FunctionTag::Min,
            FunctionIdentity::Max => FunctionTag::Max,
            FunctionIdentity::Clamp => FunctionTag::Clamp,
            FunctionIdentity::Keys => FunctionTag::Keys,
            FunctionIdentity::Values => FunctionTag::Values,
            FunctionIdentity::Sleep => FunctionTag::Sleep,
            FunctionIdentity::SleepUnique => FunctionTag::SleepUnique,
        }
    }
}

impl From<FunctionTag> for FunctionIdentity {
    fn from(tag: FunctionTag) -> Self {
        match tag {
            FunctionTag::Len => FunctionIdentity::Len,
            FunctionTag::Abs => FunctionIdentity::Abs,
            FunctionTag::Min => FunctionIdentity::Min,
            FunctionTag::Max => FunctionIdentity::Max,
            FunctionTag::Clamp => FunctionIdentity::Clamp,
            FunctionTag::Keys => FunctionIdentity::Keys,
            FunctionTag::Values => FunctionIdentity::Values,
            FunctionTag::Sleep => FunctionIdentity::Sleep,
            FunctionTag::SleepUnique => FunctionIdentity::SleepUnique,
        }
    }
}

/// Tag byte for struct field entries inside a `Struct` instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum StructFieldTag {
    Set = 0,
    Spread = 1,
}

// ============================================================================
// Constant pool
// ============================================================================

/// One entry in a bytecode constant pool. `Float` is wrapped to expose
/// stable `Eq`/`Hash` (by bit pattern), so we can intern by value.
#[derive(Debug, Clone)]
pub enum Const {
    Int(i64),
    Float(f64),
    /// String literals (`"hello"`).
    String(String),
    /// Identifier names: builtin function names, enum names, variant names,
    /// struct names, and field accessors.
    Ident(String),
    UnitLit {
        value: String,
        unit: ast::UnitType,
    },
}

// ============================================================================
// Top-level bytecode
// ============================================================================

#[derive(Debug, Clone)]
pub struct BytecodeParam {
    pub name: String,
    pub reg: u32,
    pub ty: Ty,
}

/// A single compiled function ready for the VM.
#[derive(Debug, Clone)]
pub struct Bytecode {
    pub params: Vec<BytecodeParam>,
    pub num_regs: u32,
    pub consts: Vec<Const>,
    pub code: Vec<u8>,
}

/// A compiled automation: filter (optional) + body, both as `Bytecode`.
#[derive(Debug, Clone)]
pub struct BytecodeAutomation {
    pub kind: ast::AutomationKind,
    pub filter: Option<Bytecode>,
    pub body: Bytecode,
}

/// A compiled program.
#[derive(Debug, Clone)]
pub enum BytecodeProgram {
    Automation(BytecodeAutomation),
    Template {
        params: Vec<ast::Spanned<ast::TemplateParam>>,
        automations: Vec<BytecodeAutomation>,
    },
}
