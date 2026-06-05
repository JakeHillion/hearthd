//! HearthD Automations language parser and type checker.
//!
//! This module provides parsing and type checking for `.hda` automation files.

pub mod check;
pub mod desugar;
pub mod domain;
pub mod lexer;
pub mod lower;
pub mod lower_bytecode;
pub mod lower_lir;
pub mod parser;
pub mod relocate;
pub mod repr;
pub mod schema;
pub mod vm;

pub use check::check_program;
pub use desugar::desugar;
pub use desugar::desugar_program;
pub use lower::lower_program;
pub use lower_bytecode::lower_bytecode_program;
pub use lower_lir::lower_lir_program;
pub use parser::parse;
pub use relocate::relocate_program;
pub use repr::*;
