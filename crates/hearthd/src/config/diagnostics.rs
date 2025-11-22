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
        path: PathBuf,
        error: String, // Changed from toml::de::Error since it's not Clone
    },
    ImportCycle {
        path: PathBuf,
        cycle: Vec<PathBuf>,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io { path, error } => {
                // Format as ariadne-style error
                write!(
                    f,
                    "\x1b[31mError\x1b[0m: Failed to read config file\n  ┌─ {}:1:1\n  │\n  = {}\n",
                    path.display(),
                    error
                )
            }
            LoadError::Parse { path, error } => {
                // Format as ariadne-style error with TOML error details
                write!(
                    f,
                    "\x1b[31mError\x1b[0m: Failed to parse config file\n  ┌─ {}:1:1\n  │\n  = {}\n",
                    path.display(),
                    error
                )
            }
            LoadError::ImportCycle { path, cycle } => {
                // Format as ariadne-style error
                write!(
                    f,
                    "\x1b[31mError\x1b[0m: Import cycle detected\n  ┌─ {}:1:1\n  │\n  = Import cycle involves {} file(s)\n",
                    path.display(),
                    cycle.len()
                )
            }
        }
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
}

/// Format all diagnostics for display using Ariadne
pub fn format_diagnostics(diagnostics: &[Diagnostic]) -> String {
    use ariadne::Color;
    use ariadne::Label;
    use ariadne::Report;
    use ariadne::ReportKind;
    use ariadne::Source;

    let mut output = Vec::new();

    for diagnostic in diagnostics {
        match diagnostic {
            Diagnostic::Warning(warning) => match warning {
                Warning::EmptyConfig { file_path } => {
                    // Format manually since ariadne doesn't render notes well without source
                    use std::io::Write;
                    writeln!(
                        &mut output,
                        "\x1b[33mWarning\x1b[0m: Empty configuration file"
                    )
                    .ok();
                    writeln!(&mut output, "  ┌─ {}:1:1", file_path.display()).ok();
                    writeln!(&mut output, "  │").ok();
                    writeln!(
                        &mut output,
                        "  = Config file '{}' is empty and has no effect",
                        file_path.display()
                    )
                    .ok();
                    writeln!(&mut output).ok();
                }
            },
            Diagnostic::Error(error) => {
                match error {
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
                        .with_message(format!(
                            "Merge conflict in field '{}'",
                            merge_error.field_path
                        ))
                        .with_note(&merge_error.message);

                        // Add labels for each conflict location
                        for (idx, conflict) in merge_error.conflicts.iter().enumerate() {
                            let label_msg = if idx == 0 {
                                "first definition here"
                            } else {
                                "conflicts with this definition"
                            };

                            report =
                                report.with_label(
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
                        // Note: Ariadne will emit "Unable to fetch source" warnings for labels
                        // that reference files not in the current cache, but this is expected
                        // behavior and the output is still correct.
                        let mut written_files = std::collections::HashSet::new();
                        for conflict in &merge_error.conflicts {
                            let file_id = conflict.file_path.to_string_lossy().to_string();
                            if written_files.insert(file_id.clone()) {
                                let source = Source::from(conflict.content.clone());
                                finished_report.write((file_id, source), &mut output).ok();
                            }
                        }
                    }
                    Error::Validation(validation_error) => {
                        // Use ariadne for validation errors with span information when available
                        if let (Some(span), Some(source_info)) =
                            (&validation_error.span, &validation_error.source)
                        {
                            let file_id = source_info.file_path.to_string_lossy().to_string();
                            let report =
                                Report::build(ReportKind::Error, (file_id.clone(), span.clone()))
                                    .with_message(format!(
                                        "Validation error in '{}'",
                                        validation_error.field_path
                                    ))
                                    .with_label(
                                        Label::new((file_id.clone(), span.clone()))
                                            .with_message(&validation_error.message)
                                            .with_color(Color::Red),
                                    )
                                    .finish();

                            let source = Source::from(source_info.content.clone());
                            report.write((file_id, source), &mut output).ok();
                        } else {
                            // Fallback for validation errors without span information
                            // Format manually since ariadne doesn't render notes well without source
                            use std::io::Write;
                            let file_path = validation_error
                                .source
                                .as_ref()
                                .map(|s| s.file_path.display().to_string())
                                .unwrap_or_else(|| "<unknown>".to_string());

                            writeln!(
                                &mut output,
                                "\x1b[31mError\x1b[0m: Validation error in '{}'",
                                validation_error.field_path
                            )
                            .ok();
                            writeln!(&mut output, "  ┌─ {}:1:1", file_path).ok();
                            writeln!(&mut output, "  │").ok();
                            writeln!(&mut output, "  = {}", validation_error.message).ok();
                            writeln!(&mut output).ok();
                        }
                    }
                    Error::Load(load_error) => {
                        // Use LoadError's Display implementation
                        use std::io::Write;
                        write!(&mut output, "{}", load_error).ok();
                    }
                }
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

        let output = format_diagnostics(&diagnostics);
        let expected = "\u{1b}[33mWarning\u{1b}[0m: Empty configuration file
  ┌─ /tmp/empty.toml:1:1
  │
  = Config file '/tmp/empty.toml' is empty and has no effect

";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_format_merge_error() {
        let content = r#"[logging]
level = "info"
"#;
        let conflicts = vec![
            MergeConflictLocation {
                file_path: PathBuf::from("/tmp/base.toml"),
                span: 10..24,
                content: content.to_string(),
            },
            MergeConflictLocation {
                file_path: PathBuf::from("/tmp/override.toml"),
                span: 10..25,
                content: r#"[logging]
level = "debug"
"#
                .to_string(),
            },
        ];

        let diagnostics = vec![Diagnostic::Error(Error::Merge(MergeError {
            field_path: "logging.level".to_string(),
            message: "Logging level defined in multiple config files".to_string(),
            conflicts,
        }))];

        let output = format_diagnostics(&diagnostics);
        let expected = "\u{1b}[31mError:\u{1b}[0m Merge conflict in field 'logging.level'\n   \u{1b}[38;5;246m╭\u{1b}[0m\u{1b}[38;5;246m─\u{1b}[0m\u{1b}[38;5;246m[\u{1b}[0m /tmp/base.toml:2:1 \u{1b}[38;5;246m]\u{1b}[0m\n   \u{1b}[38;5;246m│\u{1b}[0m\n \u{1b}[38;5;246m2 │\u{1b}[0m \u{1b}[31ml\u{1b}[0m\u{1b}[31me\u{1b}[0m\u{1b}[31mv\u{1b}[0m\u{1b}[31me\u{1b}[0m\u{1b}[31ml\u{1b}[0m\u{1b}[31m \u{1b}[0m\u{1b}[31m=\u{1b}[0m\u{1b}[31m \u{1b}[0m\u{1b}[31m\"\u{1b}[0m\u{1b}[31mi\u{1b}[0m\u{1b}[31mn\u{1b}[0m\u{1b}[31mf\u{1b}[0m\u{1b}[31mo\u{1b}[0m\u{1b}[31m\"\u{1b}[0m\n \u{1b}[38;5;240m  │\u{1b}[0m \u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m┬\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m  \n \u{1b}[38;5;240m  │\u{1b}[0m        \u{1b}[31m╰\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m first definition here\n \u{1b}[38;5;240m  │\u{1b}[0m \n \u{1b}[38;5;240m  │\u{1b}[0m \u{1b}[38;5;115mNote\u{1b}[0m: Logging level defined in multiple config files\n\u{1b}[38;5;246m───╯\u{1b}[0m\n\u{1b}[31mError:\u{1b}[0m Merge conflict in field 'logging.level'\n   \u{1b}[38;5;246m╭\u{1b}[0m\u{1b}[38;5;246m─\u{1b}[0m\u{1b}[38;5;246m[\u{1b}[0m /tmp/override.toml:2:1 \u{1b}[38;5;246m]\u{1b}[0m\n   \u{1b}[38;5;246m│\u{1b}[0m\n \u{1b}[38;5;246m2 │\u{1b}[0m \u{1b}[33ml\u{1b}[0m\u{1b}[33me\u{1b}[0m\u{1b}[33mv\u{1b}[0m\u{1b}[33me\u{1b}[0m\u{1b}[33ml\u{1b}[0m\u{1b}[33m \u{1b}[0m\u{1b}[33m=\u{1b}[0m\u{1b}[33m \u{1b}[0m\u{1b}[33m\"\u{1b}[0m\u{1b}[33md\u{1b}[0m\u{1b}[33me\u{1b}[0m\u{1b}[33mb\u{1b}[0m\u{1b}[33mu\u{1b}[0m\u{1b}[33mg\u{1b}[0m\u{1b}[33m\"\u{1b}[0m\n \u{1b}[38;5;240m  │\u{1b}[0m \u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m┬\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m  \n \u{1b}[38;5;240m  │\u{1b}[0m        \u{1b}[33m╰\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m\u{1b}[33m─\u{1b}[0m conflicts with this definition\n \u{1b}[38;5;240m  │\u{1b}[0m \n \u{1b}[38;5;240m  │\u{1b}[0m \u{1b}[38;5;115mNote\u{1b}[0m: Logging level defined in multiple config files\n\u{1b}[38;5;246m───╯\u{1b}[0m\n";
        assert_eq!(output, expected);
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

        let output = format_diagnostics(&diagnostics);
        let expected = "\u{1b}[31mError:\u{1b}[0m Validation error in 'locations.home.longitude'\n   \u{1b}[38;5;246m╭\u{1b}[0m\u{1b}[38;5;246m─\u{1b}[0m\u{1b}[38;5;246m[\u{1b}[0m /tmp/config.toml:1:1 \u{1b}[38;5;246m]\u{1b}[0m\n   \u{1b}[38;5;246m│\u{1b}[0m\n \u{1b}[38;5;246m1 │\u{1b}[0m \u{1b}[31m[\u{1b}[0m\u{1b}[31ml\u{1b}[0m\u{1b}[31mo\u{1b}[0m\u{1b}[31mc\u{1b}[0m\u{1b}[31ma\u{1b}[0m\u{1b}[31mt\u{1b}[0m\u{1b}[31mi\u{1b}[0m\u{1b}[31mo\u{1b}[0m\u{1b}[31mn\u{1b}[0m\u{1b}[31ms\u{1b}[0m\u{1b}[31m.\u{1b}[0m\u{1b}[31mh\u{1b}[0m\u{1b}[31mo\u{1b}[0m\u{1b}[31mm\u{1b}[0m\u{1b}[31me\u{1b}[0m\u{1b}[31m]\u{1b}[0m\n \u{1b}[38;5;240m  │\u{1b}[0m \u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m┬\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m  \n \u{1b}[38;5;240m  │\u{1b}[0m         \u{1b}[31m╰\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m longitude is required\n\u{1b}[38;5;246m───╯\u{1b}[0m\n";
        assert_eq!(output, expected);
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

        let output = format_diagnostics(&diagnostics);
        let expected = "\u{1b}[31mError\u{1b}[0m: Validation error in 'locations.default'\n  ┌─ /tmp/config.toml:1:1\n  │\n  = default location 'nonexistent' not found in locations\n\n";
        assert_eq!(output, expected);
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

        let output = format_diagnostics(&diagnostics);
        let expected = "\u{1b}[33mWarning\u{1b}[0m: Empty configuration file\n  ┌─ /tmp/empty.toml:1:1\n  │\n  = Config file '/tmp/empty.toml' is empty and has no effect\n\n\u{1b}[31mError\u{1b}[0m: Validation error in 'test.field'\n  ┌─ <unknown>:1:1\n  │\n  = test error\n\n";
        assert_eq!(output, expected);
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
    fn test_load_error_display_parse() {
        let error = LoadError::Parse {
            path: PathBuf::from("/tmp/config.toml"),
            error: "invalid TOML syntax".to_string(),
        };
        let display = format!("{}", error);
        assert!(display.contains("Failed to parse"));
        assert!(display.contains("/tmp/config.toml"));
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
