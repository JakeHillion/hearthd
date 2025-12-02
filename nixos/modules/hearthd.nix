{ config, lib, pkgs, hearthd-flake, ... }:

with lib;

let
  cfg = config.services.hearthd;

  # JSON format for Nix -> TOML conversion
  settingsFormat = pkgs.formats.toml { };

  # Validate that secret paths are not in the Nix store
  validateSecretPath = path:
    let
      pathStr = toString path;
      inStore = lib.hasPrefix builtins.storeDir pathStr;
    in
    if inStore then
      throw "hearthd secretConfig path '${pathStr}' is in the Nix store. Secret files must not be in the store!"
    else
      path;

  # Base config file (immutable in Nix store)
  baseConfigFile = settingsFormat.generate "hearthd-config.toml" cfg.config;

  # Wrapper script that generates secrets config and starts hearthd
  startScript = pkgs.writeShellScript "hearthd-start" ''
    set -euo pipefail

    # Build hearthd command starting with base config
    HEARTHD_CMD="${cfg.package}/bin/hearthd --config ${baseConfigFile}"

    # If we have credentials, generate secrets.toml with imports
    if [ -d "''${CREDENTIALS_DIRECTORY:-}" ] && [ -n "$(ls -A "''${CREDENTIALS_DIRECTORY}" 2>/dev/null || true)" ]; then
      SECRETS_CONFIG="''${RUNTIME_DIRECTORY}/secrets.toml"

      # Generate secrets.toml with just the imports
      echo "imports = [" > "$SECRETS_CONFIG"
      for cred in "$CREDENTIALS_DIRECTORY"/*; do
        if [ -f "$cred" ]; then
          echo "  \"$cred\"," >> "$SECRETS_CONFIG"
        fi
      done
      echo "]" >> "$SECRETS_CONFIG"

      # Add secrets config to command
      HEARTHD_CMD="$HEARTHD_CMD --config $SECRETS_CONFIG"
    fi

    # Start hearthd
    exec $HEARTHD_CMD
  '';

  # Generate LoadCredential entries for each secret config
  # Format: "secretN:path" where N is the index
  loadCredentials = lib.imap0
    (i: path: "secret${toString i}:${validateSecretPath path}")
    cfg.secretConfigs;

in
{
  options.services.hearthd = {
    enable = mkEnableOption "hearthd, a home automation daemon for location-based services";

    package = mkOption {
      type = types.package;
      default = hearthd-flake.packages.${pkgs.system}.hearthd.overrideAttrs (oldAttrs: {
        cargoExtraArgs = "-p hearthd --features systemd";
      });
      defaultText = literalExpression "hearthd-flake.packages.\${pkgs.system}.hearthd with systemd feature enabled";
      description = "The hearthd package to use. By default, built with systemd support.";
    };

    config = mkOption {
      type = settingsFormat.type;
      default = { };
      example = literalExpression ''
        {
          logging = {
            level = "info";
            overrides = {
              "hearthd::config" = "debug";
            };
          };
        }
      '';
      description = ''
        Configuration for hearthd. This will be converted to TOML format.

        See the hearthd documentation for available options.
        Secrets (like location coordinates) should be provided via `secretConfigs` instead.
      '';
    };

    secretConfigs = mkOption {
      type = types.listOf types.path;
      default = [ ];
      example = literalExpression ''
        [ config.age.secrets."hearthd/locations.toml".path ]
      '';
      description = ''
        List of paths to secret configuration files that will be imported.

        These files should be TOML formatted and contain sensitive configuration
        like location coordinates. The files will be added to the main configuration's
        `imports` list.

        Warning: These paths must NOT be in the Nix store, as they would be world-readable.
        Use a secret management solution like agenix or sops-nix.
      '';
    };
  };

  config = mkIf cfg.enable {
    systemd.services.hearthd = {
      description = "hearthd - Home automation daemon";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        Type = "notify";
        DynamicUser = true;
        RuntimeDirectory = "hearthd";
        ExecStart = startScript;
        Restart = "on-failure";
        RestartSec = "10s";

        # Load secret configs as credentials
        LoadCredential = loadCredentials;

        # Security hardening
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [ ];
      };
    };
  };
}
