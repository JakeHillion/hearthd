//! Lowering pass: HIR basic blocks → LIR flat instruction stream.
//!
//! Each `HirFunction` becomes a `LirFunction`. Basic block terminators are
//! emitted as ordinary `LirInstr`s (`Jump`, `JumpIf`, `IterNext`, `Return`),
//! preceded by a `Label` for every block so jumps can resolve to positions
//! in the stream. HIR `Tmp(i)` maps directly to `Reg(i)`.
//!
//! This pass also monomorphises the numeric binops against the static
//! operand types recovered from HIR (each `Tmp` is written by exactly one
//! `Instruction`, so its `ty` is unambiguous): an `Int`/`Int` pair becomes
//! an `add_int`, a `Float`-contaminated pair an `add_float` — promoting a
//! lone `Int` operand through a `ToFloat` first. Only `eq`/`ne`/`in` stay
//! polymorphic. This commits the operand types to the opcode itself, so
//! the VM no longer inspects runtime `Value` tags to pick an overload.
//! The promoted registers are fresh, allocated past the HIR temps, so
//! `num_regs` grows to include them.
//!
//! `Op::Await` lowers to `LirInstr::Await { dst, src }`. The decision of
//! *what* to await (a `tokio::time::sleep`, etc.) is made by the VM based
//! on the value flowing into `src`, which is produced by an earlier `Call`
//! to an async builtin.

use std::collections::HashMap;

use crate::automations::repr::hir::*;
use crate::automations::repr::lir::*;
use crate::automations::repr::typed::Ty;

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

    // Recover every `Tmp`'s static type and the register high-water mark in
    // one pre-pass. HIR is SSA, so each `Tmp` is written by exactly one
    // `Instruction` whose `ty` is that register's type — these are the types
    // the binop monomorphisation resolves against.
    let mut types: HashMap<Tmp, Ty> = HashMap::new();
    for p in &func.params {
        max_reg = max_reg.max(p.tmp.0);
        types.insert(p.tmp, p.ty.clone());
    }
    for block in &func.blocks {
        for instr in &block.instructions {
            max_reg = max_reg.max(instr.dst.0);
            for r in op_input_tmps(&instr.op) {
                max_reg = max_reg.max(r);
            }
            types.insert(instr.dst, instr.ty.clone());
        }
        for r in terminator_input_tmps(&block.terminator) {
            max_reg = max_reg.max(r);
        }
        // A loop variable is bound by the `IterNext` terminator, not an
        // `Instruction`, so it is not in the map yet. Its type is the
        // iterable's element type, which the iterator register does carry.
        if let Terminator::IterNext { iter, value, .. } = &block.terminator {
            max_reg = max_reg.max(value.0);
            if !types.contains_key(value) {
                types.insert(*value, element_ty(&types[iter]));
            }
        }
    }

    let params: Vec<LirParam> = func
        .params
        .iter()
        .map(|p| LirParam {
            name: p.name.clone(),
            reg: Reg(p.tmp.0),
            ty: p.ty.clone(),
        })
        .collect();

    // Fresh registers past the HIR temps, for `ToFloat` promotion slots.
    let mut next_free = max_reg + 1;

    for block in &func.blocks {
        instrs.push(LirInstr::Label(Label(block.id.0)));
        for instr in &block.instructions {
            lower_instr(instr, &types, &mut next_free, &mut instrs);
        }
        instrs.push(lower_terminator(&block.terminator));
    }

    LirFunction {
        params,
        num_regs: next_free,
        instrs,
    }
}

fn lower_instr(
    instr: &Instruction,
    types: &HashMap<Tmp, Ty>,
    next_free: &mut usize,
    out: &mut Vec<LirInstr>,
) {
    let dst = Reg(instr.dst.0);
    match &instr.op {
        Op::ConstInt(value) => out.push(LirInstr::ConstInt { dst, value: *value }),
        Op::ConstFloat(value) => out.push(LirInstr::ConstFloat { dst, value: *value }),
        Op::ConstString(value) => out.push(LirInstr::ConstString {
            dst,
            value: value.clone(),
        }),
        Op::ConstBool(value) => out.push(LirInstr::ConstBool { dst, value: *value }),
        Op::ConstUnit { value, unit } => out.push(LirInstr::ConstUnit {
            dst,
            value: value.clone(),
            unit: *unit,
        }),
        Op::Unit => out.push(LirInstr::Unit { dst }),
        Op::BinOp { op, left, right } => {
            let lhs = Reg(left.0);
            let rhs = Reg(right.0);
            match op {
                // Equality and membership are defined over scalars and
                // collections of scalars, so they stay polymorphic — the VM
                // resolves the overload against runtime values.
                HirBinOp::Eq => out.push(LirInstr::BinOp {
                    dst,
                    op: LirBinOp::Eq,
                    lhs,
                    rhs,
                }),
                HirBinOp::Ne => out.push(LirInstr::BinOp {
                    dst,
                    op: LirBinOp::Ne,
                    lhs,
                    rhs,
                }),
                HirBinOp::In => out.push(LirInstr::BinOp {
                    dst,
                    op: LirBinOp::In,
                    lhs,
                    rhs,
                }),
                op => {
                    // Numeric. `Float` on either side contaminates the
                    // result, so promote a lone `Int` operand to `Float`
                    // and emit the homogeneous float variant.
                    let left_ty = &types[left];
                    let right_ty = &types[right];
                    let is_float = *left_ty == Ty::Float || *right_ty == Ty::Float;
                    let (lhs, rhs) = (
                        if is_float && *left_ty == Ty::Int {
                            emit_to_float(lhs, next_free, out)
                        } else {
                            lhs
                        },
                        if is_float && *right_ty == Ty::Int {
                            emit_to_float(rhs, next_free, out)
                        } else {
                            rhs
                        },
                    );
                    out.push(LirInstr::BinOp {
                        dst,
                        op: numeric_lir_op(*op, is_float),
                        lhs,
                        rhs,
                    });
                }
            }
        }
        Op::Neg(src) => out.push(LirInstr::Neg {
            dst,
            src: Reg(src.0),
        }),
        Op::Not(src) => out.push(LirInstr::Not {
            dst,
            src: Reg(src.0),
        }),
        Op::Deref(src) => out.push(LirInstr::Deref {
            dst,
            src: Reg(src.0),
        }),
        Op::Await(src) => out.push(LirInstr::Await {
            dst,
            src: Reg(src.0),
        }),
        Op::Field { base, field } => out.push(LirInstr::Field {
            dst,
            base: Reg(base.0),
            field: field.clone(),
        }),
        Op::OptionalField { base, field } => out.push(LirInstr::OptionalField {
            dst,
            base: Reg(base.0),
            field: field.clone(),
        }),
        Op::Call { function, args } => out.push(LirInstr::Call {
            dst,
            function: *function,
            args: args.iter().map(|t| Reg(t.0)).collect(),
        }),
        Op::Variant {
            enum_name,
            variant,
            args,
        } => out.push(LirInstr::Variant {
            dst,
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            args: args.iter().map(|t| Reg(t.0)).collect(),
        }),
        Op::EmptyList => out.push(LirInstr::EmptyList { dst }),
        Op::List(elems) => out.push(LirInstr::List {
            dst,
            elems: elems.iter().map(|t| Reg(t.0)).collect(),
        }),
        Op::ListPush { list, value } => out.push(LirInstr::ListPush {
            list: Reg(list.0),
            value: Reg(value.0),
        }),
        Op::IterInit(src) => out.push(LirInstr::IterInit {
            dst,
            src: Reg(src.0),
        }),
        Op::Struct { name, fields } => out.push(LirInstr::Struct {
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
        }),
        Op::Copy(src) => out.push(LirInstr::Copy {
            dst,
            src: Reg(src.0),
        }),
    }
}

/// Allocate a fresh register and emit `ToFloat` from `src` into it.
///
/// Used to promote a lone `Int` operand to `Float` when the checker's
/// contamination rule makes a mixed binop a float one.
fn emit_to_float(src: Reg, next_free: &mut usize, out: &mut Vec<LirInstr>) -> Reg {
    let dst = Reg(*next_free);
    *next_free += 1;
    out.push(LirInstr::ToFloat { dst, src });
    dst
}

/// Map a numeric HIR binop and its (implicit, post-promotion) kind to a
/// monomorphised LIR op.
fn numeric_lir_op(op: HirBinOp, is_float: bool) -> LirBinOp {
    use LirBinOp::*;
    match (op, is_float) {
        (HirBinOp::Add, false) => AddInt,
        (HirBinOp::Add, true) => AddFloat,
        (HirBinOp::Sub, false) => SubInt,
        (HirBinOp::Sub, true) => SubFloat,
        (HirBinOp::Mul, false) => MulInt,
        (HirBinOp::Mul, true) => MulFloat,
        (HirBinOp::Div, false) => DivInt,
        (HirBinOp::Div, true) => DivFloat,
        (HirBinOp::Mod, false) => ModInt,
        (HirBinOp::Mod, true) => ModFloat,
        (HirBinOp::Lt, false) => LtInt,
        (HirBinOp::Lt, true) => LtFloat,
        (HirBinOp::Le, false) => LeInt,
        (HirBinOp::Le, true) => LeFloat,
        (HirBinOp::Gt, false) => GtInt,
        (HirBinOp::Gt, true) => GtFloat,
        (HirBinOp::Ge, false) => GeInt,
        (HirBinOp::Ge, true) => GeFloat,
        (HirBinOp::Eq | HirBinOp::Ne | HirBinOp::In, _) => {
            unreachable!("only numeric binops are monomorphised")
        }
    }
}

/// The element type of an iterable `Ty`, for typing the loop variable an
/// `IterNext` binds. Only `List`/`Set` can be iterated in this language, so
/// anything else is an unreachable broken compiler.
fn element_ty(ty: &Ty) -> Ty {
    match ty {
        Ty::List(elem) | Ty::Set(elem) => (**elem).clone(),
        other => unreachable!("iterating a non-iterable type {}", other),
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
