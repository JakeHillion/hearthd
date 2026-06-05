//! Relocation pass: `RelocatableProgram` → `BytecodeProgram`.
//!
//! Compilation stops at a program that names entities without knowing what
//! they are. This pass resolves those names against one deployment, and is
//! the only way to produce the [`Bytecode`] the VM runs.
//!
//! Keeping it separate is what makes a late device cheap. Zigbee2MQTT
//! discovery is not instant, and a house gains and loses devices while it is
//! running; an automation naming one that has not turned up yet fails to
//! relocate, not to compile. When it does turn up, the same compiled program
//! is relocated again against the new schema. Nothing is re-parsed, re-typed
//! or re-encoded.
//!
//! Only the constant pool is rewritten. The instruction stream is carried
//! across byte for byte, so what a deployment executes is what the compiler
//! emitted.

use crate::automations::bytecode::Bytecode;
use crate::automations::bytecode::BytecodeAutomation;
use crate::automations::bytecode::BytecodeProgram;
use crate::automations::bytecode::Const;
use crate::automations::bytecode::EntitySymbol;
use crate::automations::bytecode::RelocConst;
use crate::automations::bytecode::RelocatableAutomation;
use crate::automations::bytecode::RelocatableBytecode;
use crate::automations::bytecode::RelocatableProgram;
use crate::automations::schema::DeploymentSchema;

#[cfg(test)]
mod tests;

/// An entity the program names that the deployment does not have.
///
/// Carries the symbol whole, so the span can point a diagnostic back at the
/// name in the source that failed to resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub symbol: EntitySymbol,
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no entity '{}' in this deployment", self.symbol)
    }
}

/// Resolve every entity a program names against `schema`.
///
/// All symbols are attempted even after the first failure, so a program
/// naming several missing entities reports them together rather than one
/// per attempt.
pub fn relocate_program(
    program: &RelocatableProgram,
    schema: &DeploymentSchema,
) -> Result<BytecodeProgram, Vec<Unresolved>> {
    let mut unresolved = Vec::new();
    let relocated = match program {
        RelocatableProgram::Automation(auto) => {
            relocate_automation(auto, schema, &mut unresolved).map(BytecodeProgram::Automation)
        }
        RelocatableProgram::Template {
            params,
            automations,
        } => {
            let relocated: Vec<_> = automations
                .iter()
                .map(|auto| relocate_automation(auto, schema, &mut unresolved))
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
    schema: &DeploymentSchema,
    unresolved: &mut Vec<Unresolved>,
) -> Option<BytecodeAutomation> {
    // Both halves are relocated before either is inspected, so an
    // unresolved name in the filter does not hide one in the body.
    let filter = auto
        .filter
        .as_ref()
        .map(|f| relocate_function(f, schema, unresolved));
    let body = relocate_function(&auto.body, schema, unresolved)?;
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
    schema: &DeploymentSchema,
    unresolved: &mut Vec<Unresolved>,
) -> Option<Bytecode> {
    let mut consts = Vec::with_capacity(func.consts.len());
    let mut resolved = true;

    for konst in &func.consts {
        match konst {
            RelocConst::Resolved(konst) => consts.push(konst.clone()),
            RelocConst::Symbol(symbol) => match schema.lookup(symbol.domain, &symbol.slug) {
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
