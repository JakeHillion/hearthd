//! HearthD Automations language parser and type checker.
//!
//! This module provides parsing and type checking for `.hda` automation files.

pub mod check;
pub mod desugar;
pub mod lexer;
pub mod parser;
pub mod repr;

pub use check::check_program;
pub use desugar::desugar;
pub use desugar::desugar_program;
pub use parser::parse;
pub use repr::*;
