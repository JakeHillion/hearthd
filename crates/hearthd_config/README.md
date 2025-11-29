# hearthd_config

Runtime support library for mergeable configuration with diagnostics and error reporting.

## Overview

This crate provides the runtime types and error handling for the config merging system. It re-exports the derive macros from `hearthd_config_derive` and provides diagnostic formatting using the `ariadne` library.

## Features

- Beautiful error messages with source context
- Multi-file conflict visualization
- Validation error tracking
- Load error reporting

## Usage

```rust
use hearthd_config::{MergeableConfig, SubConfig};
use serde::Deserialize;

#[derive(MergeableConfig, Deserialize)]
struct Config {
    port: u16,
    host: String,
}

impl TryFrom<PartialConfig> for Config {
    type Error = hearthd_config::ValidationError;

    fn try_from(partial: PartialConfig) -> Result<Self, Self::Error> {
        // Extract required fields with helpful errors
        let port = partial.port
            .ok_or_else(|| hearthd_config::ValidationError {
                field: "port".to_string(),
                message: "port is required".to_string(),
                source: partial.source,
            })?
            .into_inner();

        let host = partial.host
            .ok_or_else(|| hearthd_config::ValidationError {
                field: "host".to_string(),
                message: "host is required".to_string(),
                source: partial.source,
            })?
            .into_inner();

        Ok(Config { port, host })
    }
}

// Load and validate
let configs = PartialConfig::load_with_imports(&["config.toml"])?;
let merged = PartialConfig::merge(configs)?;
let config: Config = merged.try_into()?;
```

## Error Types

### `LoadError`

Errors that occur when loading TOML files:

```rust
pub struct LoadError {
    pub path: PathBuf,
    pub source: toml::de::Error,
}
```

### `MergeError`

Errors from merging configurations with conflicts:

```rust
pub struct MergeError {
    pub field: String,
    pub locations: Vec<MergeConflictLocation>,
}
```

### `ValidationError`

Errors from validating the final configuration:

```rust
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub source: Option<SourceInfo>,
}
```

## Diagnostic Output

Errors are formatted with color-coded source context:

```
Error: Configuration conflict

  × Field 'port' defined in multiple files
   ╭─[config/base.toml:5:1]
 5 │ port = 8080
   · ──────┬─────
   ·       ╰── first defined here
   ╰────
   ╭─[config/override.toml:2:1]
 2 │ port = 3000
   · ──────┬─────
   ·       ╰── conflicts with previous definition
   ╰────
```

Validation errors show the source location:

```
Error: Validation failed

  × Field 'locations.home.latitude' is required
   ╭─[config/locations.toml:1:1]
 1 │ [home]
   · ───┬───
   ·    ╰── location defined here but missing latitude
   ╰────
```

## Re-exports

This crate re-exports the derive macros for convenience:

```rust
pub use hearthd_config_derive::{MergeableConfig, SubConfig};
```

## Dependencies

- `ariadne`: Beautiful diagnostic output
- `toml`: TOML parsing with span tracking
- `serde`: Serialization framework

## License

Apache-2.0
