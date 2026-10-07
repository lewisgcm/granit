# CLI reference

```text
granit <COMMAND>
```

| Command                          | Description |
| -------------------------------- | ----------- |
| `granit init`                    | Create a new workspace in the current directory: `granit.toml`, a sample `packages/hello`, and a `.gitignore`. Refuses to overwrite an existing `granit.toml`. |
| `granit build [package]`         | Build a package and its dependencies in the Nix sandbox, then link outputs under `.granit/build/`. |
| `granit build --emit-only`       | Print the generated `flake.nix` to stdout without building. |
| `granit test [package]`          | Run a package's `test` command in the Nix sandbox. |
| `granit run <command> [package]` | Run a named command from `[commands]` in place, with the package's tools on `PATH`. |
| `granit graph`                   | Print the dependency graph and build order. |
| `granit search <query>`          | Search the pinned nixpkgs for tools. Prints attribute names, versions and descriptions. |
| `granit update`                  | Re-resolve nixpkgs and overlays and rewrite `granit.lock`. |
| `granit doctor`                  | Check that Nix is installed and flakes work. |
| `granit --version`               | Print the Granit version. |
| `granit help [command]`          | Show help. |

## Choosing the package

`build`, `test` and `run` take an optional package name. Without one:

- **Inside a package directory**, the command applies to that package.
- **At the workspace root**, it applies to **every** package. For `granit run`,
  every package then has to define that command.

```sh
granit build              # at the root: every package
granit build server       # server and its dependencies
cd packages/server && granit test   # just server
```
