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

use super::ast;
use super::hir::HirBinOp;
use super::typed::Ty;

// ============================================================================
// Opcode tags
// ============================================================================

/// One byte per opcode. Numeric values are stable — they are written into
/// the byte stream and decoded by the VM and disassembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    LoadConstInt = 0x01,
    LoadConstFloat = 0x02,
    LoadConstString = 0x03,
    LoadConstBool = 0x04,
    LoadConstUnit = 0x05,
    Unit = 0x06,
    BinOp = 0x10,
    Neg = 0x11,
    Not = 0x12,
    Deref = 0x13,
    Field = 0x20,
    OptionalField = 0x21,
    Call = 0x30,
    Variant = 0x31,
    EmptyList = 0x40,
    List = 0x41,
    ListPush = 0x42,
    IterInit = 0x43,
    Struct = 0x50,
    Copy = 0x60,
    Jump = 0x70,
    JumpIf = 0x71,
    IterNext = 0x72,
    Return = 0x73,
    Await = 0x80,
}

impl Opcode {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Opcode::LoadConstInt,
            0x02 => Opcode::LoadConstFloat,
            0x03 => Opcode::LoadConstString,
            0x04 => Opcode::LoadConstBool,
            0x05 => Opcode::LoadConstUnit,
            0x06 => Opcode::Unit,
            0x10 => Opcode::BinOp,
            0x11 => Opcode::Neg,
            0x12 => Opcode::Not,
            0x13 => Opcode::Deref,
            0x20 => Opcode::Field,
            0x21 => Opcode::OptionalField,
            0x30 => Opcode::Call,
            0x31 => Opcode::Variant,
            0x40 => Opcode::EmptyList,
            0x41 => Opcode::List,
            0x42 => Opcode::ListPush,
            0x43 => Opcode::IterInit,
            0x50 => Opcode::Struct,
            0x60 => Opcode::Copy,
            0x70 => Opcode::Jump,
            0x71 => Opcode::JumpIf,
            0x72 => Opcode::IterNext,
            0x73 => Opcode::Return,
            0x80 => Opcode::Await,
            _ => return None,
        })
    }
}

/// Tag byte for `BinOp` instructions. Stable values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BinOpTag {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
    Mod = 4,
    Eq = 5,
    Ne = 6,
    Lt = 7,
    Le = 8,
    Gt = 9,
    Ge = 10,
    In = 11,
}

impl BinOpTag {
    pub fn from_hir(op: HirBinOp) -> Self {
        match op {
            HirBinOp::Add => BinOpTag::Add,
            HirBinOp::Sub => BinOpTag::Sub,
            HirBinOp::Mul => BinOpTag::Mul,
            HirBinOp::Div => BinOpTag::Div,
            HirBinOp::Mod => BinOpTag::Mod,
            HirBinOp::Eq => BinOpTag::Eq,
            HirBinOp::Ne => BinOpTag::Ne,
            HirBinOp::Lt => BinOpTag::Lt,
            HirBinOp::Le => BinOpTag::Le,
            HirBinOp::Gt => BinOpTag::Gt,
            HirBinOp::Ge => BinOpTag::Ge,
            HirBinOp::In => BinOpTag::In,
        }
    }

    pub fn to_hir(self) -> HirBinOp {
        match self {
            BinOpTag::Add => HirBinOp::Add,
            BinOpTag::Sub => HirBinOp::Sub,
            BinOpTag::Mul => HirBinOp::Mul,
            BinOpTag::Div => HirBinOp::Div,
            BinOpTag::Mod => HirBinOp::Mod,
            BinOpTag::Eq => HirBinOp::Eq,
            BinOpTag::Ne => HirBinOp::Ne,
            BinOpTag::Lt => HirBinOp::Lt,
            BinOpTag::Le => HirBinOp::Le,
            BinOpTag::Gt => HirBinOp::Gt,
            BinOpTag::Ge => HirBinOp::Ge,
            BinOpTag::In => HirBinOp::In,
        }
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => BinOpTag::Add,
            1 => BinOpTag::Sub,
            2 => BinOpTag::Mul,
            3 => BinOpTag::Div,
            4 => BinOpTag::Mod,
            5 => BinOpTag::Eq,
            6 => BinOpTag::Ne,
            7 => BinOpTag::Lt,
            8 => BinOpTag::Le,
            9 => BinOpTag::Gt,
            10 => BinOpTag::Ge,
            11 => BinOpTag::In,
            _ => return None,
        })
    }
}

/// Tag byte for struct field entries inside a `Struct` instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
