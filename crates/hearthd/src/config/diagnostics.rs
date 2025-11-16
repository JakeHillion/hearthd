use std::ops::Range;
use std::path::PathBuf;

/// Source information for where a diagnostic came from
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub file_path: PathBuf,
    pub content: String,
}

/// A diagnostic message that can be either a warning or an error
#[derive(Debug, Clone)]
pub enum Diagnostic {
    Warning(Warning),
    Error(Error),
}

/// Warning messages that don't prevent config loading
#[derive(Debug, Clone)]
pub enum Warning {
    EmptyConfig {
        file_path: PathBuf,
    },
}

/// Error messages that indicate problems with the config
#[derive(Debug, Clone)]
pub enum Error {
    Merge(MergeError),
    Validation(ValidationError),
}

/// Error type for merge conflicts
#[derive(Debug, Clone)]
pub struct MergeError {
    pub field_path: String,
    pub message: String,
    pub conflicts: Vec<MergeConflictLocation>,
}

#[derive(Debug, Clone)]
pub struct MergeConflictLocation {
    pub file_path: PathBuf,
    pub span: Range<usize>,
    pub content: String,
}

/// Error type for validation failures
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field_path: String,
    pub message: String,
    #[allow(dead_code)] // May be used in future for better error reporting
    pub span: Option<Range<usize>>,
    #[allow(dead_code)] // May be used in future for better error reporting
    pub source: Option<SourceInfo>,
}

/// Error type for config loading failures (parse errors, IO errors, etc.)
#[derive(Debug)]
pub enum LoadError {
    Io { path: PathBuf, error: std::io::Error },
    Parse { path: PathBuf, error: toml::de::Error },
    ImportCycle { path: PathBuf, cycle: Vec<PathBuf> },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io { path, error } => {
                write!(f, "Failed to read '{}': {}", path.display(), error)
            }
            LoadError::Parse { path, error } => {
                write!(f, "Failed to parse '{}': {}", path.display(), error)
            }
            LoadError::ImportCycle { path, cycle } => {
                write!(
                    f,
                    "Import cycle detected at '{}': involves {} file(s)",
                    path.display(),
                    cycle.len()
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

impl Diagnostic {
    /// Returns true if this diagnostic is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Diagnostic::Error(_))
    }

    /// Returns true if this diagnostic is a warning
    pub fn is_warning(&self) -> bool {
        matches!(self, Diagnostic::Warning(_))
    }
}

/// Format all diagnostics for display using Ariadne
pub fn format_diagnostics(diagnostics: &[Diagnostic]) -> String {
    use ariadne::{Color, Label, Report, ReportKind, Source};

    let mut output = Vec::new();

    for diagnostic in diagnostics {
        match diagnostic {
            Diagnostic::Warning(warning) => match warning {
                Warning::EmptyConfig { file_path } => {
                    let warning_msg = format!(
                        "Warning: Config file '{}' is empty and has no effect\n",
                        file_path.display()
                    );
                    output.extend_from_slice(warning_msg.as_bytes());
                }
            },
            Diagnostic::Error(error) => match error {
                Error::Merge(merge_error) => {
                    // Build a report with the first conflict's span
                    let first_conflict = &merge_error.conflicts[0];
                    let mut report = Report::build(
                        ReportKind::Error,
                        (
                            first_conflict.file_path.to_string_lossy().to_string(),
                            first_conflict.span.clone(),
                        ),
                    )
                    .with_message(format!("Merge conflict in field '{}'", merge_error.field_path))
                    .with_note(&merge_error.message);

                    // Add labels for each conflict location
                    for (idx, conflict) in merge_error.conflicts.iter().enumerate() {
                        let label_msg = if idx == 0 {
                            "first definition here"
                        } else {
                            "conflicts with this definition"
                        };

                        report = report.with_label(
                            Label::new((
                                conflict.file_path.to_string_lossy().to_string(),
                                conflict.span.clone(),
                            ))
                            .with_message(label_msg)
                            .with_color(if idx == 0 { Color::Red } else { Color::Yellow }),
                        );
                    }

                    // Finish the report and write it
                    let finished_report = report.finish();

                    // Write to each unique source file
                    let mut written_files = std::collections::HashSet::new();
                    for conflict in &merge_error.conflicts {
                        let file_id = conflict.file_path.to_string_lossy().to_string();
                        if written_files.insert(file_id.clone()) {
                            let source = Source::from(&conflict.content);
                            finished_report
                                .write((file_id, source), &mut output)
                                .ok();
                        }
                    }
                }
                Error::Validation(validation_error) => {
                    // For validation errors, format them simply
                    let error_msg = format!(
                        "Validation error in '{}': {}\n",
                        validation_error.field_path, validation_error.message
                    );
                    output.extend_from_slice(error_msg.as_bytes());
                }
            },
        }
    }

    String::from_utf8_lossy(&output).to_string()
}
