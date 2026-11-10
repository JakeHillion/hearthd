use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

use ariadne::Color;
use ariadne::Config;
use ariadne::IndexType;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use ariadne::Source;

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
    EmptyConfig { file_path: PathBuf },
}

/// Error messages that indicate problems with the config
#[derive(Debug, Clone)]
pub enum Error {
    Merge(MergeError),
    Validation(ValidationError),
    Load(LoadError),
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
    pub span: Option<Range<usize>>,
    pub source: Option<SourceInfo>,
}

/// Error type for config loading failures (parse errors, IO errors, etc.)
#[derive(Debug, Clone)]
pub enum LoadError {
    Io {
        path: PathBuf,
        error: String, // Changed from std::io::Error since it's not Clone
    },
    Parse {
        source: SourceInfo,
        span: Range<usize>,
        message: String,
    },
    ImportCycle {
        path: PathBuf,
        cycle: Vec<PathBuf>,
    },
}

impl LoadError {
    fn render(&self, color: bool) -> String {
        match self {
            LoadError::Io { path, error } => headline(
                ReportKind::Error,
                "Failed to read config file",
                path,
                error,
                color,
            ),
            LoadError::Parse {
                source,
                span,
                message,
            } => {
                let id = file_id(&source.file_path);
                let mut output = Vec::new();
                Report::build(ReportKind::Error, (id.clone(), span.clone()))
                    .with_config(report_config(color))
                    .with_message("Failed to parse config file")
                    .with_label(
                        Label::new((id.clone(), span.clone()))
                            .with_message(message)
                            .with_color(Color::Red),
                    )
                    .finish()
                    .write((id, Source::from(source.content.as_str())), &mut output)
                    .ok();
                String::from_utf8_lossy(&output).to_string()
            }
            LoadError::ImportCycle { path, cycle } => headline(
                ReportKind::Error,
                "Import cycle detected",
                path,
                &format!("Import cycle involves {} file(s)", cycle.len()),
                color,
            ),
        }
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render(true))
    }
}

impl std::error::Error for LoadError {}

/// A collection of diagnostics (warnings and/or errors)
#[derive(Debug, Clone)]
pub struct Diagnostics(pub Vec<Diagnostic>);

impl std::fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_diagnostics(&self.0))
    }
}

impl std::error::Error for Diagnostics {}

impl From<LoadError> for Diagnostics {
    fn from(error: LoadError) -> Self {
        Self(vec![Diagnostic::Error(Error::Load(error))])
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_diagnostics(std::slice::from_ref(self)))
    }
}

impl Diagnostic {
    /// Returns true if this diagnostic is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Diagnostic::Error(_))
    }

    /// Returns true if this diagnostic is a warning
    pub fn is_warning(&self) -> bool {
        matches!(self, Diagnostic::Warning(_))
    }

    /// Prepend a path prefix to the field_path in this diagnostic
    ///
    /// This is used when bubbling diagnostics up through nested configs.
    /// For example, if a nested config reports an error on field "latitude",
    /// the parent can prepend "locations.home" to get "locations.home.latitude".
    pub fn prepend_path(self, prefix: &str) -> Self {
        match self {
            Diagnostic::Error(error) => Diagnostic::Error(match error {
                Error::Merge(mut merge_error) => {
                    merge_error.field_path = if merge_error.field_path.is_empty() {
                        prefix.to_string()
                    } else {
                        format!("{}.{}", prefix, merge_error.field_path)
                    };
                    Error::Merge(merge_error)
                }
                Error::Validation(mut validation_error) => {
                    validation_error.field_path = if validation_error.field_path.is_empty() {
                        prefix.to_string()
                    } else {
                        format!("{}.{}", prefix, validation_error.field_path)
                    };
                    Error::Validation(validation_error)
                }
                Error::Load(load_error) => Error::Load(load_error), // Load errors don't have field paths
            }),
            Diagnostic::Warning(warning) => Diagnostic::Warning(warning), // Warnings don't have field paths
        }
    }
}

/// Format all diagnostics for display on a terminal, in colour.
pub fn format_diagnostics(diagnostics: &[Diagnostic]) -> String {
    render(diagnostics, true)
}

/// Format all diagnostics without colour escapes.
pub fn format_diagnostics_plain(diagnostics: &[Diagnostic]) -> String {
    render(diagnostics, false)
}

/// A report about a file as a whole, with no excerpt from it.
fn headline(kind: ReportKind, title: &str, path: &Path, detail: &str, color: bool) -> String {
    let (name, code) = match kind {
        ReportKind::Warning => ("Warning", "33"),
        _ => ("Error", "31"),
    };
    let name = if color {
        format!("\x1b[{code}m{name}\x1b[0m")
    } else {
        name.to_string()
    };
    format!(
        "{name}: {title}\n  ┌─ {}:1:1\n  │\n  = {detail}\n",
        path.display()
    )
}

fn file_id(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn report_config(color: bool) -> Config {
    // toml reports byte offsets, which is not what ariadne assumes.
    Config::default()
        .with_color(color)
        .with_index_type(IndexType::Byte)
}

fn render(diagnostics: &[Diagnostic], color: bool) -> String {
    let config = report_config(color);
    let mut output = Vec::new();

    for diagnostic in diagnostics {
        match diagnostic {
            Diagnostic::Warning(Warning::EmptyConfig { file_path }) => {
                output.extend_from_slice(
                    headline(
                        ReportKind::Warning,
                        "Empty configuration file",
                        file_path,
                        &format!(
                            "Config file '{}' is empty and has no effect",
                            file_path.display()
                        ),
                        color,
                    )
                    .as_bytes(),
                );
                output.push(b'\n');
            }
            Diagnostic::Error(Error::Merge(merge_error)) => {
                let first = &merge_error.conflicts[0];
                let mut report = Report::build(
                    ReportKind::Error,
                    (file_id(&first.file_path), first.span.clone()),
                )
                .with_config(config)
                .with_message(format!(
                    "Merge conflict in field '{}'",
                    merge_error.field_path
                ))
                .with_note(&merge_error.message);

                for (idx, conflict) in merge_error.conflicts.iter().enumerate() {
                    let (message, label_color) = if idx == 0 {
                        ("first definition here", Color::Red)
                    } else {
                        ("conflicts with this definition", Color::Yellow)
                    };
                    report = report.with_label(
                        Label::new((file_id(&conflict.file_path), conflict.span.clone()))
                            .with_message(message)
                            .with_color(label_color),
                    );
                }

                // One report over every file involved, so each definition is
                // shown in place and none is printed twice.
                let files: HashMap<String, &str> = merge_error
                    .conflicts
                    .iter()
                    .map(|conflict| (file_id(&conflict.file_path), conflict.content.as_str()))
                    .collect();
                report
                    .finish()
                    .write(ariadne::sources(files), &mut output)
                    .ok();
            }
            Diagnostic::Error(Error::Validation(validation_error)) => {
                if let (Some(span), Some(source_info)) =
                    (&validation_error.span, &validation_error.source)
                {
                    let id = file_id(&source_info.file_path);
                    Report::build(ReportKind::Error, (id.clone(), span.clone()))
                        .with_config(config)
                        .with_message(format!(
                            "Validation error in '{}'",
                            validation_error.field_path
                        ))
                        .with_label(
                            Label::new((id.clone(), span.clone()))
                                .with_message(&validation_error.message)
                                .with_color(Color::Red),
                        )
                        .finish()
                        .write(
                            (id, Source::from(source_info.content.as_str())),
                            &mut output,
                        )
                        .ok();
                } else {
                    let file_path = validation_error
                        .source
                        .as_ref()
                        .map(|s| s.file_path.as_path())
                        .unwrap_or(Path::new("<unknown>"));
                    output.extend_from_slice(
                        headline(
                            ReportKind::Error,
                            &format!("Validation error in '{}'", validation_error.field_path),
                            file_path,
                            &validation_error.message,
                            color,
                        )
                        .as_bytes(),
                    );
                    output.push(b'\n');
                }
            }
            Diagnostic::Error(Error::Load(load_error)) => {
                output.extend_from_slice(load_error.render(color).as_bytes());
            }
        }
    }

    String::from_utf8_lossy(&output).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_is_error() {
        let error = Diagnostic::Error(Error::Validation(ValidationError {
            field_path: "test".to_string(),
            message: "test error".to_string(),
            span: None,
            source: None,
        }));
        assert!(error.is_error());
        assert!(!error.is_warning());
    }

    #[test]
    fn test_diagnostic_is_warning() {
        let warning = Diagnostic::Warning(Warning::EmptyConfig {
            file_path: PathBuf::from("test.toml"),
        });
        assert!(warning.is_warning());
        assert!(!warning.is_error());
    }

    #[test]
    fn test_format_empty_config_warning() {
        let diagnostics = vec![Diagnostic::Warning(Warning::EmptyConfig {
            file_path: PathBuf::from("/tmp/empty.toml"),
        })];

        insta::assert_snapshot!(format_diagnostics_plain(&diagnostics), @"
        Warning: Empty configuration file
          ┌─ /tmp/empty.toml:1:1
          │
          = Config file '/tmp/empty.toml' is empty and has no effect
        ");
    }

    fn conflict_across_files() -> Vec<Diagnostic> {
        vec![Diagnostic::Error(Error::Merge(MergeError {
            field_path: "logging.level".to_string(),
            message: "Logging level defined in multiple config files".to_string(),
            conflicts: vec![
                MergeConflictLocation {
                    file_path: PathBuf::from("/tmp/base.toml"),
                    span: 18..24,
                    content: "[logging]\nlevel = \"info\"\n".to_string(),
                },
                MergeConflictLocation {
                    file_path: PathBuf::from("/tmp/override.toml"),
                    span: 18..25,
                    content: "[logging]\nlevel = \"debug\"\n".to_string(),
                },
            ],
        }))]
    }

    #[test]
    fn a_conflict_across_files_is_one_report() {
        insta::assert_snapshot!(format_diagnostics_plain(&conflict_across_files()), @r#"
        Error: Merge conflict in field 'logging.level'
           ╭─[ /tmp/base.toml:2:9 ]
           │
         2 │ level = "info"
           │         ───┬──  
           │            ╰──── first definition here
           │
           ├─[ /tmp/override.toml:2:9 ]
           │
         2 │ level = "debug"
           │         ───┬───  
           │            ╰───── conflicts with this definition
           │ 
           │ Note: Logging level defined in multiple config files
        ───╯
        "#);
    }

    #[test]
    fn colour_is_only_added_on_request() {
        assert!(format_diagnostics(&conflict_across_files()).contains("\x1b["));
        assert!(!format_diagnostics_plain(&conflict_across_files()).contains("\x1b["));
    }

    #[test]
    fn a_conflict_within_one_file_shows_both_definitions() {
        let content = "[logging]\nlevel = \"info\"\n\n[logging.overrides]\nlevel = \"debug\"\n";
        let diagnostics = vec![Diagnostic::Error(Error::Merge(MergeError {
            field_path: "logging.overrides.level".to_string(),
            message: "Field 'logging.overrides.level' defined in multiple config files".to_string(),
            conflicts: vec![
                MergeConflictLocation {
                    file_path: PathBuf::from("/tmp/config.toml"),
                    span: 18..24,
                    content: content.to_string(),
                },
                MergeConflictLocation {
                    file_path: PathBuf::from("/tmp/config.toml"),
                    span: 54..61,
                    content: content.to_string(),
                },
            ],
        }))];

        insta::assert_snapshot!(format_diagnostics_plain(&diagnostics), @r#"
        Error: Merge conflict in field 'logging.overrides.level'
           ╭─[ /tmp/config.toml:2:9 ]
           │
         2 │ level = "info"
           │         ───┬──  
           │            ╰──── first definition here
           │ 
         5 │ level = "debug"
           │         ───┬───  
           │            ╰───── conflicts with this definition
           │ 
           │ Note: Field 'logging.overrides.level' defined in multiple config files
        ───╯
        "#);
    }

    #[test]
    fn spans_are_byte_offsets() {
        let content = "# søk\nname = \"x\"\n";
        let diagnostics = vec![Diagnostic::Error(Error::Validation(ValidationError {
            field_path: "name".to_string(),
            message: "too short".to_string(),
            span: Some(14..17),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config.toml"),
                content: content.to_string(),
            }),
        }))];

        insta::assert_snapshot!(format_diagnostics_plain(&diagnostics), @r#"
        Error: Validation error in 'name'
           ╭─[ /tmp/config.toml:2:8 ]
           │
         2 │ name = "x"
           │        ─┬─  
           │         ╰─── too short
        ───╯
        "#);
    }

    #[test]
    fn test_format_validation_error_with_span() {
        let content = r#"[locations.home]
latitude = 59.9139
"#;
        let diagnostics = vec![Diagnostic::Error(Error::Validation(ValidationError {
            field_path: "locations.home.longitude".to_string(),
            message: "longitude is required".to_string(),
            span: Some(0..16),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config.toml"),
                content: content.to_string(),
            }),
        }))];

        insta::assert_snapshot!(format_diagnostics_plain(&diagnostics), @"
        Error: Validation error in 'locations.home.longitude'
           ╭─[ /tmp/config.toml:1:1 ]
           │
         1 │ [locations.home]
           │ ────────┬───────  
           │         ╰───────── longitude is required
        ───╯
        ");
    }

    #[test]
    fn test_format_validation_error_without_span() {
        let diagnostics = vec![Diagnostic::Error(Error::Validation(ValidationError {
            field_path: "locations.default".to_string(),
            message: "default location 'nonexistent' not found in locations".to_string(),
            span: None,
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config.toml"),
                content: String::new(),
            }),
        }))];

        insta::assert_snapshot!(format_diagnostics_plain(&diagnostics), @"
        Error: Validation error in 'locations.default'
          ┌─ /tmp/config.toml:1:1
          │
          = default location 'nonexistent' not found in locations
        ");
    }

    #[test]
    fn test_format_multiple_diagnostics() {
        let diagnostics = vec![
            Diagnostic::Warning(Warning::EmptyConfig {
                file_path: PathBuf::from("/tmp/empty.toml"),
            }),
            Diagnostic::Error(Error::Validation(ValidationError {
                field_path: "test.field".to_string(),
                message: "test error".to_string(),
                span: None,
                source: None,
            })),
        ];

        insta::assert_snapshot!(format_diagnostics_plain(&diagnostics), @"
        Warning: Empty configuration file
          ┌─ /tmp/empty.toml:1:1
          │
          = Config file '/tmp/empty.toml' is empty and has no effect

        Error: Validation error in 'test.field'
          ┌─ <unknown>:1:1
          │
          = test error
        ");
    }

    #[test]
    fn test_load_error_display_io() {
        let error = LoadError::Io {
            path: PathBuf::from("/tmp/config.toml"),
            error: "file not found".to_string(),
        };
        let display = format!("{}", error);
        assert!(display.contains("Failed to read"));
        assert!(display.contains("/tmp/config.toml"));
        assert!(display.contains("file not found"));
    }

    #[test]
    fn a_parse_error_shows_where_it_is() {
        let diagnostics = vec![Diagnostic::Error(Error::Load(LoadError::Parse {
            source: SourceInfo {
                file_path: PathBuf::from("/tmp/config.toml"),
                content: "[http]\nport = \n".to_string(),
            },
            span: 14..14,
            message: "string values must be quoted, expected literal string".to_string(),
        }))];

        insta::assert_snapshot!(format_diagnostics_plain(&diagnostics), @"
        Error: Failed to parse config file
           ╭─[ /tmp/config.toml:2:8 ]
           │
         2 │ port =
           │        │ 
           │        ╰─ string values must be quoted, expected literal string
        ───╯
        ");
    }

    #[test]
    fn test_load_error_display_import_cycle() {
        let error = LoadError::ImportCycle {
            path: PathBuf::from("/tmp/a.toml"),
            cycle: vec![PathBuf::from("/tmp/a.toml"), PathBuf::from("/tmp/b.toml")],
        };
        let display = format!("{}", error);
        assert!(display.contains("Import cycle detected"));
        assert!(display.contains("/tmp/a.toml"));
        assert!(display.contains("2 file(s)"));
    }
}
