use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::diagnostics::{
    Diagnostic, Error, LoadError, MergeConflictLocation, MergeError, SourceInfo, ValidationError,
    Warning,
};
use super::{HttpConfig, Location, LocationsConfig, LogLevel, LoggingConfig};

#[derive(Debug, Default, Deserialize)]
pub struct PartialConfig {
    #[serde(default)]
    pub imports: Vec<String>,

    pub logging: Option<PartialLoggingConfig>,
    pub locations: Option<PartialLocationsConfig>,
    pub http: Option<PartialHttpConfig>,
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
pub struct PartialHttpConfig {
    pub listen: Option<toml::Spanned<String>>,
    pub port: Option<toml::Spanned<u16>>,
}

impl From<PartialHttpConfig> for HttpConfig {
    fn from(partial: PartialHttpConfig) -> Self {
        Self {
            listen: partial
                .listen
                .map(|s| s.into_inner())
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            port: partial.port.map(|p| p.into_inner()).unwrap_or(8565),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartialIntegrationsConfig {
    // Empty for now - integrations will be added as static fields later
}

/// Context for converting a partial location to a full location
pub struct LocationConversionContext {
    pub name: String,
    pub source: Option<SourceInfo>,
}

impl TryFrom<(PartialLocation, LocationConversionContext)> for Location {
    type Error = Vec<Diagnostic>;

    fn try_from(
        (partial, ctx): (PartialLocation, LocationConversionContext),
    ) -> Result<Self, Self::Error> {
        let mut diagnostics = Vec::new();

        // Latitude is required
        let latitude = if let Some(lat) = partial.latitude {
            lat
        } else {
            diagnostics.push(Diagnostic::Error(Error::Validation(ValidationError {
                field_path: format!("locations.{}.latitude", ctx.name),
                message: "latitude is required".to_string(),
                span: None,
                source: ctx.source.clone(),
            })));
            0.0 // Default for error recovery
        };

        // Longitude is required
        let longitude = if let Some(lon) = partial.longitude {
            lon
        } else {
            diagnostics.push(Diagnostic::Error(Error::Validation(ValidationError {
                field_path: format!("locations.{}.longitude", ctx.name),
                message: "longitude is required".to_string(),
                span: None,
                source: ctx.source.clone(),
            })));
            0.0 // Default for error recovery
        };

        let elevation_m = partial.elevation_m;
        let timezone = partial.timezone;

        if diagnostics.is_empty() {
            Ok(Location {
                latitude,
                longitude,
                elevation_m,
                timezone,
            })
        } else {
            Err(diagnostics)
        }
    }
}

impl TryFrom<PartialLoggingConfig> for LoggingConfig {
    type Error = Vec<Diagnostic>;

    fn try_from(partial: PartialLoggingConfig) -> Result<Self, Self::Error> {
        Ok(LoggingConfig {
            level: partial.level.map(|s| *s.get_ref()).unwrap_or_default(),
            overrides: partial
                .overrides
                .map(|hm| hm.into_iter().map(|(k, v)| (k, *v.get_ref())).collect())
                .unwrap_or_default(),
        })
    }
}

impl TryFrom<(PartialLocationsConfig, Option<SourceInfo>)> for LocationsConfig {
    type Error = Vec<Diagnostic>;

    fn try_from(
        (partial, source): (PartialLocationsConfig, Option<SourceInfo>),
    ) -> Result<Self, Self::Error> {
        let mut diagnostics = Vec::new();
        let mut locations_map = HashMap::new();

        for (key, partial_location) in partial.locations {
            let ctx = LocationConversionContext {
                name: key.clone(),
                source: source.clone(),
            };

            match Location::try_from((partial_location, ctx)) {
                Ok(location) => {
                    locations_map.insert(key, location);
                }
                Err(errs) => {
                    diagnostics.extend(errs);
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(LocationsConfig {
                default: partial.default.map(|s| s.into_inner()),
                locations: locations_map,
            })
        } else {
            Err(diagnostics)
        }
    }
}

impl PartialConfig {
    /// Load a single config file without processing imports
    pub fn from_file(path: &Path) -> Result<Self, LoadError> {
        let content = std::fs::read_to_string(path).map_err(|e| LoadError::Io {
            path: path.to_path_buf(),
            error: e.to_string(),
        })?;

        let mut config: PartialConfig = toml::from_str(&content).map_err(|e| LoadError::Parse {
            path: path.to_path_buf(),
            error: e.to_string(),
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
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

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
        // Track field-level conflicts: location_key -> field_name -> conflict location
        let mut location_field_locs: HashMap<String, HashMap<String, MergeConflictLocation>> =
            HashMap::new();

        for config in configs {
            // Collect all imports
            imports.extend(config.imports.clone());

            let source_info = config
                .source
                .as_ref()
                .cloned()
                .unwrap_or_else(|| SourceInfo {
                    file_path: PathBuf::from("<unknown>"),
                    content: String::new(),
                });

            // Check if config is empty (no meaningful content)
            let is_empty = config.logging.is_none()
                && config.locations.is_none()
                && config.http.is_none()
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
                            message: "Default location defined in multiple config files"
                                .to_string(),
                            conflicts: vec![prev_loc.clone(), conflict_loc],
                        })));
                    } else {
                        // First occurrence: keep it
                        result_locations.default = Some(default_spanned);
                        locations_default_loc = Some(conflict_loc);
                    }
                }

                // Check location definitions conflicts (first-wins per field)
                for (key, value) in locations.locations {
                    // Ensure we have a location entry in the result
                    if !result_locations.locations.contains_key(&key) {
                        result_locations.locations.insert(
                            key.clone(),
                            PartialLocation {
                                latitude: None,
                                longitude: None,
                                elevation_m: None,
                                timezone: None,
                            },
                        );
                    }

                    // Get or create field tracking for this location
                    let field_locs = location_field_locs.entry(key.clone()).or_default();
                    let result_location = result_locations.locations.get_mut(&key).unwrap();

                    // Helper function to find field span in source
                    let find_field_span =
                        |field_name: &str, _field_value: &str| -> std::ops::Range<usize> {
                            // Look for the field assignment line, e.g., "latitude = 59.9139"
                            let search_pattern = format!("{} =", field_name);
                            if let Some(start) = source_info.content.find(&search_pattern) {
                                // Find the end of the line
                                let line_end = source_info.content[start..]
                                    .find('\n')
                                    .map(|offset| start + offset)
                                    .unwrap_or(source_info.content.len());
                                start..line_end
                            } else {
                                0..0
                            }
                        };

                    // Check and merge latitude
                    if let Some(new_lat) = value.latitude {
                        let field_name = "latitude";
                        if let Some(prev_loc) = field_locs.get(field_name) {
                            // Conflict: field already defined (first-wins)
                            let span = find_field_span(field_name, &new_lat.to_string());
                            let conflict_loc = MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span,
                                content: source_info.content.clone(),
                            };

                            diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                                field_path: format!("locations.{}.{}", key, field_name),
                                message: format!(
                                    "Field '{}' for location '{}' defined in multiple config files",
                                    field_name, key
                                ),
                                conflicts: vec![prev_loc.clone(), conflict_loc],
                            })));
                        } else {
                            // First occurrence: keep it
                            result_location.latitude = Some(new_lat);
                            let span = find_field_span(field_name, &new_lat.to_string());
                            field_locs.insert(
                                field_name.to_string(),
                                MergeConflictLocation {
                                    file_path: source_info.file_path.clone(),
                                    span,
                                    content: source_info.content.clone(),
                                },
                            );
                        }
                    }

                    // Check and merge longitude
                    if let Some(new_lon) = value.longitude {
                        let field_name = "longitude";
                        if let Some(prev_loc) = field_locs.get(field_name) {
                            // Conflict: field already defined (first-wins)
                            let span = find_field_span(field_name, &new_lon.to_string());
                            let conflict_loc = MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span,
                                content: source_info.content.clone(),
                            };

                            diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                                field_path: format!("locations.{}.{}", key, field_name),
                                message: format!(
                                    "Field '{}' for location '{}' defined in multiple config files",
                                    field_name, key
                                ),
                                conflicts: vec![prev_loc.clone(), conflict_loc],
                            })));
                        } else {
                            // First occurrence: keep it
                            result_location.longitude = Some(new_lon);
                            let span = find_field_span(field_name, &new_lon.to_string());
                            field_locs.insert(
                                field_name.to_string(),
                                MergeConflictLocation {
                                    file_path: source_info.file_path.clone(),
                                    span,
                                    content: source_info.content.clone(),
                                },
                            );
                        }
                    }

                    // Check and merge elevation_m
                    if let Some(new_elev) = value.elevation_m {
                        let field_name = "elevation_m";
                        if let Some(prev_loc) = field_locs.get(field_name) {
                            // Conflict: field already defined (first-wins)
                            let span = find_field_span(field_name, &new_elev.to_string());
                            let conflict_loc = MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span,
                                content: source_info.content.clone(),
                            };

                            diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                                field_path: format!("locations.{}.{}", key, field_name),
                                message: format!(
                                    "Field '{}' for location '{}' defined in multiple config files",
                                    field_name, key
                                ),
                                conflicts: vec![prev_loc.clone(), conflict_loc],
                            })));
                        } else {
                            // First occurrence: keep it
                            result_location.elevation_m = Some(new_elev);
                            let span = find_field_span(field_name, &new_elev.to_string());
                            field_locs.insert(
                                field_name.to_string(),
                                MergeConflictLocation {
                                    file_path: source_info.file_path.clone(),
                                    span,
                                    content: source_info.content.clone(),
                                },
                            );
                        }
                    }

                    // Check and merge timezone
                    if let Some(ref new_tz) = value.timezone {
                        let field_name = "timezone";
                        if let Some(prev_loc) = field_locs.get(field_name) {
                            // Conflict: field already defined (first-wins)
                            let span = find_field_span(field_name, new_tz);
                            let conflict_loc = MergeConflictLocation {
                                file_path: source_info.file_path.clone(),
                                span,
                                content: source_info.content.clone(),
                            };

                            diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                                field_path: format!("locations.{}.{}", key, field_name),
                                message: format!(
                                    "Field '{}' for location '{}' defined in multiple config files",
                                    field_name, key
                                ),
                                conflicts: vec![prev_loc.clone(), conflict_loc],
                            })));
                        } else {
                            // First occurrence: keep it
                            result_location.timezone = Some(new_tz.clone());
                            let span = find_field_span(field_name, new_tz);
                            field_locs.insert(
                                field_name.to_string(),
                                MergeConflictLocation {
                                    file_path: source_info.file_path.clone(),
                                    span,
                                    content: source_info.content.clone(),
                                },
                            );
                        }
                    }
                }
            }

            // Merge http config
            if let Some(http) = config.http {
                if result.http.is_none() {
                    result.http = Some(PartialHttpConfig {
                        listen: None,
                        port: None,
                    });
                }

                let result_http = result.http.as_mut().unwrap();

                // Check http.listen conflict (first-wins)
                if let Some(listen_spanned) = http.listen {
                    let conflict_loc = MergeConflictLocation {
                        file_path: source_info.file_path.clone(),
                        span: listen_spanned.span(),
                        content: source_info.content.clone(),
                    };

                    if let Some(prev_loc) =
                        result_http.listen.as_ref().map(|_| conflict_loc.clone())
                    {
                        // Note: we need to track the first location separately
                        // For now, just report conflict
                        diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                            field_path: "http.listen".to_string(),
                            message: "HTTP listen address defined in multiple config files"
                                .to_string(),
                            conflicts: vec![prev_loc, conflict_loc],
                        })));
                    } else {
                        // First occurrence: keep it
                        result_http.listen = Some(listen_spanned);
                    }
                }

                // Check http.port conflict (first-wins)
                if let Some(port_spanned) = http.port {
                    let conflict_loc = MergeConflictLocation {
                        file_path: source_info.file_path.clone(),
                        span: port_spanned.span(),
                        content: source_info.content.clone(),
                    };

                    if let Some(prev_loc) = result_http.port.as_ref().map(|_| conflict_loc.clone())
                    {
                        diagnostics.push(Diagnostic::Error(Error::Merge(MergeError {
                            field_path: "http.port".to_string(),
                            message: "HTTP port defined in multiple config files".to_string(),
                            conflicts: vec![prev_loc, conflict_loc],
                        })));
                    } else {
                        // First occurrence: keep it
                        result_http.port = Some(port_spanned);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn test_partial_config_from_file() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_partial_from_file");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("test.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[logging]
level = "debug"

[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        let partial = PartialConfig::from_file(&config_path).unwrap();
        assert!(partial.logging.is_some());
        assert!(partial.locations.is_some());
        assert!(partial.source.is_some());

        let source = partial.source.unwrap();
        assert_eq!(source.file_path, config_path);
        assert!(source.content.contains("debug"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_partial_config_from_file_not_found() {
        let result = PartialConfig::from_file(Path::new("/nonexistent/config.toml"));
        assert!(result.is_err());
        match result.unwrap_err() {
            LoadError::Io { .. } => {}
            _ => panic!("Expected Io error"),
        }
    }

    #[test]
    fn test_partial_config_from_file_parse_error() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_partial_parse_error");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("invalid.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(config_file, "invalid toml ][").unwrap();

        let result = PartialConfig::from_file(&config_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            LoadError::Parse { .. } => {}
            _ => panic!("Expected Parse error"),
        }

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_with_imports_single_file() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_load_single");
        fs::create_dir_all(&temp_dir).unwrap();

        let config_path = temp_dir.join("config.toml");
        let mut config_file = fs::File::create(&config_path).unwrap();
        write!(
            config_file,
            r#"
[logging]
level = "info"
"#
        )
        .unwrap();

        let configs = PartialConfig::load_with_imports(&[config_path]).unwrap();
        assert_eq!(configs.len(), 1);
        assert!(configs[0].logging.is_some());

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_with_imports_multiple_files() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_load_multiple");
        fs::create_dir_all(&temp_dir).unwrap();

        let base_path = temp_dir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "info"
"#
        )
        .unwrap();

        let extra_path = temp_dir.join("extra.toml");
        let mut extra_file = fs::File::create(&extra_path).unwrap();
        write!(
            extra_file,
            r#"
[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        let configs =
            PartialConfig::load_with_imports(&[base_path.clone(), extra_path.clone()]).unwrap();
        assert_eq!(configs.len(), 2);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_with_imports_nested() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_load_nested");
        fs::create_dir_all(&temp_dir).unwrap();

        let base_path = temp_dir.join("base.toml");
        let mut base_file = fs::File::create(&base_path).unwrap();
        write!(
            base_file,
            r#"
[logging]
level = "trace"
"#
        )
        .unwrap();

        let main_path = temp_dir.join("main.toml");
        let mut main_file = fs::File::create(&main_path).unwrap();
        write!(
            main_file,
            r#"
imports = ["base.toml"]

[locations.home]
latitude = 59.9139
longitude = 10.7522
"#
        )
        .unwrap();

        let configs = PartialConfig::load_with_imports(std::slice::from_ref(&main_path)).unwrap();
        // Should have 2 configs: base (loaded first) and main
        assert_eq!(configs.len(), 2);
        assert!(configs[0].logging.is_some()); // base
        assert!(configs[1].locations.is_some()); // main

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_with_imports_cycle_detection() {
        let temp_dir = std::env::temp_dir().join("hearthd_test_load_cycle");
        fs::create_dir_all(&temp_dir).unwrap();

        let a_path = temp_dir.join("a.toml");
        let mut a_file = fs::File::create(&a_path).unwrap();
        write!(
            a_file,
            r#"
imports = ["b.toml"]

[logging]
level = "info"
"#
        )
        .unwrap();

        let b_path = temp_dir.join("b.toml");
        let mut b_file = fs::File::create(&b_path).unwrap();
        write!(
            b_file,
            r#"
imports = ["a.toml"]

[locations]
"#
        )
        .unwrap();

        let result = PartialConfig::load_with_imports(std::slice::from_ref(&a_path));
        assert!(result.is_err());
        match result.unwrap_err() {
            LoadError::ImportCycle { .. } => {}
            _ => panic!("Expected ImportCycle error"),
        }

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_merge_empty_configs() {
        let configs = vec![];
        let (result, diagnostics) = PartialConfig::merge(configs);

        assert!(result.logging.is_none());
        assert!(result.locations.is_none());
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_merge_single_config() {
        let config = PartialConfig {
            logging: Some(PartialLoggingConfig {
                level: Some(toml::Spanned::new(0..4, LogLevel::Info)),
                overrides: None,
            }),
            ..Default::default()
        };

        let (result, diagnostics) = PartialConfig::merge(vec![config]);

        assert!(result.logging.is_some());
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_merge_non_overlapping_configs() {
        let config1 = PartialConfig {
            logging: Some(PartialLoggingConfig {
                level: Some(toml::Spanned::new(0..4, LogLevel::Info)),
                overrides: None,
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("config1.toml"),
                content: String::new(),
            }),
            ..Default::default()
        };

        let config2 = PartialConfig {
            locations: Some(PartialLocationsConfig {
                default: None,
                locations: {
                    let mut map = HashMap::new();
                    map.insert(
                        "home".to_string(),
                        PartialLocation {
                            latitude: Some(59.9139),
                            longitude: Some(10.7522),
                            elevation_m: None,
                            timezone: None,
                        },
                    );
                    map
                },
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("config2.toml"),
                content: String::new(),
            }),
            ..Default::default()
        };

        let (result, diagnostics) = PartialConfig::merge(vec![config1, config2]);

        assert!(result.logging.is_some());
        assert!(result.locations.is_some());
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_merge_conflicting_logging_level() {
        let content1 = r#"[logging]
level = "info"
"#;
        let content2 = r#"[logging]
level = "debug"
"#;

        let config1 = PartialConfig {
            logging: Some(PartialLoggingConfig {
                level: Some(toml::Spanned::new(10..24, LogLevel::Info)),
                overrides: None,
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config1.toml"),
                content: content1.to_string(),
            }),
            ..Default::default()
        };

        let config2 = PartialConfig {
            logging: Some(PartialLoggingConfig {
                level: Some(toml::Spanned::new(10..25, LogLevel::Debug)),
                overrides: None,
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config2.toml"),
                content: content2.to_string(),
            }),
            ..Default::default()
        };

        let (result, diagnostics) = PartialConfig::merge(vec![config1, config2]);

        // First-wins: should keep Info
        assert!(result.logging.is_some());
        assert_eq!(
            *result.logging.unwrap().level.unwrap().get_ref(),
            LogLevel::Info
        );

        // Should have 1 error diagnostic
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].is_error());
    }

    #[test]
    fn test_merge_empty_config_warning() {
        let config = PartialConfig {
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/empty.toml"),
                content: String::new(),
            }),
            ..Default::default()
        };

        let (_result, diagnostics) = PartialConfig::merge(vec![config]);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].is_warning());
    }

    #[test]
    fn test_merge_location_conflict() {
        let content1 = r#"[locations.home]
latitude = 59.9139
longitude = 10.7522
"#;
        let content2 = r#"[locations.home]
latitude = 60.0
longitude = 11.0
"#;

        let config1 = PartialConfig {
            locations: Some(PartialLocationsConfig {
                default: None,
                locations: {
                    let mut map = HashMap::new();
                    map.insert(
                        "home".to_string(),
                        PartialLocation {
                            latitude: Some(59.9139),
                            longitude: Some(10.7522),
                            elevation_m: None,
                            timezone: None,
                        },
                    );
                    map
                },
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config1.toml"),
                content: content1.to_string(),
            }),
            ..Default::default()
        };

        let config2 = PartialConfig {
            locations: Some(PartialLocationsConfig {
                default: None,
                locations: {
                    let mut map = HashMap::new();
                    map.insert(
                        "home".to_string(),
                        PartialLocation {
                            latitude: Some(60.0),
                            longitude: Some(11.0),
                            elevation_m: None,
                            timezone: None,
                        },
                    );
                    map
                },
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config2.toml"),
                content: content2.to_string(),
            }),
            ..Default::default()
        };

        let (result, diagnostics) = PartialConfig::merge(vec![config1, config2]);

        // First-wins: should keep first location's fields
        let locations = result.locations.unwrap();
        let home = locations.locations.get("home").unwrap();
        assert_eq!(home.latitude.unwrap(), 59.9139);
        assert_eq!(home.longitude.unwrap(), 10.7522);

        // Should have 2 error diagnostics (one for latitude, one for longitude)
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].is_error());
        assert!(diagnostics[1].is_error());
    }

    #[test]
    fn test_merge_multiple_conflicts() {
        let content = r#"[logging]
level = "info"

[locations]
default = "home"
"#;

        let config1 = PartialConfig {
            logging: Some(PartialLoggingConfig {
                level: Some(toml::Spanned::new(10..24, LogLevel::Info)),
                overrides: None,
            }),
            locations: Some(PartialLocationsConfig {
                default: Some(toml::Spanned::new(50..54, "home".to_string())),
                locations: HashMap::new(),
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config1.toml"),
                content: content.to_string(),
            }),
            ..Default::default()
        };

        let config2 = PartialConfig {
            logging: Some(PartialLoggingConfig {
                level: Some(toml::Spanned::new(10..25, LogLevel::Debug)),
                overrides: None,
            }),
            locations: Some(PartialLocationsConfig {
                default: Some(toml::Spanned::new(50..54, "work".to_string())),
                locations: HashMap::new(),
            }),
            source: Some(SourceInfo {
                file_path: PathBuf::from("/tmp/config2.toml"),
                content: content.to_string(),
            }),
            ..Default::default()
        };

        let (_result, diagnostics) = PartialConfig::merge(vec![config1, config2]);

        // Should have 2 error diagnostics (logging.level and locations.default)
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|d| d.is_error()));
    }
}
