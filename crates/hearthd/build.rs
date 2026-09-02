//! Records the build-time inputs that locate the Home Assistant shim's assets.
//!
//! The shim needs things at runtime that are not products of the Rust build: a
//! Python interpreter carrying Home Assistant's dependencies, hearthd's own
//! shim package and runner, and the Home Assistant source that supplies
//! `homeassistant.components.*`. A packaged build has no working directory to
//! resolve those against, so the packaging passes their absolute paths in the
//! build environment and `ha::paths` reads them with `option_env!`.
//!
//! `option_env!` is expanded by rustc, not here; this file exists only to tell
//! Cargo that the compiled output depends on those variables. Without it Cargo
//! keeps a stale binary when the packaging changes where the assets live.

fn main() {
    for var in [
        "HEARTHD_PYTHON_INTERPRETER",
        "HEARTHD_PYTHON_ASSETS",
        "HEARTHD_PYTHON_PATH",
        "HEARTHD_HA_SOURCE",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
}
