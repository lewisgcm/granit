# Granit

Granit is a simple monorepo build tool that wraps [Nix](https://nixos.org) flakes.

You describe your workspace and its packages in TOML. Each package names the
build-time tools it needs (pulled from nixpkgs), the outputs it produces, and the
other packages it depends on. Granit generates a `flake.nix`, pins everything in
a `granit.lock`, and runs `nix build`. You get reproducible, sandboxed builds
without writing any Nix.

```toml
# packages/api/package.toml
[package]
name = "api"
tools = ["go"]
dependencies = ["common:src"]

[outputs]
server = "bin/server"

[commands]
build = "go build -o bin/server ./cmd/server"
test  = "go test ./... && go build -o bin/server ./cmd/server"
```

```sh
granit build api      # builds api (and anything it depends on) in a Nix sandbox
```

## Why Granit?

- **Toolchains without Nix.** Ask for `go`, `nodejs_20`, `jdk21` or `python3` by
  nixpkgs attribute name and they're on `PATH` in every build, pinned to an exact
  nixpkgs revision. Everyone, including CI, gets the same versions.
- **Dependencies between packages.** One package produces something, such as a
  binary, a generated schema or its source tree, and another depends on it.
  Granit builds the producer first and puts the result where the consumer
  expects it.
- **Familiar concepts.** Workspaces, packages, dependencies and named commands,
  much like Cargo workspaces or npm workspaces, but language-agnostic.

## How it works

1. `granit.toml` at the repository root declares the workspace and which
   directories are packages.
2. Each package has a `package.toml` declaring `tools`, `dependencies`,
   `[outputs]` and `[commands]`.
3. `granit build` resolves the dependency graph, generates a flake with one
   derivation per package, and calls `nix build`.
4. Built outputs are linked under `.granit/build/<package>/result`.

Head to [Installation](installation.md) to get started.
