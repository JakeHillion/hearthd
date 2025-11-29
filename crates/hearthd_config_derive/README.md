# hearthd_config_derive

Procedural macros for generating mergeable configuration structures with automatic conflict detection and source tracking.

## Overview

This crate provides derive macros that automatically generate code for loading, merging, and validating TOML configuration files. It solves the problem of managing configuration split across multiple files while detecting conflicts and providing helpful error messages with exact source locations.

## Features

- **Automatic Partial struct generation**: Converts your config struct into a `Partial` variant with `Option` fields
- **Multi-file merging**: Load and merge multiple TOML files with first-wins semantics
- **Conflict detection**: Automatically detects when the same field is defined in multiple files
- **Field-level merging**: HashMap of structs can be defined across multiple files and merged field-by-field
- **Import system**: Recursive import resolution with cycle detection
- **Source tracking**: Uses `toml::Spanned` to track exact source locations for error reporting
- **Type safety**: All code generated at compile time with full type checking

## Usage

### Basic Example

```rust
use hearthd_config::{MergeableConfig, SubConfig};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(MergeableConfig, Deserialize)]
struct Config {
    port: u16,
    host: String,
    http: HttpConfig,
}

#[derive(SubConfig, Deserialize)]
struct HttpConfig {
    timeout_ms: u32,
    max_connections: Option<usize>,
}

// Load and merge configuration files
let configs = PartialConfig::load_with_imports(&["base.toml", "override.toml"])?;
let merged = PartialConfig::merge(configs)?;

// Convert to final config with validation
let config: Config = merged.try_into()?;
```

### HashMap Merging

```rust
#[derive(MergeableConfig, Deserialize)]
struct Config {
    #[serde(flatten)]
    locations: HashMap<String, Location>,
}

#[derive(SubConfig, Deserialize)]
#[config(no_span)]  // Disable span tracking for HashMap values
struct Location {
    latitude: f64,
    longitude: f64,
}
```

With this setup, you can define locations across multiple files:

**base.toml:**
```toml
[home]
latitude = 37.7749
```

**override.toml:**
```toml
[home]
longitude = -122.4194  # Merges with base.toml's latitude

[work]
latitude = 37.8044
longitude = -122.2712
```

The `home` location will be merged field-by-field, while `work` is only in one file.

### Import System

**main.toml:**
```toml
imports = ["base.toml", "local.toml"]

port = 8080
```

**base.toml:**
```toml
host = "localhost"
```

Imports are resolved recursively with cycle detection.

## Derive Macros

### `#[derive(MergeableConfig)]`

Use this for your root configuration struct. It generates:

- `Partial{TypeName}` struct with optional fields
- `from_file(path: impl AsRef<Path>)` - Load single TOML file
- `load_with_imports(paths: &[impl AsRef<Path>])` - Load with import resolution
- `merge(configs: Vec<Self>)` - Merge multiple configs with conflict detection

### `#[derive(SubConfig)]`

Use this for nested configuration structs. It generates:

- `Partial{TypeName}` struct with optional fields
- `merge_from(&mut self, other: Self, current_path: &str)` - Field-level merge

The difference is that `SubConfig` doesn't generate file loading methods since nested types are loaded as part of the parent.

## Attributes

### `#[config(no_span)]`

Disables `toml::Spanned` wrapping for all fields in the struct. Useful for types used as HashMap values where source tracking isn't needed and would complicate deserialization.

```rust
#[derive(SubConfig, Deserialize)]
#[config(no_span)]
struct Location {
    latitude: f64,
    longitude: f64,
}
```

### `#[serde(flatten)]`

Mark HashMap fields that should be flattened in TOML:

```rust
#[derive(MergeableConfig, Deserialize)]
struct Config {
    port: u16,
    #[serde(flatten)]
    locations: HashMap<String, Location>,
}
```

This allows locations to be defined at the top level:

```toml
port = 8080

[home]
latitude = 37.7749
longitude = -122.4194
```

## Supported Field Types

- Primitives: `bool`, `i8`-`i128`, `u8`-`u128`, `f32`, `f64`, `String`
- `Option<T>` where T is any supported type
- `HashMap<K, V>` where:
  - K is any hashable type
  - V is either a primitive or a struct with `#[derive(SubConfig)]`
- Nested structs with `#[derive(SubConfig)]`
- Custom simple types like enums (hardcoded in macro, extensibility limited)

**Not supported**: `Vec<T>`, `BTreeMap`, `HashSet`, or other collection types.

## How It Works

1. The macro analyzes your struct definition
2. Generates a `Partial` version where fields become `Option<Spanned<T>>`
3. Generates merge logic based on field types:
   - Simple fields: first-wins with conflict detection
   - HashMap: per-key conflict detection
   - HashMap of structs: recursive field-level merging
   - Nested structs: recursive merging
4. Tracks source locations for error reporting

## Error Handling

Conflicts are reported with exact source locations:

```
Error: Configuration conflict

  × Field 'port' defined in multiple files
   ╭─[base.toml:1:1]
 1 │ port = 8080
   · ──────┬─────
   ·       ╰── first defined here
   ╰────
   ╭─[override.toml:1:1]
 1 │ port = 3000
   · ──────┬─────
   ·       ╰── conflicts with previous definition
   ╰────
```

## License

Apache-2.0
