//! Pretty-printer for the relocated bytecode.
//!
//! Only the constant pool is this module's own. Everything else a listing
//! shows — the register count, the parameters, the decoded instruction
//! stream — is printed by [`crate::automations::bytecode`]'s disassembler,
//! which owns the encoding, so the two forms cannot drift apart in how they
//! render the same bytes.

use super::bytecode::Bytecode;
use super::bytecode::BytecodeAutomation;
use super::bytecode::BytecodeProgram;
use crate::automations::bytecode::brief;
use crate::automations::bytecode::verbose;
use crate::automations::bytecode::write_automation;
use crate::automations::bytecode::write_function;
use crate::automations::bytecode::write_template;
use crate::automations::pretty_print::PrettyPrint;

impl PrettyPrint for BytecodeProgram {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeProgram::Automation(auto) => auto.pretty_print(indent, f),
            BytecodeProgram::Template {
                params,
                automations,
            } => write_template(params, automations, indent, f),
        }
    }
}

impl PrettyPrint for BytecodeAutomation {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_automation(self.kind, self.filter.as_ref(), &self.body, indent, f)
    }
}

impl PrettyPrint for Bytecode {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let listing: Vec<String> = self.consts.iter().map(verbose).collect();
        let briefs: Vec<String> = self.consts.iter().map(brief).collect();
        write_function(
            &self.params,
            self.num_regs,
            &listing,
            &briefs,
            &self.code,
            indent,
            f,
        )
    }
}
