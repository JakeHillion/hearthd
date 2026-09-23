//! Verbose, multi-line pretty-printing shared by every representation.
//!
//! Each representation implements [`PrettyPrint`] alongside its own types, so
//! a printer lives with the representation it prints rather than in a common
//! module. What is common is only the trait itself and the indentation helper
//! every implementation writes through, which is what this module holds.
//!
//! The output is deliberately verbose and unambiguous: it exists so snapshot
//! tests fail on a structural change rather than on a formatting one.

/// Trait for verbose, multi-line pretty-printing.
pub trait PrettyPrint {
    fn pretty_print(&self, indent: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;

    fn to_pretty_string(&self) -> String {
        struct Wrapper<'a, T: PrettyPrint + ?Sized>(&'a T);
        impl<T: PrettyPrint + ?Sized> std::fmt::Display for Wrapper<'_, T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.pretty_print(0, f)
            }
        }
        Wrapper(self).to_string()
    }
}

/// Write indentation (two spaces per level).
pub fn write_indent<W: std::fmt::Write>(indent: usize, f: &mut W) -> std::fmt::Result {
    for _ in 0..indent {
        write!(f, "  ")?;
    }
    Ok(())
}
