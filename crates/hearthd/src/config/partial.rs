use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::diagnostics::{
    Diagnostic, Error, LoadError, MergeConflictLocation, MergeError, SourceInfo, Warning,
};
use super::LogLevel;

#[derive(Debug, Default, Deserialize)]
pub struct PartialConfig {
    #[serde(default)]
    pub imports: Vec<String>,

    pub logging: Option<PartialLoggingConfig>,
    pub locations: Option<PartialLocationsConfig>,
    pub integrations: Option<PartialIntegrationsConfig>,

    /// Source information for error reporting (not serialized)
    #[serde(skip)]
    pub source: Option<SourceInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartialLoggingConfig {
    pub level: Option<toml::Spanned<LogLevel>>,
    pub overrides: Option<HashMap<String, toml::Spanned<LogLevel>>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartialLocationsConfig {
    pub default: Option<toml::Spanned<String>>,
    #[serde(flatten)]
    pub locations: HashMap<String, PartialLocation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartialLocation {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub elevation_m: Option<f64>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartialIntegrationsConfig {
    // Empty for now - integrations will be added as static fields later
}


impl PartialConfig {
    /// Load a single config file without processing imports
    pub fn from_file(path: &Path) -> Result<Self, LoadError> {
        let content = std::fs::read_to_string(path).map_err(|e| LoadError::Io {
            path: path.to_path_buf(),
            error: e,
        })?;

        let mut config: PartialConfig = toml::from_str(&content).map_err(|e| LoadError::Parse {
            path: path.to_path_buf(),
            error: e,
        })?;

        config.source = Some(SourceInfo {
            file_path: path.to_path_buf(),
            content,
        });

        Ok(config)
    }

    /// Load config files with import resolution
    ///
    /// Each config file is loaded, then its imports are recursively processed.
    /// Cycle detection prevents infinite loops.
    ///
    /// Returns a Vec of all loaded configs in order (imports first, then parent)
    pub fn load_with_imports(paths: &[PathBuf]) -> Result<Vec<Self>, LoadError> {
        let mut visited = HashSet::new();
        let mut all_configs = Vec::new();

        for path in paths {
            Self::load_recursive(path, &mut visited, &mut all_configs)?;
        }

        Ok(all_configs)
    }

    /// Recursively load a config file and its imports
    fn load_recursive(
        path: &Path,
        visited: &mut HashSet<PathBuf>,
        configs: &mut Vec<Self>,
    ) -> Result<(), LoadError> {
        // Canonicalize the path to detect cycles reliably
        let canonical_path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf());

        // Check for import cycles
        if visited.contains(&canonical_path) {
            return Err(LoadError::ImportCycle {
                path: canonical_path.clone(),
                cycle: visited.iter().cloned().collect(),
            });
        }

        visited.insert(canonical_path.clone());

        // Load the config file
        let config = Self::from_file(path)?;

        // Process imports first (depth-first)
        for import_path in &config.imports {
            let import_path_buf = PathBuf::from(import_path);

            // Resolve relative imports from the parent file's directory
            let resolved_path = if import_path_buf.is_absolute() {
                import_path_buf
            } else {
                let parent_dir = path.parent().unwrap_or_else(|| Path::new("."));
                parent_dir.join(import_path_buf)
            };

            Self::load_recursive(&resolved_path, visited, configs)?;
        }

        // Add this config after its imports
        configs.push(config);

        // Remove from visited set to allow imports from sibling branches
        visited.remove(&canonical_path);

        Ok(())
    }

    /// Merge multiple partial configs together
    ///
    /// Uses first-wins semantics: the first occurrence of a field is kept.
    /// Conflicts (same field defined in multiple configs) are collected as errors
    /// but merging continues to find all conflicts at once (compiler-style error collection).
    ///
    /// Returns (merged, diagnostics) where diagnostics may contain warnings and errors
    pub fn merge<I>(configs: I) -> (Self, Vec<Diagnostic>)
    where
        I: IntoIterator<Item = Self>,
    {
        let mut result = PartialConfig::default();
        let mut diagnostics = Vec::new();
        let mut imports = Vec::new();

        // Track which file set each field with span information (for first-wins)
        let mut logging_level_loc: Option<MergeConflictLocation> = None;
        let mut logging_overrides_locs: HashMap<String, MergeConflictLocation> = HashMap::new();
        let mut locations_default_loc: Option<MergeConflictLocation> = None;
        let mut location_locs: HashMap<String, MergeConflictLocation> = HashMap::new();

        for config in configs {
            // Collect all imports
            imports.extend(config.imports.clone());

            let source_info = config.source.as_ref().cloned().unwrap_or_else(|| SourceInfo {
                file_path: PathBuf::from("<unknown>"),
                content: String::new(),
            });

            // Check if config is empty (no meaningful content)
            let is_empty = config.logging.is_none()
                && config.locations.is_none()
                && config.integrations.is_none()
                && config.imports.is_empty();

            if is_empty {
                diagnostics.push(Diagnostic::Warning(Warning::EmptyConfig {
                    file_path: source_info.file_path.clone(),
                }));
            }

            // Merge logging config
            if let Some(logging) = config.logging {
                if result.logging.is_none() {
                    result.logging = Some(PartialLoggingConfig {
                        level: None,
                        overrides: None,
                    });
                }

                let result_logging = result.logging.as_mut().unwrap();

                // Check logging level conflict (first-wins)
                if let Some(level_spanned) = logging.level {
                    let conflict_loc = MergeConflictLocation {
                        file_path: source_info.file_path.clone(),
                        span: level_spanned.span(),
                        content: source_info.content.clone(),
                    };

                    if let Some(prev_loc) = logging_level_loc.as_ref() {
                        // Conflict: keep first value, record error
                        diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                            field_path: "logging.level".to_string(),
                            message: "Logging level defined in multiple config files".to_string(),
                            conflicts: vec![prev_loc.clone(), conflict_loc],
                        })));
                    } else {
                        // First occurrence: keep it
                        result_logging.level = Some(level_spanned);
                        logging_level_loc = Some(conflict_loc);
                    }
                }

                // Check logging overrides conflicts (first-wins per key)
                if let Some(overrides) = logging.overrides {
                    if result_logging.overrides.is_none() {
                        result_logging.overrides = Some(HashMap::new());
                    }

                    let result_overrides = result_logging.overrides.as_mut().unwrap();
                    for (key, value_spanned) in overrides {
                        let conflict_loc = MergeConflictLocation {
                            file_path: source_info.file_path.clone(),
                            span: value_spanned.span(),
                            content: source_info.content.clone(),
                        };

                        if let Some(prev_loc) = logging_overrides_locs.get(&key) {
                            // Conflict: keep first value, record error
                            diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                                field_path: format!("logging.overrides.{}", key),
                                message: format!(
                                    "Logging override for '{}' defined in multiple config files",
                                    key
                                ),
                                conflicts: vec![prev_loc.clone(), conflict_loc],
                            })));
                        } else {
                            // First occurrence: keep it
                            result_overrides.insert(key.clone(), value_spanned);
                            logging_overrides_locs.insert(key, conflict_loc);
                        }
                    }
                }
            }

            // Merge locations config
            if let Some(locations) = config.locations {
                if result.locations.is_none() {
                    result.locations = Some(PartialLocationsConfig {
                        default: None,
                        locations: HashMap::new(),
                    });
                }

                let result_locations = result.locations.as_mut().unwrap();

                // Check default location conflict (first-wins)
                if let Some(default_spanned) = locations.default {
                    let conflict_loc = MergeConflictLocation {
                        file_path: source_info.file_path.clone(),
                        span: default_spanned.span(),
                        content: source_info.content.clone(),
                    };

                    if let Some(prev_loc) = locations_default_loc.as_ref() {
                        // Conflict: keep first value, record error
                        diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                            field_path: "locations.default".to_string(),
                            message: "Default location defined in multiple config files".to_string(),
                            conflicts: vec![prev_loc.clone(), conflict_loc],
                        })));
                    } else {
                        // First occurrence: keep it
                        result_locations.default = Some(default_spanned);
                        locations_default_loc = Some(conflict_loc);
                    }
                }

                // Check location definitions conflicts (first-wins per location)
                for (key, value) in locations.locations {
                    // Find the span of the location definition in the source
                    let location_header = format!("[locations.{}]", key);
                    let span = source_info.content.find(&location_header)
                        .map(|start| start..(start + location_header.len()))
                        .unwrap_or(0..0);

                    let conflict_loc = MergeConflictLocation {
                        file_path: source_info.file_path.clone(),
                        span,
                        content: source_info.content.clone(),
                    };

                    if let Some(prev_loc) = location_locs.get(&key) {
                        // Conflict: keep first value, record error
                        diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                            field_path: format!("locations.{}", key),
                            message: format!(
                                "Location '{}' defined in multiple config files",
                                key
                            ),
                            conflicts: vec![prev_loc.clone(), conflict_loc],
                        })));
                    } else {
                        // First occurrence: keep it
                        result_locations.locations.insert(key.clone(), value);
                        location_locs.insert(key, conflict_loc);
                    }
                }
            }

            // Merge integrations config (currently empty, but set up for future)
            if config.integrations.is_some() && result.integrations.is_none() {
                result.integrations = config.integrations;
            }
        }

        result.imports = imports;

        (result, diagnostics)
    }
}
