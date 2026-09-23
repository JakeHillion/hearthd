#[allow(clippy::module_inception)]
mod lexer;
#[cfg(test)]
mod tests;

pub use lexer::Token;
pub use lexer::UnitType;
pub use lexer::lexer;
