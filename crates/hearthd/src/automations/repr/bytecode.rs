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

use super::hir::HirBinOp;
use super::hir::NumTy;
use crate::automations::check::function::FunctionIdentity;
use crate::automations::check::typed::Ty;
use crate::automations::lexer::UnitType;
use crate::automations::parser::ast;

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
///
/// An operation that the language defines over both `Int` and `Float` gets
/// one opcode per type rather than a shared opcode and a type operand. The
/// opcode is then the whole decision: the VM reads it and knows what its
/// registers hold, with no second byte to branch on and nothing to inspect
/// at the values themselves.
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

    // === 0x1_: unary operators, and the binary ones that stay polymorphic ===
    //
    // 0x10 is retired: it held the single `BinOp` from before numeric
    // operations were specialised.
    Neg = 0x11,
    Not = 0x12,
    Deref = 0x13,
    ToFloat = 0x14,
    Eq = 0x15,
    Ne = 0x16,
    In = 0x17,

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

    // === 0x9_: arithmetic and ordering over Int ===
    //
    // The low nibble names the operator and is shared with the 0xA_ group,
    // so an opcode's dimension and its operator read off the two nibbles
    // independently: 0x93 and 0xA3 are the same division, on either type.
    AddInt = 0x90,
    SubInt = 0x91,
    MulInt = 0x92,
    DivInt = 0x93,
    ModInt = 0x94,
    LtInt = 0x95,
    LeInt = 0x96,
    GtInt = 0x97,
    GeInt = 0x98,

    // === 0xA_: arithmetic and ordering over Float ===
    AddFloat = 0xA0,
    SubFloat = 0xA1,
    MulFloat = 0xA2,
    DivFloat = 0xA3,
    ModFloat = 0xA4,
    LtFloat = 0xA5,
    LeFloat = 0xA6,
    GtFloat = 0xA7,
    GeFloat = 0xA8,
}

impl Opcode {
    /// The opcode `op` specialised to `ty` encodes as.
    pub fn typed_binop(op: HirBinOp, ty: NumTy) -> Opcode {
        match (op, ty) {
            (HirBinOp::Add, NumTy::Int) => Opcode::AddInt,
            (HirBinOp::Sub, NumTy::Int) => Opcode::SubInt,
            (HirBinOp::Mul, NumTy::Int) => Opcode::MulInt,
            (HirBinOp::Div, NumTy::Int) => Opcode::DivInt,
            (HirBinOp::Mod, NumTy::Int) => Opcode::ModInt,
            (HirBinOp::Lt, NumTy::Int) => Opcode::LtInt,
            (HirBinOp::Le, NumTy::Int) => Opcode::LeInt,
            (HirBinOp::Gt, NumTy::Int) => Opcode::GtInt,
            (HirBinOp::Ge, NumTy::Int) => Opcode::GeInt,
            (HirBinOp::Add, NumTy::Float) => Opcode::AddFloat,
            (HirBinOp::Sub, NumTy::Float) => Opcode::SubFloat,
            (HirBinOp::Mul, NumTy::Float) => Opcode::MulFloat,
            (HirBinOp::Div, NumTy::Float) => Opcode::DivFloat,
            (HirBinOp::Mod, NumTy::Float) => Opcode::ModFloat,
            (HirBinOp::Lt, NumTy::Float) => Opcode::LtFloat,
            (HirBinOp::Le, NumTy::Float) => Opcode::LeFloat,
            (HirBinOp::Gt, NumTy::Float) => Opcode::GtFloat,
            (HirBinOp::Ge, NumTy::Float) => Opcode::GeFloat,
            (op, ty) => unreachable!("{:?} has no {:?} specialisation", op, ty),
        }
    }

    /// The opcode `op` encodes as when its operands keep whatever type
    /// they hold.
    pub fn binop(op: HirBinOp) -> Opcode {
        match op {
            HirBinOp::Eq => Opcode::Eq,
            HirBinOp::Ne => Opcode::Ne,
            HirBinOp::In => Opcode::In,
            op => unreachable!("{:?} is only encoded with an operand type", op),
        }
    }
}

/// Tag byte identifying the function a `Call` instruction targets. Stable
/// values, mirroring [`FunctionIdentity`].
///
/// A tag rather than an opcode per function, unlike the numeric operators:
/// a builtin's identity does not tell the VM what its registers hold, so
/// there is nothing for the opcode to commit to.
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
        unit: UnitType,
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
