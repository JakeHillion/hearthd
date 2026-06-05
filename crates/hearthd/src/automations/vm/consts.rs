//! The constant pool in the form opcodes read it.

use super::error::VmError;
use super::quantity::Quantity;
use crate::automations::repr::bytecode::Const;

/// One constant-pool slot, decoded into the form opcodes actually read.
///
/// The encoded [`Const`] keeps a unit literal as the text the author wrote
/// plus its `UnitType`, because that is what the disassembler renders. No
/// opcode wants that: they want the canonical magnitude. Converting once at
/// construction rather than per execution is the same reasoning that drops
/// parameter names and `num_regs` — a `Vm` keeps only what opcodes read.
/// Without it a filter carrying `5min` would re-parse a string on every
/// event.
#[derive(Debug, Clone)]
pub(super) enum VmConst {
    Int(i64),
    Float(f64),
    String(String),
    Ident(String),
    Quantity(Quantity),
}

impl TryFrom<Const> for VmConst {
    type Error = VmError;

    /// Decode one pool slot into the form opcodes read.
    ///
    /// Fallible because a unit literal an author can write may not fit its
    /// dimension's canonical unit: `1000000000d` type-checks and overflows
    /// `i64` nanoseconds. That is the automation's own doing, so it is an
    /// [`VmError::Overflow`] reported when the automation is built rather
    /// than a panic. The magnitude cannot fail to *parse* — the lexer only
    /// ever emits a numeric literal — so overflow is the only real case.
    fn try_from(c: Const) -> Result<Self, VmError> {
        Ok(match c {
            Const::Int(n) => VmConst::Int(n),
            Const::Float(n) => VmConst::Float(n),
            Const::String(s) => VmConst::String(s),
            Const::Ident(s) => VmConst::Ident(s),
            Const::UnitLit { value, unit } => {
                VmConst::Quantity(Quantity::from_unit_literal(unit, &value).ok_or_else(|| {
                    VmError::Overflow(format!("unit literal `{}{}`", value, unit))
                })?)
            }
        })
    }
}
