# homeassistant-shim

hearthd's replacement for the parts of Home Assistant's core that an
integration imports.

Home Assistant components are run unmodified, out of a real Home Assistant
source tree. What they import from `homeassistant.core`,
`homeassistant.config_entries` and `homeassistant.helpers.*` is served from
here instead, so that entity registration, coordinator scheduling and state
updates arrive at hearthd's Rust side over a socketpair rather than inside a
Home Assistant process.

## How it is assembled

`../runner.py` puts this package first on `sys.path` and the real Home
Assistant source last. `homeassistant.core` therefore resolves here, while
`homeassistant.components.met` resolves to Home Assistant's own tree, which
`homeassistant/components/__init__.py` splices in by extending `__path__`.

Both locations are passed in by the Rust side as `HEARTHD_SHIM` and
`HEARTHD_HA_SOURCE`; see `crates/hearthd/src/ha/paths.rs` for where they come
from. There is nothing to install and no virtualenv: the interpreter, this
package and the Home Assistant source are all chosen when hearthd is built.

## Checks

`nix flake check` runs `ruff format`, `ruff check` and an import check that
asserts the layering above still holds against the Home Assistant version
being shipped.
