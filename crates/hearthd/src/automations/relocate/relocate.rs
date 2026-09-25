//! The relocation pass: `RelocatableProgram` → `BytecodeProgram`.
//!
//! Resolves every entity a compiled program names against one deployment.
//! The pass and the representation it produces are described by the module
//! and its [`bytecode`] submodule.

use super::bytecode::Bytecode;
use super::bytecode::BytecodeAutomation;
use super::bytecode::BytecodeProgram;
use crate::automations::bytecode::Const;
use crate::automations::bytecode::EntitySymbol;
use crate::automations::bytecode::RelocConst;
use crate::automations::bytecode::RelocatableAutomation;
use crate::automations::bytecode::RelocatableBytecode;
use crate::automations::bytecode::RelocatableProgram;
use crate::automations::entity_index::EntityIndex;

/// An entity the program names that the deployment does not have.
///
/// Carries the symbol whole, so the span can point a diagnostic back at the
/// name in the source that failed to resolve.
#[derive(Debug, Clone)]
pub struct Unresolved {
    pub symbol: EntitySymbol,
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no entity '{}' in this deployment", self.symbol)
    }
}

/// Resolve every entity a program names against `index`.
///
/// All symbols are attempted even after the first failure, so a program
/// naming several missing entities reports them together rather than one
/// per attempt.
pub fn relocate_program(
    program: &RelocatableProgram,
    index: &dyn EntityIndex,
) -> Result<BytecodeProgram, Vec<Unresolved>> {
    let mut unresolved = Vec::new();
    let relocated = match program {
        RelocatableProgram::Automation(auto) => {
            relocate_automation(auto, index, &mut unresolved).map(BytecodeProgram::Automation)
        }
        RelocatableProgram::Template {
            params,
            automations,
        } => {
            let relocated: Vec<_> = automations
                .iter()
                .map(|auto| relocate_automation(auto, index, &mut unresolved))
                .collect();
            relocated
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(|automations| BytecodeProgram::Template {
                    params: params.clone(),
                    automations,
                })
        }
    };

    match relocated {
        Some(program) if unresolved.is_empty() => Ok(program),
        _ => Err(unresolved),
    }
}

fn relocate_automation(
    auto: &RelocatableAutomation,
    index: &dyn EntityIndex,
    unresolved: &mut Vec<Unresolved>,
) -> Option<BytecodeAutomation> {
    // Both halves are relocated before either is inspected, so an
    // unresolved name in the filter does not hide one in the body.
    let filter = auto
        .filter
        .as_ref()
        .map(|f| relocate_function(f, index, unresolved));
    let body = relocate_function(&auto.body, index, unresolved)?;
    let filter = match filter {
        Some(filter) => Some(filter?),
        None => None,
    };
    Some(BytecodeAutomation {
        kind: auto.kind,
        filter,
        body,
    })
}

/// Rewrite one function's constant pool, leaving everything else alone.
///
/// Returns `None` if any symbol went unresolved. A `Bytecode` is never built
/// from a partially resolved pool: a `NodeId` is obtainable only from the
/// engine's allocator, and inventing a placeholder to fill the gap would put
/// an identifier naming nothing into a structure whose whole claim is that
/// every entity in it resolved.
fn relocate_function(
    func: &RelocatableBytecode,
    index: &dyn EntityIndex,
    unresolved: &mut Vec<Unresolved>,
) -> Option<Bytecode> {
    let mut consts = Vec::with_capacity(func.consts.len());
    let mut resolved = true;

    for konst in &func.consts {
        match konst {
            RelocConst::Resolved(konst) => consts.push(konst.clone()),
            RelocConst::Symbol(symbol) => match index.resolve(symbol.domain, &symbol.slug) {
                Some(node_id) => consts.push(Const::Node(node_id)),
                None => {
                    unresolved.push(Unresolved {
                        symbol: symbol.clone(),
                    });
                    resolved = false;
                }
            },
        }
    }

    resolved.then(|| Bytecode {
        params: func.params.clone(),
        num_regs: func.num_regs,
        consts,
        code: func.code.clone(),
    })
}
