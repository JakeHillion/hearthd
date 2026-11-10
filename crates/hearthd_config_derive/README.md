# hearthd_config_derive

The derive macros behind `hearthd_config`, which re-exports them. Use them
from there.

- `FromConfig` reads a struct with named fields from a table, field by field.
  It understands `#[config(default = <expr>)]` and `#[config(flatten)]`.
- `ConfigValue` reads any `serde::Deserialize` type as a single value.

See the `hearthd_config` README for how configuration is read and what the
attributes mean.

## License

Apache-2.0
