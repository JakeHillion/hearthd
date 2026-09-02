{
  description = "hearthd";

  nixConfig = {
    extra-substituters = [
      "https://hearthd.cachix.org"
    ];
    extra-trusted-public-keys = [
      "hearthd.cachix.org-1:Lt/GTziCLrilXymMR1tEX1TZkv5ZEqF6JKfyS5aGEqY="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";

    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";

    advisory-db.url = "github:rustsec/advisory-db";
    advisory-db.flake = false;
  };

  outputs = { self, nixpkgs, flake-utils, treefmt-nix, fenix, crane, advisory-db }:
    {
      nixosModules.default = { config, lib, pkgs, ... }: {
        imports = [ ./nixos/modules/hearthd.nix ];
        _module.args = { hearthd-flake = self; };
      };
    } // flake-utils.lib.eachSystem [ "aarch64-linux" "x86_64-linux" ]
      (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          lib = pkgs.lib;
          toolchain = fenix.packages.${system}.stable.withComponents [
            "cargo"
            "clippy"
            "rust-src"
            "rustc"
          ];
          fmt-toolchain = fenix.packages.${system}.default.withComponents [
            "rustfmt"
          ];
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

          treefmtEval = treefmt-nix.lib.evalModule pkgs {
            projectRootFile = "flake.nix";
            programs = {
              rustfmt = {
                enable = true;
                package = fmt-toolchain;
              };
              nixpkgs-fmt.enable = true;
              ruff-format.enable = true;
            };
            settings.formatter.rustfmt.options = [
              "--config-path"
              "${./rustfmt.toml}"
            ];
          };

          src = craneLib.cleanCargoSource (craneLib.path ./.);
          inherit (craneLib.crateNameFromCargoToml { inherit src; }) version;

          fileSetForCrate = cratePath:
            lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                (craneLib.fileset.commonCargoSources cratePath)
              ];
            };

          commonArgs = {
            inherit src;
            strictDeps = true;
            buildInputs = [ ];
            nativeBuildInputs = [ ];
          };

          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "hearthd-deps";
            version = "git";
          });

          hearthd_config_derive = craneLib.buildPackage (commonArgs // {
            pname = "hearthd_config_derive";
            cargoExtraArgs = "-p hearthd_config_derive";
            cargoArtifacts = cargoArtifacts;
            doCheck = false;
          });

          hearthd_config = craneLib.buildPackage (commonArgs // {
            pname = "hearthd_config";
            cargoExtraArgs = "-p hearthd_config";
            cargoArtifacts = hearthd_config_derive;
            doCheck = false;
          });

          # Home Assistant integrations the Python shim is built to run.
          #
          # Naming one here is the whole declaration: nixpkgs looks its Python
          # dependencies up in its own component-packages.nix, so adding an
          # integration no longer means working out what it needs and
          # hand-maintaining a package list. When nixpkgs gets one wrong, the
          # override below takes `extraPackages` and `packageOverrides` too.
          haComponents = [ "met" ];

          homeAssistant = pkgs.home-assistant.override {
            extraComponents = haComponents;
          };

          # Refuse at evaluation rather than as a ModuleNotFoundError inside
          # the Python child, which is a much worse place to learn that an
          # integration is not packaged.
          unpackagedComponents =
            lib.subtractLists homeAssistant.availableComponents haComponents;

          haSource =
            assert lib.assertMsg (unpackagedComponents == [ ])
              ("Home Assistant components not packaged in nixpkgs: "
              + lib.concatStringsSep ", " unpackagedComponents);
            "${homeAssistant}/lib/python${homeAssistant.python3Packages.python.pythonVersion}/site-packages";

          # What the Python child imports from, in three parts, because nixpkgs
          # keeps them apart: Home Assistant itself, the dependencies its core
          # needs (`propagatedBuildInputs`, so aiohttp and voluptuous), and the
          # dependencies of the components selected above (`pythonPath`, so
          # pymetno for `met`). Omitting the last is not a build error; it
          # surfaces as a ModuleNotFoundError when the component is imported.
          haPythonPath = lib.concatStringsSep ":" [
            haSource
            (homeAssistant.python3Packages.makePythonPath homeAssistant.propagatedBuildInputs)
            homeAssistant.pythonPath
          ];

          # hearthd's own Python: the runner, and the shim package that stands
          # in for Home Assistant's core modules.
          #
          # Its own derivation rather than part of the hearthd package for two
          # reasons: the binary then references it by store path, which is what
          # keeps it in the closure without declaring the dependency anywhere
          # else; and the checks below can run against exactly what ships.
          pythonAssets = pkgs.runCommandLocal "hearthd-python-assets" { } ''
            cp -r ${lib.fileset.toSource { root = ./python; fileset = ./python; }} $out
          '';

          # Where the shim finds its assets, baked into the binary at build
          # time. See crates/hearthd/src/ha/paths.rs; nothing is resolved
          # against the working directory, because a packaged build has none.
          haAssetEnv = {
            HEARTHD_PYTHON_INTERPRETER = homeAssistant.python3Packages.python.interpreter;
            HEARTHD_PYTHON_ASSETS = "${pythonAssets}";
            HEARTHD_PYTHON_PATH = haPythonPath;
            HEARTHD_HA_SOURCE = haSource;
          };

          hearthd = craneLib.buildPackage (commonArgs // haAssetEnv // {
            pname = "hearthd";
            cargoExtraArgs = "-p hearthd";
            cargoArtifacts = hearthd_config;
            doCheck = false;
          });
        in
        {
          packages = {
            inherit hearthd;
            default = hearthd;
          };

          devShells.default = craneLib.devShell (haAssetEnv // {
            checks = self.checks.${system};
            packages = with pkgs; [
              rust-analyzer
              cargo-insta
              fmt-toolchain
              ruff
            ];
          });

          formatter = treefmtEval.config.build.wrapper;

          checks = {
            inherit hearthd;

            hearthd-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

            hearthd-doc = craneLib.cargoDoc (commonArgs // {
              inherit cargoArtifacts;
              env.RUSTDOCFLAGS = "--deny warnings";
            });

            formatting = treefmtEval.config.build.check self;

            hearthd-audit = craneLib.cargoAudit {
              inherit advisory-db;
              # Not the shared `src`: cleanCargoSource strips .cargo/audit.toml,
              # which is where the advisory exclusions and their justifications
              # live. cargo-audit needs only these two files.
              src = lib.fileset.toSource {
                root = ./.;
                fileset = lib.fileset.unions [
                  ./Cargo.lock
                  ./.cargo/audit.toml
                ];
              };
            };

            hearthd-deny = craneLib.cargoDeny {
              inherit src;
            };

            # The Python half of the Home Assistant shim gets the same
            # treatment as the Rust: nothing lands unlinted. `formatting`
            # above covers ruff-format; these cover what a formatter cannot.

            hearthd-python-lint = pkgs.runCommandLocal "hearthd-python-lint"
              { nativeBuildInputs = [ pkgs.ruff ]; } ''
              ruff check --no-cache --config ${./ruff.toml} ${pythonAssets}
              touch $out
            '';

            # Every module has to at least import, which no linter checks and
            # nothing else here would catch: the shim exists to satisfy Home
            # Assistant's imports, so a shim that cannot be imported is the one
            # failure that makes all the others irrelevant.
            #
            # Importing a real component is the point. `homeassistant.core`
            # resolving to the shim while `homeassistant.components.met`
            # resolves to Home Assistant's own tree is the layering this whole
            # design rests on, and this is the cheapest place to prove it holds
            # against the Home Assistant version we actually ship.
            hearthd-python-imports = pkgs.runCommandLocal "hearthd-python-imports"
              {
                inherit (haAssetEnv) HEARTHD_HA_SOURCE;
                PYTHONPATH = "${pythonAssets}/homeassistant-shim:${haPythonPath}";
              } ''
              ${homeAssistant.python3Packages.python.interpreter} - <<'PY'
              import homeassistant.components.weather
              import homeassistant.config_entries
              import homeassistant.core
              import homeassistant.helpers.aiohttp_client
              import homeassistant.helpers.device_registry
              import homeassistant.helpers.entity_registry
              import homeassistant.helpers.sun
              import homeassistant.helpers.update_coordinator
              import homeassistant.util.dt

              assert homeassistant.core.__file__.startswith("${pythonAssets}"), (
                  "homeassistant.core must resolve to hearthd's shim, not "
                  f"Home Assistant's own: {homeassistant.core.__file__}"
              )

              import homeassistant.components.met as met

              assert not met.__file__.startswith("${pythonAssets}"), (
                  "homeassistant.components.met must resolve to Home "
                  f"Assistant's own tree: {met.__file__}"
              )
              assert hasattr(met, "async_setup_entry"), "met has no async_setup_entry"
              PY
              touch $out
            '';

            hearthd-nextest = craneLib.cargoNextest (commonArgs // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
              cargoNextestPartitionsExtraArgs = "--no-tests=pass";
            });
          };
        });
}
