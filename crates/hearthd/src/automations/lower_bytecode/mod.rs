//! Lowering pass: LIR → bytecode.
//!
//! Walks each `LirFunction` in two passes. The first pass emits the byte
//! stream with placeholder zeros for jump operands and records each label
//! id's resulting byte offset. The second pass walks the recorded
//! backpatch list and overwrites the placeholders with the actual byte
//! offsets.
//!
//! Constants are interned into the per-function pool: the encoder
//! deduplicates by structural value, so repeated literals or identifier
//! names share a single pool slot.

use std::collections::HashMap;

use crate::automations::repr::bytecode::*;
use crate::automations::repr::lir::*;

#[cfg(test)]
mod tests;

/// Lower an LIR program to bytecode.
pub fn lower_bytecode_program(lir: &LirProgram) -> BytecodeProgram {
    match lir {
        LirProgram::Automation(auto) => BytecodeProgram::Automation(lower_automation(auto)),
        LirProgram::Template {
            params,
            automations,
        } => BytecodeProgram::Template {
            params: params.clone(),
            automations: automations.iter().map(lower_automation).collect(),
        },
    }
}

fn lower_automation(auto: &LirAutomation) -> BytecodeAutomation {
    BytecodeAutomation {
        kind: auto.kind,
        filter: auto.filter.as_ref().map(lower_function),
        body: lower_function(&auto.body),
    }
}

fn lower_function(func: &LirFunction) -> Bytecode {
    let mut enc = Encoder::default();

    // Map label id → first byte offset that follows the label.
    let mut label_offsets: HashMap<usize, u32> = HashMap::new();

    for instr in &func.instrs {
        if let LirInstr::Label(lbl) = instr {
            label_offsets.insert(lbl.0, enc.code.len() as u32);
            continue;
        }
        emit(&mut enc, instr);
    }

    // Resolve each recorded jump target placeholder to its label's offset.
    for backpatch in &enc.backpatches {
        let offset = *label_offsets
            .get(&backpatch.label)
            .expect("undefined label in LIR");
        let bytes = offset.to_le_bytes();
        enc.code[backpatch.byte_pos..backpatch.byte_pos + 4].copy_from_slice(&bytes);
    }

    Bytecode {
        params: func
            .params
            .iter()
            .map(|p| BytecodeParam {
                name: p.name.clone(),
                reg: p.reg.0 as u32,
                ty: p.ty.clone(),
            })
            .collect(),
        num_regs: func.num_regs as u32,
        consts: enc.consts,
        code: enc.code,
    }
}

// ============================================================================
// Encoder
// ============================================================================

#[derive(Default)]
struct Encoder {
    code: Vec<u8>,
    consts: Vec<Const>,
    backpatches: Vec<Backpatch>,
    // Interning tables, keyed by structural value.
    int_idx: HashMap<i64, u32>,
    float_idx: HashMap<u64, u32>, // f64 bit pattern
    string_idx: HashMap<String, u32>,
    ident_idx: HashMap<String, u32>,
    unit_idx: HashMap<(String, crate::automations::repr::ast::UnitType), u32>,
}

struct Backpatch {
    label: usize,
    byte_pos: usize,
}

impl Encoder {
    fn write_u8(&mut self, v: u8) {
        self.code.push(v);
    }

    fn write_u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    fn write_reg(&mut self, r: Reg) {
        self.write_u32(r.0 as u32);
    }

    /// Reserve 4 bytes for a later-resolved jump target and record the
    /// label this slot points at.
    fn write_label_placeholder(&mut self, label: Label) {
        let byte_pos = self.code.len();
        self.write_u32(0);
        self.backpatches.push(Backpatch {
            label: label.0,
            byte_pos,
        });
    }

    fn intern_int(&mut self, v: i64) -> u32 {
        if let Some(&idx) = self.int_idx.get(&v) {
            return idx;
        }
        let idx = self.consts.len() as u32;
        self.consts.push(Const::Int(v));
        self.int_idx.insert(v, idx);
        idx
    }

    fn intern_float(&mut self, v: f64) -> u32 {
        let bits = v.to_bits();
        if let Some(&idx) = self.float_idx.get(&bits) {
            return idx;
        }
        let idx = self.consts.len() as u32;
        self.consts.push(Const::Float(v));
        self.float_idx.insert(bits, idx);
        idx
    }

    fn intern_string(&mut self, v: &str) -> u32 {
        if let Some(&idx) = self.string_idx.get(v) {
            return idx;
        }
        let idx = self.consts.len() as u32;
        self.consts.push(Const::String(v.to_string()));
        self.string_idx.insert(v.to_string(), idx);
        idx
    }

    fn intern_ident(&mut self, v: &str) -> u32 {
        if let Some(&idx) = self.ident_idx.get(v) {
            return idx;
        }
        let idx = self.consts.len() as u32;
        self.consts.push(Const::Ident(v.to_string()));
        self.ident_idx.insert(v.to_string(), idx);
        idx
    }

    fn intern_unit(&mut self, value: &str, unit: crate::automations::repr::ast::UnitType) -> u32 {
        let key = (value.to_string(), unit);
        if let Some(&idx) = self.unit_idx.get(&key) {
            return idx;
        }
        let idx = self.consts.len() as u32;
        self.consts.push(Const::UnitLit {
            value: value.to_string(),
            unit,
        });
        self.unit_idx.insert(key, idx);
        idx
    }
}

fn emit(enc: &mut Encoder, instr: &LirInstr) {
    match instr {
        LirInstr::Label(_) => unreachable!("labels are skipped in the encode loop"),
        LirInstr::ConstInt { dst, value } => {
            let idx = enc.intern_int(*value);
            enc.write_u8(Opcode::LoadConstInt as u8);
            enc.write_reg(*dst);
            enc.write_u32(idx);
        }
        LirInstr::ConstFloat { dst, value } => {
            let idx = enc.intern_float(*value);
            enc.write_u8(Opcode::LoadConstFloat as u8);
            enc.write_reg(*dst);
            enc.write_u32(idx);
        }
        LirInstr::ConstString { dst, value } => {
            let idx = enc.intern_string(value);
            enc.write_u8(Opcode::LoadConstString as u8);
            enc.write_reg(*dst);
            enc.write_u32(idx);
        }
        LirInstr::ConstBool { dst, value } => {
            enc.write_u8(Opcode::LoadConstBool as u8);
            enc.write_reg(*dst);
            enc.write_u8(u8::from(*value));
        }
        LirInstr::ConstUnit { dst, value, unit } => {
            let idx = enc.intern_unit(value, *unit);
            enc.write_u8(Opcode::LoadConstUnit as u8);
            enc.write_reg(*dst);
            enc.write_u32(idx);
        }
        LirInstr::Unit { dst } => {
            enc.write_u8(Opcode::Unit as u8);
            enc.write_reg(*dst);
        }
        LirInstr::BinOp { dst, op, lhs, rhs } => {
            enc.write_u8(Opcode::BinOp as u8);
            enc.write_reg(*dst);
            enc.write_u8(BinOpTag::from_hir(*op) as u8);
            enc.write_reg(*lhs);
            enc.write_reg(*rhs);
        }
        LirInstr::Neg { dst, src } => {
            enc.write_u8(Opcode::Neg as u8);
            enc.write_reg(*dst);
            enc.write_reg(*src);
        }
        LirInstr::Not { dst, src } => {
            enc.write_u8(Opcode::Not as u8);
            enc.write_reg(*dst);
            enc.write_reg(*src);
        }
        LirInstr::Deref { dst, src } => {
            enc.write_u8(Opcode::Deref as u8);
            enc.write_reg(*dst);
            enc.write_reg(*src);
        }
        LirInstr::Field { dst, base, field } => {
            let idx = enc.intern_ident(field);
            enc.write_u8(Opcode::Field as u8);
            enc.write_reg(*dst);
            enc.write_reg(*base);
            enc.write_u32(idx);
        }
        LirInstr::OptionalField { dst, base, field } => {
            let idx = enc.intern_ident(field);
            enc.write_u8(Opcode::OptionalField as u8);
            enc.write_reg(*dst);
            enc.write_reg(*base);
            enc.write_u32(idx);
        }
        LirInstr::Call { dst, name, args } => {
            let name_idx = enc.intern_ident(name);
            enc.write_u8(Opcode::Call as u8);
            enc.write_reg(*dst);
            enc.write_u32(name_idx);
            enc.write_u32(args.len() as u32);
            for a in args {
                enc.write_reg(*a);
            }
        }
        LirInstr::Variant {
            dst,
            enum_name,
            variant,
            args,
        } => {
            let enum_idx = enc.intern_ident(enum_name);
            let variant_idx = enc.intern_ident(variant);
            enc.write_u8(Opcode::Variant as u8);
            enc.write_reg(*dst);
            enc.write_u32(enum_idx);
            enc.write_u32(variant_idx);
            enc.write_u32(args.len() as u32);
            for a in args {
                enc.write_reg(*a);
            }
        }
        LirInstr::EmptyList { dst } => {
            enc.write_u8(Opcode::EmptyList as u8);
            enc.write_reg(*dst);
        }
        LirInstr::List { dst, elems } => {
            enc.write_u8(Opcode::List as u8);
            enc.write_reg(*dst);
            enc.write_u32(elems.len() as u32);
            for e in elems {
                enc.write_reg(*e);
            }
        }
        LirInstr::ListPush { list, value } => {
            enc.write_u8(Opcode::ListPush as u8);
            enc.write_reg(*list);
            enc.write_reg(*value);
        }
        LirInstr::IterInit { dst, src } => {
            enc.write_u8(Opcode::IterInit as u8);
            enc.write_reg(*dst);
            enc.write_reg(*src);
        }
        LirInstr::Struct { dst, name, fields } => {
            let name_idx = enc.intern_ident(name);
            enc.write_u8(Opcode::Struct as u8);
            enc.write_reg(*dst);
            enc.write_u32(name_idx);
            enc.write_u32(fields.len() as u32);
            for f in fields {
                match f {
                    LirStructField::Set { name, value } => {
                        enc.write_u8(StructFieldTag::Set as u8);
                        let field_idx = enc.intern_ident(name);
                        enc.write_u32(field_idx);
                        enc.write_reg(*value);
                    }
                    LirStructField::Spread(src) => {
                        enc.write_u8(StructFieldTag::Spread as u8);
                        enc.write_reg(*src);
                    }
                }
            }
        }
        LirInstr::Copy { dst, src } => {
            enc.write_u8(Opcode::Copy as u8);
            enc.write_reg(*dst);
            enc.write_reg(*src);
        }
        LirInstr::Jump(lbl) => {
            enc.write_u8(Opcode::Jump as u8);
            enc.write_label_placeholder(*lbl);
        }
        LirInstr::JumpIf {
            cond,
            then_lbl,
            else_lbl,
        } => {
            enc.write_u8(Opcode::JumpIf as u8);
            enc.write_reg(*cond);
            enc.write_label_placeholder(*then_lbl);
            enc.write_label_placeholder(*else_lbl);
        }
        LirInstr::IterNext {
            iter,
            value,
            body_lbl,
            exit_lbl,
        } => {
            enc.write_u8(Opcode::IterNext as u8);
            enc.write_reg(*iter);
            enc.write_reg(*value);
            enc.write_label_placeholder(*body_lbl);
            enc.write_label_placeholder(*exit_lbl);
        }
        LirInstr::Return(src) => {
            enc.write_u8(Opcode::Return as u8);
            enc.write_reg(*src);
        }
        LirInstr::Await { dst, src } => {
            enc.write_u8(Opcode::Await as u8);
            enc.write_reg(*dst);
            enc.write_reg(*src);
        }
    }
}
