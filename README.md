# hearthd

## Installation

### NixOS

hearthd provides a NixOS module for easy integration into your system configuration.

#### Using with Flakes

Add hearthd to your flake inputs and import the NixOS module:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    hearthd.url = "github:yourusername/hearthd";  # Update with your repo
  };

  outputs = { self, nixpkgs, hearthd }: {
    nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        hearthd.nixosModules.default

        # Your configuration
        {
          services.hearthd = {
            enable = true;
            config = {
              logging = {
                level = "info";
                overrides = {
                  "hearthd::config" = "debug";
                };
              };
            };
            # Optionally provide secret config files (e.g., for location coordinates)
            # secretConfigs = [ config.age.secrets."hearthd/locations.toml".path ];
          };
        }
      ];
    };
  };
}
```

#### Configuration

The NixOS module supports the following options:

- `services.hearthd.enable`: Enable the hearthd service (default: `false`)
- `services.hearthd.package`: The hearthd package to use (default: `pkgs.hearthd` from overlay)
- `services.hearthd.config`: Main configuration (TOML format, converted from Nix attrset)
- `services.hearthd.secretConfigs`: List of paths to secret TOML config files

Secret configuration files (like location coordinates) should be managed with tools like agenix or sops-nix and must not be in the Nix store.

Example with secrets:

```nix
{
  services.hearthd = {
    enable = true;
    config = {
      # Non-secret configuration here
    };
    secretConfigs = [
      config.age.secrets."hearthd/locations.toml".path
    ];
  };
}
```
