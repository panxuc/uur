# Nix and NixOS

The repository exports an x86_64 Linux package, an app, a development shell,
and a NixOS module. The lock file pins NixOS/nixpkgs 26.05.

## Build or run from a checkout

```bash
nix build
nix run . -- doctor
nix run . -- setup --accept-eula
nix run . -- run
```

The derivation builds the Rust supervisor, the musl/glibc-independent source
components, the PipeWire helper, and all MinGW Win64 adapters. Runtime tools
are wrapped into `PATH`; the UU client and Wine prefix remain in the user's XDG
data directory.

## NixOS module

Add this repository as a flake input and import its module:

```nix
{
  inputs.uur.url = "github:panxuc/uur";

  outputs = { self, nixpkgs, uur, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        uur.nixosModules.default
        ({ ... }: {
          programs.uur.enable = true;
        })
      ];
    };
  };
}
```

The module installs the package, loads `uinput`, and registers uur's udev rule.
The selected desktop still supplies its matching xdg-desktop-portal backend.

Override the package when following a fork or local build:

```nix
programs.uur.package = inputs.uur.packages.x86_64-linux.uur;
```
