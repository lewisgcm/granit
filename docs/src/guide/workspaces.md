# Workspaces

A workspace is the repository root. It's marked by a `granit.toml`:

```toml
[workspace]
name = "example"
# Member package directories. Globs and explicit paths both work.
members = ["packages/*", "tools/codegen"]

[nixpkgs]
# A flake reference for nixpkgs.
ref = "github:NixOS/nixpkgs/nixpkgs-unstable"
```

## `[workspace]`

| Key       | Description                                                                     |
| --------- | ------------------------------------------------------------------------------- |
| `name`    | The workspace name.                                                             |
| `members` | Directories that contain a `package.toml`. Globs (`packages/*`) and plain paths both work. |

## `[nixpkgs]`

| Key   | Description |
| ----- | ----------- |
| `ref` | A flake reference for nixpkgs. Every package's `tools` are resolved against it. |

A branch ref such as `github:NixOS/nixpkgs/nixos-24.05` is mutable, but
`granit.lock` pins the exact commit it resolved to. To pin by hand, use a commit
SHA: `github:NixOS/nixpkgs/<sha>`. See [Lock file](lock-and-outputs.md).

## `[[overlays]]` (optional)

Overlays add or override packages on top of nixpkgs, for example an internal
package set. Each entry is a flake that exposes `overlays.default`. They're
applied in the order you declare them.

```toml
[[overlays]]
source = "git+https://example.com/my-nix-overlay"
```

See the [private overlays example](../examples/overlays.md).

## Running commands from anywhere

Granit walks up from the current directory to find `granit.toml`. Inside a
package directory, `build`, `test` and `run` default to that package:

```sh
cd packages/api
granit build        # same as `granit build api` from the root
```
