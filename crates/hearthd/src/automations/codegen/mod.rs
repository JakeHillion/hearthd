//! HIR → LIR lowering pass (codegen).
//!
//! Transforms the HIR control-flow graph into a linear LIR instruction stream
//! with a constant pool and symbol table. Basic blocks are flattened, symbolic
//! names become table indices, and block targets become instruction offsets.

use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::automations::repr::hir::*;
use crate::automations::repr::lir::*;

#[cfg(test)]
mod tests;

// ============================================================================
// Public API
// ============================================================================

/// Lower a type-checked HIR program to LIR.
pub fn codegen_program(hir: &HirProgram) -> Result<LirProgram> {
    let mut ctx = Codegen::new();
    let automations = match hir {
        HirProgram::Automation(auto) => vec![ctx.lower_automation(auto)?],
        HirProgram::Template { automations, .. } => automations
            .iter()
            .map(|auto| ctx.lower_automation(auto))
            .collect::<Result<Vec<_>>>()?,
    };
    Ok(LirProgram {
        constant_pool: ctx.constants,
        symbol_table: ctx.symbols,
        automations,
    })
}

// ============================================================================
// Shared context: constant pool + symbol table with dedup
// ============================================================================

struct Codegen {
    constants: Vec<Constant>,
    constant_map: HashMap<Constant, ConstIdx>,
    symbols: Vec<Symbol>,
    symbol_map: HashMap<String, SymIdx>,
}

impl Codegen {
    fn new() -> Self {
        Self {
            constants: Vec::new(),
            constant_map: HashMap::new(),
            symbols: Vec::new(),
            symbol_map: HashMap::new(),
        }
    }

    fn intern_constant(&mut self, c: Constant) -> Result<ConstIdx> {
        if let Some(&idx) = self.constant_map.get(&c) {
            return Ok(idx);
        }
        let idx = ConstIdx(
            u16::try_from(self.constants.len()).context("constant pool overflow (>65535)")?,
        );
        self.constant_map.insert(c.clone(), idx);
        self.constants.push(c);
        Ok(idx)
    }

    fn intern_symbol(&mut self, name: &str) -> Result<SymIdx> {
        if let Some(&idx) = self.symbol_map.get(name) {
            return Ok(idx);
        }
        let idx =
            SymIdx(u16::try_from(self.symbols.len()).context("symbol table overflow (>65535)")?);
        self.symbol_map.insert(name.to_string(), idx);
        self.symbols.push(Symbol(name.to_string()));
        Ok(idx)
    }

    fn lower_automation(&mut self, auto: &HirAutomation) -> Result<LirAutomation> {
        let mut emitter = AutomationEmitter::new();

        // Record block start offsets (first pass: emit instructions).
        let mut block_starts: Vec<Option<Label>> = vec![None; auto.blocks.len()];

        for block in &auto.blocks {
            let offset =
                u16::try_from(emitter.instructions.len()).context("instruction count overflow")?;
            block_starts[block.id.0] = Some(Label(offset));

            // Emit instructions for this block.
            for instr in &block.instructions {
                let dst = tmp_to_reg(instr.dst)?;
                self.emit_op(&mut emitter, dst, &instr.op)?;
            }

            // Emit terminator with placeholder labels.
            self.emit_terminator(&mut emitter, &block.terminator)?;
        }

        // Fixup pass: resolve block targets to instruction offsets.
        for fixup in &emitter.fixups {
            let label = block_starts[fixup.block_id.0]
                .expect("block_starts should be populated for all blocks");
            match &fixup.kind {
                FixupKind::Jump => {
                    emitter.instructions[fixup.instr_idx] = LirInstruction::Jump { target: label };
                }
                FixupKind::BranchThen { cond, else_target } => {
                    // The else_target was already resolved by a separate fixup.
                    // We need to reconstruct: get the current else_target if already fixed.
                    let current_else = match &emitter.instructions[fixup.instr_idx] {
                        LirInstruction::Branch { else_target, .. } => *else_target,
                        _ => *else_target,
                    };
                    emitter.instructions[fixup.instr_idx] = LirInstruction::Branch {
                        cond: *cond,
                        then_target: label,
                        else_target: current_else,
                    };
                }
                FixupKind::BranchElse { cond, then_target } => {
                    let current_then = match &emitter.instructions[fixup.instr_idx] {
                        LirInstruction::Branch { then_target, .. } => *then_target,
                        _ => *then_target,
                    };
                    emitter.instructions[fixup.instr_idx] = LirInstruction::Branch {
                        cond: *cond,
                        then_target: current_then,
                        else_target: label,
                    };
                }
                FixupKind::IterNextBody { iter, value, exit } => {
                    let current_exit = match &emitter.instructions[fixup.instr_idx] {
                        LirInstruction::IterNext { exit, .. } => *exit,
                        _ => *exit,
                    };
                    emitter.instructions[fixup.instr_idx] = LirInstruction::IterNext {
                        iter: *iter,
                        value: *value,
                        body: label,
                        exit: current_exit,
                    };
                }
                FixupKind::IterNextExit { iter, value, body } => {
                    let current_body = match &emitter.instructions[fixup.instr_idx] {
                        LirInstruction::IterNext { body, .. } => *body,
                        _ => *body,
                    };
                    emitter.instructions[fixup.instr_idx] = LirInstruction::IterNext {
                        iter: *iter,
                        value: *value,
                        body: current_body,
                        exit: label,
                    };
                }
            }
        }

        // Build params.
        let params = auto
            .params
            .iter()
            .map(|p| {
                Ok(LirParam {
                    name: self.intern_symbol(&p.name)?,
                    reg: tmp_to_reg(p.tmp)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // Compute register count = max Tmp seen + 1.
        let register_count = emitter.max_reg + 1;

        Ok(LirAutomation {
            kind: auto.kind,
            params,
            instructions: emitter.instructions,
            register_count,
        })
    }

    fn emit_op(&mut self, emitter: &mut AutomationEmitter, dst: Reg, op: &Op) -> Result<()> {
        emitter.track_reg(dst);
        let instr = match op {
            Op::ConstInt(n) => {
                let idx = self.intern_constant(Constant::Int(*n))?;
                LirInstruction::LoadConst { dst, idx }
            }
            Op::ConstFloat(n) => {
                let idx = self.intern_constant(Constant::Float(*n))?;
                LirInstruction::LoadConst { dst, idx }
            }
            Op::ConstString(s) => {
                let idx = self.intern_constant(Constant::String(s.clone()))?;
                LirInstruction::LoadConst { dst, idx }
            }
            Op::ConstBool(b) => {
                let idx = self.intern_constant(Constant::Bool(*b))?;
                LirInstruction::LoadConst { dst, idx }
            }
            Op::ConstUnit { value, unit } => {
                let idx = self.intern_constant(Constant::Unit {
                    value: value.clone(),
                    unit: *unit,
                })?;
                LirInstruction::LoadConst { dst, idx }
            }
            Op::Unit => {
                let idx = self.intern_constant(Constant::Void)?;
                LirInstruction::LoadConst { dst, idx }
            }
            Op::BinOp { op, left, right } => LirInstruction::BinOp {
                dst,
                op: *op,
                left: tmp_to_reg(*left)?,
                right: tmp_to_reg(*right)?,
            },
            Op::Neg(src) => LirInstruction::Neg {
                dst,
                src: tmp_to_reg(*src)?,
            },
            Op::Not(src) => LirInstruction::Not {
                dst,
                src: tmp_to_reg(*src)?,
            },
            Op::Deref(src) => LirInstruction::Deref {
                dst,
                src: tmp_to_reg(*src)?,
            },
            Op::Await(src) => LirInstruction::Await {
                dst,
                src: tmp_to_reg(*src)?,
            },
            Op::Field { base, field } => LirInstruction::Field {
                dst,
                base: tmp_to_reg(*base)?,
                field: self.intern_symbol(field)?,
            },
            Op::OptionalField { base, field } => LirInstruction::OptionalField {
                dst,
                base: tmp_to_reg(*base)?,
                field: self.intern_symbol(field)?,
            },
            Op::Call { name, args } => LirInstruction::Call {
                dst,
                func: self.intern_symbol(name)?,
                args: args
                    .iter()
                    .map(|t| tmp_to_reg(*t))
                    .collect::<Result<Vec<_>>>()?,
            },
            Op::Variant {
                enum_name,
                variant,
                args,
            } => LirInstruction::Variant {
                dst,
                enum_name: self.intern_symbol(enum_name)?,
                variant: self.intern_symbol(variant)?,
                args: args
                    .iter()
                    .map(|t| tmp_to_reg(*t))
                    .collect::<Result<Vec<_>>>()?,
            },
            Op::EmptyList => LirInstruction::EmptyList { dst },
            Op::List(elems) => LirInstruction::List {
                dst,
                elements: elems
                    .iter()
                    .map(|t| tmp_to_reg(*t))
                    .collect::<Result<Vec<_>>>()?,
            },
            Op::ListPush { list, value } => LirInstruction::ListPush {
                dst,
                list: tmp_to_reg(*list)?,
                value: tmp_to_reg(*value)?,
            },
            Op::IterInit(src) => LirInstruction::IterInit {
                dst,
                src: tmp_to_reg(*src)?,
            },
            Op::Struct { name, fields } => LirInstruction::Struct {
                dst,
                name: self.intern_symbol(name)?,
                fields: fields
                    .iter()
                    .map(|f| match f {
                        HirStructField::Set { name, value } => Ok(LirStructField::Set {
                            name: self.intern_symbol(name)?,
                            value: tmp_to_reg(*value)?,
                        }),
                        HirStructField::Spread(tmp) => Ok(LirStructField::Spread(tmp_to_reg(*tmp)?)),
                    })
                    .collect::<Result<Vec<_>>>()?,
            },
            Op::Copy(src) => LirInstruction::Copy {
                dst,
                src: tmp_to_reg(*src)?,
            },
        };
        emitter.instructions.push(instr);
        Ok(())
    }

    fn emit_terminator(
        &mut self,
        emitter: &mut AutomationEmitter,
        term: &Terminator,
    ) -> Result<()> {
        let placeholder = Label(u16::MAX);
        let instr_idx = emitter.instructions.len();

        match term {
            Terminator::Jump(target) => {
                emitter
                    .instructions
                    .push(LirInstruction::Jump { target: placeholder });
                emitter.fixups.push(Fixup {
                    instr_idx,
                    block_id: *target,
                    kind: FixupKind::Jump,
                });
            }
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => {
                let cond_reg = tmp_to_reg(*cond)?;
                emitter.instructions.push(LirInstruction::Branch {
                    cond: cond_reg,
                    then_target: placeholder,
                    else_target: placeholder,
                });
                emitter.fixups.push(Fixup {
                    instr_idx,
                    block_id: *then_block,
                    kind: FixupKind::BranchThen {
                        cond: cond_reg,
                        else_target: placeholder,
                    },
                });
                emitter.fixups.push(Fixup {
                    instr_idx,
                    block_id: *else_block,
                    kind: FixupKind::BranchElse {
                        cond: cond_reg,
                        then_target: placeholder,
                    },
                });
            }
            Terminator::Return(tmp) => {
                let src = tmp_to_reg(*tmp)?;
                emitter.track_reg(src);
                emitter.instructions.push(LirInstruction::Return { src });
            }
            Terminator::IterNext {
                iter,
                value,
                body,
                exit,
            } => {
                let iter_reg = tmp_to_reg(*iter)?;
                let value_reg = tmp_to_reg(*value)?;
                emitter.track_reg(iter_reg);
                emitter.track_reg(value_reg);
                emitter.instructions.push(LirInstruction::IterNext {
                    iter: iter_reg,
                    value: value_reg,
                    body: placeholder,
                    exit: placeholder,
                });
                emitter.fixups.push(Fixup {
                    instr_idx,
                    block_id: *body,
                    kind: FixupKind::IterNextBody {
                        iter: iter_reg,
                        value: value_reg,
                        exit: placeholder,
                    },
                });
                emitter.fixups.push(Fixup {
                    instr_idx,
                    block_id: *exit,
                    kind: FixupKind::IterNextExit {
                        iter: iter_reg,
                        value: value_reg,
                        body: placeholder,
                    },
                });
            }
        }
        Ok(())
    }
}

// ============================================================================
// Per-automation emission state
// ============================================================================

struct AutomationEmitter {
    instructions: Vec<LirInstruction>,
    fixups: Vec<Fixup>,
    max_reg: u16,
}

impl AutomationEmitter {
    fn new() -> Self {
        Self {
            instructions: Vec::new(),
            fixups: Vec::new(),
            max_reg: 0,
        }
    }

    fn track_reg(&mut self, reg: Reg) {
        if reg.0 >= self.max_reg {
            self.max_reg = reg.0;
        }
    }
}

struct Fixup {
    instr_idx: usize,
    block_id: BlockId,
    kind: FixupKind,
}

enum FixupKind {
    Jump,
    BranchThen { cond: Reg, else_target: Label },
    BranchElse { cond: Reg, then_target: Label },
    IterNextBody { iter: Reg, value: Reg, exit: Label },
    IterNextExit { iter: Reg, value: Reg, body: Label },
}

// ============================================================================
// Helpers
// ============================================================================

fn tmp_to_reg(tmp: Tmp) -> Result<Reg> {
    Ok(Reg(
        u16::try_from(tmp.0).context("register index overflow (>65535)")?,
    ))
}
