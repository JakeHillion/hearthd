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
            "rustfmt"
          ];
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

          treefmtEval = treefmt-nix.lib.evalModule pkgs {
            projectRootFile = "flake.nix";
            programs = {
              rustfmt = {
                enable = true;
                package = toolchain;
              };
              nixpkgs-fmt.enable = true;
            };
          };

          src = craneLib.cleanCargoSource (craneLib.path ./.);
          inherit (craneLib.crateNameFromCargoToml { inherit src; }) version;

          fileSetForCrate = crate:
            lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                (craneLib.fileset.commonCargoSources ./crates/hearthd)
              ];
            };

          commonArgs = {
            inherit src;
            strictDeps = true;
            buildInputs = [ ];
            nativeBuildInputs = [ ];
          };

          individualCrateArgs = commonArgs // {
            inherit cargoArtifacts;
            inherit (craneLib.crateNameFromCargoToml { inherit src; }) version;
            doCheck = false;
          };

          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "hearthd-deps";
            version = "git";
          });

          hearthd = craneLib.buildPackage (individualCrateArgs // {
            pname = "hearthd";
            cargoExtraArgs = "-p hearthd";
            src = fileSetForCrate ./crates/hearthd;
          });

          # Python environment configuration
          haPythonEnv = pkgs.callPackage ./nixos/pkgs/ha-python-env.nix { };
        in
        {
          packages = {
            inherit hearthd;
            default = hearthd;
          };

          devShells.default = craneLib.devShell {
            checks = self.checks.${system};
            packages = with pkgs; [
              rust-analyzer
              treefmtEval.config.build.wrapper
              cargo-insta
            ];

            HA_PYTHON_INTERPRETER = "${haPythonEnv}/bin/python";
          };

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
              inherit src advisory-db;
            };

            hearthd-deny = craneLib.cargoDeny {
              inherit src;
            };

            hearthd-nextest = craneLib.cargoNextest (commonArgs // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
              cargoNextestPartitionsExtraArgs = "--no-tests=pass";
            });
          };
        });
}
