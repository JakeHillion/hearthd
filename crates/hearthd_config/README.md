# hearthd_config

Reads configuration that is split across several TOML files into one typed
value, and says precisely what is wrong when it cannot.

## How configuration is read

Every file is parsed first, keeping the position of each key and value. A type
is then read through `FromConfig`, which is handed every place its path is
written, across all the files, and decides what several definitions mean:

- A single value (a number, string, boolean, or any `ConfigValue` type) is
  written once. A second definition is a conflict, and both are shown.
- A struct is one table however many files its keys are spread over.
- A map gathers its entries from every file, and each entry is in turn read
  from everywhere it is written. Keys are parsed with `FromStr`.
- Lists are joined in the order the files were loaded.
- An `Option` is `None` when nothing is written.

Nothing is silently overridden, and every problem found is reported, not just
the first.

## Usage

```rust
use std::collections::HashMap;

use hearthd_config::ConfigValue;
use hearthd_config::FromConfig;
use serde::Deserialize;

#[derive(Debug, FromConfig)]
struct Config {
    http: Http,
    pdus: HashMap<String, Pdu>,
}

#[derive(Debug, FromConfig)]
struct Http {
    #[config(default = "127.0.0.1")]
    listen: String,
    #[config(default = 8565)]
    port: u16,
}

#[derive(Debug, FromConfig)]
struct Pdu {
    model: Model,
    host: String,
    outlets: HashMap<u8, Outlet>,
}

#[derive(Debug, FromConfig)]
struct Outlet {
    label: Option<String>,
}

#[derive(Debug, Deserialize, ConfigValue)]
#[serde(rename_all = "snake_case")]
enum Model {
    ApcAp7920,
    ApcAp7921,
}

let (config, diagnostics) = hearthd_config::load::<Config>(&paths);
```

`load` returns the value, if one could be built, and every diagnostic. A value
can come back alongside errors, so check the diagnostics before using it.

### `#[derive(FromConfig)]`

For a struct with named fields. Each field is read from the key of its name.

- `#[config(default = <expr>)]` gives the field's value when no file sets it.
  A string literal is converted with `Into`, so it can stand for a `String`.
- `#[config(flatten)]` on one map field makes it take every key the struct's
  other fields do not read.

A field with no default that nothing sets is an error. That includes the fields
of a section no file mentions, which is read as an empty table.

### `#[derive(ConfigValue)]`

For a type written as a single value, such as an enum or a string that is
parsed. The type says how it is read by implementing `serde::Deserialize`, and
whatever its deserializer rejects is reported where it is written.

## Imports

A file may list others to load before it:

```toml
imports = ["base.toml", "secrets.toml"]
```

Relative paths are taken from the importing file's directory. A cycle is an
error.

## Diagnostics

`format_diagnostics` renders for a terminal and `format_diagnostics_plain`
leaves out the colour.

```
Error: Validation error in 'pdus.rack.host'
   ╭─[ config.toml:4:1 ]
   │
 4 │ [pdus.rack]
   │ ─────┬─────
   │      ╰─────── host is required
───╯
Error: Validation error in 'pdus.rack.model'
   ╭─[ config.toml:5:9 ]
   │
 5 │ model = "apc_ap9999"
   │         ──────┬─────
   │               ╰─────── unknown variant `apc_ap9999`, expected `apc_ap7920` or `apc_ap7921`
───╯
Error: Merge conflict in field 'http.port'
   ╭─[ base.toml:2:8 ]
   │
 2 │ port = 8565
   │        ──┬─
   │          ╰─── first definition here
   │
   ├─[ local.toml:2:8 ]
   │
 2 │ port = 9000
   │        ──┬─
   │          ╰─── conflicts with this definition
   │
   │ Note: Field 'http.port' defined in multiple config files
───╯
```

## License

Apache-2.0
