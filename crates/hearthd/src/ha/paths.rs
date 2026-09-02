//! Locating the assets the Home Assistant shim needs at runtime.
//!
//! The shim runs a Python child process, and that process needs four things
//! this crate does not build: an interpreter, hearthd's own shim package and
//! runner, the Home Assistant source that supplies `homeassistant.components.*`,
//! and a `PYTHONPATH` carrying the components' dependencies.
//!
//! # Nothing is resolved against the working directory
//!
//! This module exists because the shim used to spawn the literal relative path
//! `python/runner.py` and probe the literal `vendor/ha-core`, both of which are
//! resolved against the process working directory. That made the integration
//! work only under `cargo run` from the workspace root: the packaged binary has
//! no such directory (the systemd unit sets none, and the assets are not in the
//! Cargo build source at all), and the tests had to `chdir` the whole process,
//! which is global and forced the suite onto a single thread.
//!
//! Every path here is therefore absolute. Each is taken from the first source
//! that supplies it:
//!
//! 1. configuration, so a deployment can point at its own copies;
//! 2. a value baked in at build time, which is how packaging injects store
//!    paths — see `build.rs`;
//! 3. for hearthd's own Python only, the source tree this crate was compiled
//!    from, which is what makes a plain `cargo run` work from anywhere.
//!
//! There is deliberately no fallback to `python3` on `PATH`. An interpreter
//! without Home Assistant's dependencies does not fail here, it fails much
//! later as an import error inside the child, which is a far worse place to
//! discover the mistake.

use std::path::Path;
use std::path::PathBuf;

/// Locations baked in at compile time from the build environment.
///
/// `None` under a plain `cargo build`, which is what selects the source-tree
/// fallback below.
const BUILT_IN_INTERPRETER: Option<&str> = option_env!("HEARTHD_PYTHON_INTERPRETER");
const BUILT_IN_ASSETS: Option<&str> = option_env!("HEARTHD_PYTHON_ASSETS");
const BUILT_IN_PYTHON_PATH: Option<&str> = option_env!("HEARTHD_PYTHON_PATH");
const BUILT_IN_HA_SOURCE: Option<&str> = option_env!("HEARTHD_HA_SOURCE");

/// The workspace's own `python/` tree, absolute, as of compile time.
///
/// Only a development convenience: it points into the source checkout, which a
/// packaged build cannot rely on still being there. Packaging is expected to
/// bake `HEARTHD_PYTHON_ASSETS` instead.
const SOURCE_TREE_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../python");

/// The runner script, relative to the assets root.
const RUNNER: &str = "runner.py";

/// hearthd's replacement `homeassistant` package, relative to the assets root.
const SHIM: &str = "homeassistant-shim";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "no Python interpreter available: set `python_interpreter` under \
         [integrations.ha], or build hearthd with HEARTHD_PYTHON_INTERPRETER set"
    )]
    NoInterpreter,

    #[error(
        "no Home Assistant source available: set `ha_source` under \
         [integrations.ha], or build hearthd with HEARTHD_HA_SOURCE set"
    )]
    NoHaSource,

    #[error("{what} not found at {}", .path.display())]
    Missing { what: &'static str, path: PathBuf },
}

pub type Result<T> = ::core::result::Result<T, Error>;

/// Where each of the shim's runtime assets lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Python interpreter to spawn.
    pub interpreter: PathBuf,

    /// hearthd's runner script, the child process's entry point.
    pub runner: PathBuf,

    /// hearthd's replacement `homeassistant` package. Takes precedence over
    /// [`Self::ha_source`] on the child's `sys.path`, which is what lets the
    /// shim stand in for Home Assistant's core modules while the real ones
    /// supply the components.
    pub shim: PathBuf,

    /// Directory containing a `homeassistant/` package, whose `components`
    /// subdirectory holds the integrations being run.
    pub ha_source: PathBuf,

    /// `PYTHONPATH` for the child, carrying the components' dependencies.
    ///
    /// `None` when the interpreter already knows its own dependencies, which
    /// is the case for an interpreter built with its packages bundled in.
    pub python_path: Option<String>,
}

/// Configured overrides, each taking precedence over the built-in default.
#[derive(Debug, Default, Clone, Copy)]
pub struct Overrides<'a> {
    pub interpreter: Option<&'a Path>,
    pub assets: Option<&'a Path>,
    pub ha_source: Option<&'a Path>,
    pub python_path: Option<&'a str>,
}

impl Paths {
    /// Resolve every asset location, checking that each exists.
    ///
    /// Existence is checked here, once, rather than left to the child process:
    /// a missing interpreter or a missing Home Assistant checkout otherwise
    /// surfaces as a spawn failure or a Python traceback, neither of which
    /// says which of the four inputs was wrong.
    pub fn resolve(overrides: Overrides<'_>) -> Result<Self> {
        let interpreter = overrides
            .interpreter
            .map(Path::to_path_buf)
            .or_else(|| BUILT_IN_INTERPRETER.map(PathBuf::from))
            .ok_or(Error::NoInterpreter)?;

        let assets = overrides
            .assets
            .map(Path::to_path_buf)
            .or_else(|| BUILT_IN_ASSETS.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(SOURCE_TREE_ASSETS));

        let ha_source = overrides
            .ha_source
            .map(Path::to_path_buf)
            .or_else(|| BUILT_IN_HA_SOURCE.map(PathBuf::from))
            .ok_or(Error::NoHaSource)?;

        let python_path = overrides
            .python_path
            .map(str::to_string)
            .or_else(|| BUILT_IN_PYTHON_PATH.map(str::to_string));

        let resolved = Self {
            interpreter,
            runner: assets.join(RUNNER),
            shim: assets.join(SHIM),
            ha_source,
            python_path,
        };
        resolved.check()?;
        Ok(resolved)
    }

    /// Report the first asset that is not where it should be.
    fn check(&self) -> Result<()> {
        let missing = |what, path: &Path| Error::Missing {
            what,
            path: path.to_path_buf(),
        };

        if !self.interpreter.is_file() {
            return Err(missing("Python interpreter", &self.interpreter));
        }
        if !self.runner.is_file() {
            return Err(missing("hearthd's Python runner", &self.runner));
        }
        if !self.shim.is_dir() {
            return Err(missing("hearthd's Home Assistant shim", &self.shim));
        }

        // The components are what the shim is for, so this checks the whole
        // path rather than just the root: a directory that exists but holds no
        // `homeassistant/components` is the shape of a submodule that was
        // never initialised, and saying so is more use than "not found".
        let components = self.ha_source.join("homeassistant").join("components");
        if !components.is_dir() {
            return Err(missing("Home Assistant components", &components));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A directory laid out the way `resolve` expects to find one.
    fn assets(root: &Path) -> PathBuf {
        let assets = root.join("python");
        fs::create_dir_all(assets.join(SHIM)).unwrap();
        fs::write(assets.join(RUNNER), "").unwrap();
        assets
    }

    fn ha_source(root: &Path) -> PathBuf {
        let source = root.join("ha");
        fs::create_dir_all(source.join("homeassistant").join("components")).unwrap();
        source
    }

    fn interpreter(root: &Path) -> PathBuf {
        let interpreter = root.join("python3");
        fs::write(&interpreter, "").unwrap();
        interpreter
    }

    #[test]
    fn overrides_win_and_produce_absolute_asset_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (assets, source, interpreter) = (assets(root), ha_source(root), interpreter(root));

        let paths = Paths::resolve(Overrides {
            interpreter: Some(&interpreter),
            assets: Some(&assets),
            ha_source: Some(&source),
            python_path: Some("/somewhere/site-packages"),
        })
        .expect("a complete set of overrides resolves");

        assert_eq!(paths.interpreter, interpreter);
        assert_eq!(paths.runner, assets.join(RUNNER));
        assert_eq!(paths.shim, assets.join(SHIM));
        assert_eq!(paths.ha_source, source);
        assert_eq!(
            paths.python_path.as_deref(),
            Some("/somewhere/site-packages")
        );
    }

    /// The failure this module exists to prevent: a relative path that happens
    /// to resolve under `cargo test` from the workspace root and nowhere else.
    #[test]
    fn resolved_paths_do_not_depend_on_the_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (assets, source, interpreter) = (assets(root), ha_source(root), interpreter(root));

        let overrides = Overrides {
            interpreter: Some(&interpreter),
            assets: Some(&assets),
            ha_source: Some(&source),
            python_path: None,
        };

        let paths = Paths::resolve(overrides).unwrap();
        for path in [
            &paths.interpreter,
            &paths.runner,
            &paths.shim,
            &paths.ha_source,
        ] {
            assert!(path.is_absolute(), "{} is not absolute", path.display());
        }
    }

    /// Every message has to name which of the four inputs was wrong, because
    /// the alternative is a Python traceback from the child that does not.
    #[test]
    fn a_missing_asset_is_named_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (assets, source, interpreter) = (assets(root), ha_source(root), interpreter(root));

        let complete = Overrides {
            interpreter: Some(&interpreter),
            assets: Some(&assets),
            ha_source: Some(&source),
            python_path: None,
        };

        let absent = root.join("absent");

        let err = Paths::resolve(Overrides {
            interpreter: Some(&absent),
            ..complete
        })
        .unwrap_err();
        assert!(err.to_string().contains("Python interpreter"), "{err}");

        let err = Paths::resolve(Overrides {
            assets: Some(&absent),
            ..complete
        })
        .unwrap_err();
        assert!(err.to_string().contains("runner"), "{err}");

        // A directory that exists but holds no components: the shape of an
        // uninitialised checkout.
        let err = Paths::resolve(Overrides {
            ha_source: Some(root),
            ..complete
        })
        .unwrap_err();
        assert!(err.to_string().contains("components"), "{err}");
    }

    /// An interpreter is never guessed. Falling back to `python3` on `PATH`
    /// finds one without Home Assistant's dependencies, which fails as an
    /// import error inside the child long after the mistake was made.
    #[test]
    fn an_unconfigured_interpreter_is_an_error_not_a_guess() {
        if BUILT_IN_INTERPRETER.is_some() {
            // Built with an interpreter baked in, so there is no unconfigured
            // case to observe.
            return;
        }

        let err = Paths::resolve(Overrides::default()).unwrap_err();
        assert!(matches!(err, Error::NoInterpreter), "{err}");
    }
}
