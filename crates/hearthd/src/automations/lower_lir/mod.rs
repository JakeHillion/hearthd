//! Lowering pass: HIR basic blocks → LIR flat instruction stream.
//!
//! Each `HirFunction` becomes a `LirFunction`. Basic block terminators are
//! emitted as ordinary `LirInstr`s (`Jump`, `JumpIf`, `IterNext`, `Return`),
//! preceded by a `Label` for every block so jumps can resolve to positions
//! in the stream. HIR `Tmp(i)` maps directly to `Reg(i)`; the function
//! reports `num_regs = max_tmp + 1`.
//!
//! `Op::Await` lowers to `LirInstr::Await { dst, src }`. The decision of
//! *what* to await (a `tokio::time::sleep`, etc.) is made by the VM based
//! on the value flowing into `src`, which is produced by an earlier `Call`
//! to an async builtin.

use crate::automations::repr::hir::*;
use crate::automations::repr::lir::*;

#[cfg(test)]
mod tests;

/// Lower an HIR program to LIR.
pub fn lower_lir_program(hir: &HirProgram) -> LirProgram {
    match hir {
        HirProgram::Automation(auto) => LirProgram::Automation(lower_automation(auto)),
        HirProgram::Template {
            params,
            automations,
        } => LirProgram::Template {
            params: params.clone(),
            automations: automations.iter().map(lower_automation).collect(),
        },
    }
}

fn lower_automation(auto: &HirAutomation) -> LirAutomation {
    LirAutomation {
        kind: auto.kind,
        filter: auto.filter.as_ref().map(lower_function),
        body: lower_function(&auto.body),
    }
}

fn lower_function(func: &HirFunction) -> LirFunction {
    let mut instrs = Vec::new();
    let mut max_reg = 0;

    let params: Vec<LirParam> = func
        .params
        .iter()
        .map(|p| {
            max_reg = max_reg.max(p.tmp.0);
            LirParam {
                name: p.name.clone(),
                reg: Reg(p.tmp.0),
                ty: p.ty.clone(),
            }
        })
        .collect();

    for block in &func.blocks {
        instrs.push(LirInstr::Label(Label(block.id.0)));
        for instr in &block.instructions {
            max_reg = max_reg.max(instr.dst.0);
            for r in op_input_tmps(&instr.op) {
                max_reg = max_reg.max(r);
            }
            instrs.push(lower_instr(instr));
        }
        for r in terminator_input_tmps(&block.terminator) {
            max_reg = max_reg.max(r);
        }
        instrs.push(lower_terminator(&block.terminator));
    }

    LirFunction {
        params,
        num_regs: max_reg + 1,
        instrs,
    }
}

fn lower_instr(instr: &Instruction) -> LirInstr {
    let dst = Reg(instr.dst.0);
    match &instr.op {
        Op::ConstInt(value) => LirInstr::ConstInt { dst, value: *value },
        Op::ConstFloat(value) => LirInstr::ConstFloat { dst, value: *value },
        Op::ConstString(value) => LirInstr::ConstString {
            dst,
            value: value.clone(),
        },
        Op::ConstBool(value) => LirInstr::ConstBool { dst, value: *value },
        Op::ConstUnit { value, unit } => LirInstr::ConstUnit {
            dst,
            value: value.clone(),
            unit: *unit,
        },
        Op::Unit => LirInstr::Unit { dst },
        Op::BinOp { op, left, right } => LirInstr::BinOp {
            dst,
            op: *op,
            lhs: Reg(left.0),
            rhs: Reg(right.0),
        },
        Op::Neg(src) => LirInstr::Neg {
            dst,
            src: Reg(src.0),
        },
        Op::Not(src) => LirInstr::Not {
            dst,
            src: Reg(src.0),
        },
        Op::Deref(src) => LirInstr::Deref {
            dst,
            src: Reg(src.0),
        },
        Op::Await(src) => LirInstr::Await {
            dst,
            src: Reg(src.0),
        },
        Op::Field { base, field } => LirInstr::Field {
            dst,
            base: Reg(base.0),
            field: field.clone(),
        },
        Op::OptionalField { base, field } => LirInstr::OptionalField {
            dst,
            base: Reg(base.0),
            field: field.clone(),
        },
        Op::Call { name, args } => LirInstr::Call {
            dst,
            name: name.clone(),
            args: args.iter().map(|t| Reg(t.0)).collect(),
        },
        Op::Variant {
            enum_name,
            variant,
            args,
        } => LirInstr::Variant {
            dst,
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            args: args.iter().map(|t| Reg(t.0)).collect(),
        },
        Op::EmptyList => LirInstr::EmptyList { dst },
        Op::List(elems) => LirInstr::List {
            dst,
            elems: elems.iter().map(|t| Reg(t.0)).collect(),
        },
        Op::ListPush { list, value } => LirInstr::ListPush {
            list: Reg(list.0),
            value: Reg(value.0),
        },
        Op::IterInit(src) => LirInstr::IterInit {
            dst,
            src: Reg(src.0),
        },
        Op::Struct { name, fields } => LirInstr::Struct {
            dst,
            name: name.clone(),
            fields: fields
                .iter()
                .map(|f| match f {
                    HirStructField::Set { name, value } => LirStructField::Set {
                        name: name.clone(),
                        value: Reg(value.0),
                    },
                    HirStructField::Spread(src) => LirStructField::Spread(Reg(src.0)),
                })
                .collect(),
        },
        Op::Copy(src) => LirInstr::Copy {
            dst,
            src: Reg(src.0),
        },
    }
}

fn lower_terminator(term: &Terminator) -> LirInstr {
    match term {
        Terminator::Jump(target) => LirInstr::Jump(Label(target.0)),
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => LirInstr::JumpIf {
            cond: Reg(cond.0),
            then_lbl: Label(then_block.0),
            else_lbl: Label(else_block.0),
        },
        Terminator::Return(r) => LirInstr::Return(Reg(r.0)),
        Terminator::IterNext {
            iter,
            value,
            body,
            exit,
        } => LirInstr::IterNext {
            iter: Reg(iter.0),
            value: Reg(value.0),
            body_lbl: Label(body.0),
            exit_lbl: Label(exit.0),
        },
    }
}

/// Returns every `Tmp::0` value referenced as an input by an `Op`.
/// Used by `lower_function` to compute `num_regs`.
fn op_input_tmps(op: &Op) -> Vec<usize> {
    match op {
        Op::ConstInt(_)
        | Op::ConstFloat(_)
        | Op::ConstString(_)
        | Op::ConstBool(_)
        | Op::ConstUnit { .. }
        | Op::Unit
        | Op::EmptyList => Vec::new(),
        Op::BinOp { left, right, .. } => vec![left.0, right.0],
        Op::Neg(t) | Op::Not(t) | Op::Deref(t) | Op::Await(t) | Op::IterInit(t) | Op::Copy(t) => {
            vec![t.0]
        }
        Op::Field { base, .. } | Op::OptionalField { base, .. } => vec![base.0],
        Op::Call { args, .. } | Op::Variant { args, .. } => args.iter().map(|t| t.0).collect(),
        Op::List(elems) => elems.iter().map(|t| t.0).collect(),
        Op::ListPush { list, value } => vec![list.0, value.0],
        Op::Struct { fields, .. } => fields
            .iter()
            .map(|f| match f {
                HirStructField::Set { value, .. } => value.0,
                HirStructField::Spread(t) => t.0,
            })
            .collect(),
    }
}

fn terminator_input_tmps(term: &Terminator) -> Vec<usize> {
    match term {
        Terminator::Jump(_) => Vec::new(),
        Terminator::Branch { cond, .. } => vec![cond.0],
        Terminator::Return(t) => vec![t.0],
        Terminator::IterNext { iter, value, .. } => vec![iter.0, value.0],
    }
}
