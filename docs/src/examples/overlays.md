# Private overlays

nixpkgs has most tools, but you might want internal tools or patched versions
too. Put them in an **overlay** flake and add it to the workspace. Packages can
then list those tools in `tools` like any other nixpkgs attribute.

## The overlay flake

In a separate repository, for example `acme/nix-overlay`:

```nix
# flake.nix
{
  outputs = { self }: {
    overlays.default = final: prev: {
      # A new internal tool.
      acme-cli = prev.callPackage ./pkgs/acme-cli.nix { };

      # Override an existing package's version.
      terraform = prev.terraform.overrideAttrs (old: { /* ... */ });
    };
  };
}
```

## Using it

```toml
# granit.toml
[workspace]
name = "acme"
members = ["services/*"]

[nixpkgs]
ref = "github:NixOS/nixpkgs/nixos-24.05"

[[overlays]]
source = "github:acme/nix-overlay"

# Private repositories work through Nix's normal git access, e.g.:
# [[overlays]]
# source = "git+ssh://git@github.com/acme/private-overlay"
```

```toml
# services/deploy/package.toml
[package]
name = "deploy"
tools = ["acme-cli", "terraform"]

[commands]
plan = "terraform plan"
```

Overlays are applied over nixpkgs in the order they're declared, so later
overlays can override earlier ones. `granit.lock` pins each overlay's revision
alongside nixpkgs. Run `granit update` to pick up new commits.
