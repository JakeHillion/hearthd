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

          hearthd = craneLib.buildPackage (commonArgs // {
            pname = "hearthd";
            cargoExtraArgs = "-p hearthd";
            cargoArtifacts = hearthd_config;
            doCheck = false;
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
              cargo-insta
              fmt-toolchain
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
