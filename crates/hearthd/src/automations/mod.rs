//! HearthD Automations language parser and type checker.
//!
//! This module provides parsing and type checking for `.hda` automation files.

pub mod ast;
pub mod desugar;
pub mod lexer;
pub mod lowered_ast;
pub mod lowered_pretty_print;
pub mod parser;
pub mod pretty_print;

pub use ast::*;
pub use desugar::desugar;
pub use parser::parse;
pub use pretty_print::PrettyPrint;
