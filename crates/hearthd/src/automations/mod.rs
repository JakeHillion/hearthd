//! HearthD Automations language parser and type checker.
//!
//! This module provides parsing and type checking for `.hda` automation files.

pub mod bytecode;
pub mod check;
pub mod desugar;
pub mod domain;
pub mod entity_index;
pub mod hir;
pub mod lexer;
pub mod lir;
pub mod parser;
pub mod pretty_print;
pub mod relocate;
pub mod vm;

pub use bytecode::lower_bytecode_program;
pub use check::check_program;
pub use desugar::desugar;
pub use desugar::desugar_program;
pub use hir::lower_program;
pub use lir::lower_lir_program;
pub use parser::parse;
pub use pretty_print::PrettyPrint;
pub use relocate::relocate_program;
