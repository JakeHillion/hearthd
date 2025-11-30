use std::ops::Deref;
use std::ops::Range;

use serde::Deserialize;

use crate::SourceInfo;

/// A wrapper that associates a value with its source location information.
///
/// This type is similar to `toml::Spanned<T>` but also includes the source file
/// information, enabling rich diagnostics that show both the exact location in
/// the file (line/column) and which file the value came from.
///
/// The `Located<T>` type is automatically used by the `MergeableConfig` and
/// `SubConfig` derive macros for all fields in partial config types.
#[derive(Debug, Clone)]
pub struct Located<T> {
    /// The actual value
    value: T,
    /// Byte span in the source file
    span: Range<usize>,
    /// Source file information (path and content)
    source: SourceInfo,
}

impl<T> Located<T> {
    /// Create a new Located value with span and source information
    pub fn new(value: T, span: Range<usize>, source: SourceInfo) -> Self {
        Self {
            value,
            span,
            source,
        }
    }

    /// Get a reference to the inner value
    pub fn get_ref(&self) -> &T {
        &self.value
    }

    /// Consume the Located and return the inner value
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Get the byte span in the source file
    pub fn span(&self) -> &Range<usize> {
        &self.span
    }

    /// Get the source information
    pub fn source(&self) -> &SourceInfo {
        &self.source
    }

    /// Map the inner value while preserving location information
    pub fn map<U, F>(self, f: F) -> Located<U>
    where
        F: FnOnce(T) -> U,
    {
        Located {
            value: f(self.value),
            span: self.span,
            source: self.source,
        }
    }

    /// Update the source information (used during loading to attach file info)
    pub fn with_source(mut self, source: SourceInfo) -> Self {
        self.source = source;
        self
    }

    /// Convert this Located value into a MergeConflictLocation for diagnostics
    pub fn to_conflict_location(&self) -> crate::MergeConflictLocation {
        crate::MergeConflictLocation {
            file_path: self.source.file_path.clone(),
            span: self.span.clone(),
            content: self.source.content.clone(),
        }
    }
}

impl<T> Deref for Located<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

/// Custom deserialize implementation that reads from `toml::Spanned<T>`
///
/// During deserialization, we only have access to the span information from TOML.
/// The source information will be attached later during the loading process.
impl<'de, T> Deserialize<'de> for Located<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialize as toml::Spanned<T>
        let spanned = toml::Spanned::<T>::deserialize(deserializer)?;

        // Extract span before consuming spanned (need to clone since span() returns &Range)
        let span = spanned.span().clone();

        // Create a placeholder source - this will be replaced during loading
        let source = SourceInfo {
            file_path: std::path::PathBuf::from("<unknown>"),
            content: String::new(),
        };

        Ok(Located {
            value: spanned.into_inner(),
            span,
            source,
        })
    }
}

impl<T: PartialEq> PartialEq for Located<T> {
    fn eq(&self, other: &Self) -> bool {
        // Only compare values, not location information
        self.value == other.value
    }
}

impl<T: Eq> Eq for Located<T> {}
