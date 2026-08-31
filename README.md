# Granit

A simple monorepo build tool that wraps Nix flakes.

Granit lets you define a workspace and its packages in TOML. You declare
build-time tools by name (pulled from nixpkgs), label the artifacts each package
produces, and declare artifact dependencies between packages. Granit generates a
`flake.nix`, manages a `granit.lock` for reproducibility, and shells out to
`nix build` — wrapping Nix's complexity behind a familiar, ergonomic format.

## Goals

1. **Simplified interface over nixpkgs.** Bring in build-time tools (java, node,
   gcc, …) by attribute name and get reproducible builds without writing Nix.
2. **Artifact dependencies between packages.** Package A produces file F; package
   B consumes it. Granit knows that building B requires building A first.
3. **Familiar syntax.** Concepts you already know: workspaces, dependencies,
   and labeled outputs (artifacts).

## Requirements

- [Nix](https://nixos.org/download) with the `nix-command` and `flakes`
  experimental features. Granit enables these for its own invocations, and
  `granit doctor` reports on your environment.

## Install

```sh
# Install the `granit` binary to ~/.cargo/bin (must be on your PATH).
cargo install --path .

# Verify it's available:
granit --version
granit doctor
```

Alternatively, build and run from the repo without installing:

```sh
cargo build --release        # binary at target/release/granit
./target/release/granit --help
```

> `cargo install --path .` places the binary in `~/.cargo/bin`, which the
> standard rustup setup adds to your `PATH`. Granit shells out to `nix`; if `nix`
> is installed but not on your `PATH` (e.g. a multi-user install), Granit still
> finds it via the standard profile location, so `build`/`update`/`doctor` work
> regardless.

## Quick start

```sh
granit init      # scaffold granit.toml + packages/hello
granit doctor    # check your nix environment
granit graph     # print the dependency graph and build order
granit build     # build all packages via nix
```

## Commands

| Command                | Description                                                             |
| ---------------------- | ----------------------------------------------------------------------- |
| `granit init`          | Scaffold a new workspace in the current directory.                      |
| `granit build [pkg]`   | Build a package (and its dependencies). Defaults to the package         |
|                        | containing the current directory, or all packages from the root.        |
| `granit build --emit-only [pkg]` | Print the generated `flake.nix` without building.             |
| `granit test [pkg]`    | Run a package's `test` command.                                         |
| `granit run <cmd> [pkg]` | Run a named custom command from a package's `[commands]` table.       |
| `granit graph`         | Print the dependency graph and resolved build order.                    |
| `granit search <query>` | Search the pinned nixpkgs for build-time tools (attribute name, version, description). |
| `granit update`        | Re-resolve pinned inputs and rewrite `granit.lock`.                     |
| `granit doctor`        | Verify the environment (nix installed, flakes enabled).                 |

You can run `build`/`test`/`run` from inside a package directory; granit walks up
to find the workspace root and defaults the target to that package. The root
`granit.lock` is always used.

## Workspace format (`granit.toml`)

```toml
[workspace]
name = "example"
# Member package directories. Globs and explicit paths both work.
members = ["packages/*"]

[nixpkgs]
# A flake reference. A branch is mutable but pinned by granit.lock; use a commit
# SHA (github:NixOS/nixpkgs/<sha>) to pin exactly.
ref = "github:NixOS/nixpkgs/nixpkgs-unstable"

# Optional: extra overlays (e.g. proprietary package sets) composed over
# nixpkgs, in declared order. Each source is a flake ref exposing
# `overlays.default`.
# [[overlays]]
# source = "git+https://example.com/my-nix-overlay"
```

## Package format (`package.toml`)

```toml
[package]
name = "b"
# Build-time tools by nixpkgs attribute name (resolved against nixpkgs + overlays).
# Not sure of the attribute name? Run `granit search node` to find it.
tools = ["coreutils", "nodejs_20"]
# Artifact dependencies on other packages, each as "package:label".
dependencies = ["a:hello"]
# Optional: files/directories to exclude from the build source (see below).
# A plain name is anchored to the package root; use a glob for any depth.
exclude = ["dist", "tsp-output"]

# Labeled outputs: label = "filename produced by the build".
# granit collects each named file into the package's output under the label.
[outputs]
result = "from-a.txt"

[commands]
# Commands run in a Nix-provisioned environment. Produce your declared output
# files by their natural names — granit collects them (you never touch $out).
# `build` and `test` have first-class subcommands (`granit build`, `granit
# test`); any other entry is a custom command run with `granit run <name>`.
build = "cat $GRANIT_DEPENDENCIES/a/hello > from-a.txt && echo 'built by b' >> from-a.txt"
test  = "echo 'self-contained test' > from-a.txt"
# A custom command — invoke with `granit run generate`.
generate = "go generate ./..."
```

`granit build --emit-only [pkg]` prints the generated flake to stdout without
writing it; a normal build writes it to `.granit/flake.nix`.

### Custom commands

Any entry in a package's `[commands]` table can be run by name with
`granit run <name>`. This is how you expose project-specific dev tasks — code
generation, linting, formatting — using the same reproducible, Nix-provisioned
toolchain your builds use (the package's declared `tools` on `PATH`).

```toml
[package]
name = "api"
tools = ["go"]

[commands]
build    = "go build -o server ./cmd/server"
generate = "go generate ./..."
lint     = "gofmt -l . && go vet ./..."
```

```sh
granit run generate     # runs `go generate ./...`
granit run lint         # runs the lint command
granit run generate api # run it in a specific package from the workspace root
```

**Two execution models.** `build` and `test` are the common cases and get
first-class subcommands (`granit build`, `granit test`); every other command is
invoked through `granit run <name>` (which also keeps custom names from
colliding with granit's own subcommands). They run differently:

| Command             | Runs as                        | Working directory        | `[outputs]` |
| ------------------- | ------------------------------ | ------------------------ | ----------- |
| `build`, `test`     | a hermetic Nix derivation      | a sandboxed copy of src  | collected   |
| `granit run <name>` | a command in a `nix develop`   | your **real** source dir | not collected |

So `granit run generate` runs **in place** in your package directory — a task
like `go generate` writes its output back into your working tree, exactly as if
you ran it yourself, but with the pinned toolchain on `PATH`. `build`/`test`
stay sandboxed and hermetic (their declared `[outputs]` are collected into the
Nix store).

In both models, a declared dependency's output is available at
`$GRANIT_DEPENDENCIES/<package>/<label>` (dependencies are always *built* first),
and each command is self-contained — running a command does **not** implicitly
run `build` first.

### Package source files

A package's own files are copied into the build sandbox, so build commands can
read them (e.g. `package.json`, `main.tsp`, source trees). `.git` and `.granit`
are always excluded.

Use `exclude` to keep generated or heavy directories out of the copied source:

```toml
exclude = ["dist", "tsp-output", "*.log"]
```

Exclude patterns are gitignore-like, with one deliberate difference: **a plain
name is anchored to the package root.** So `exclude = ["dist"]` removes only the
package's own `./dist`, not every nested directory called `dist` (which would
otherwise strip, for example, `node_modules/**/dist` and break a `node_modules`
you intend to copy). To match at any depth, use a glob or slash-bearing path:

| Pattern      | Matches                                             |
| ------------ | --------------------------------------------------- |
| `dist`       | `./dist` only (package root)                        |
| `/dist`      | `./dist` only (same as above; explicit anchor)      |
| `**/dist`    | any directory named `dist`, at any depth            |
| `*.log`      | any `.log` file, at any depth                       |
| `build/tmp`  | `./build/tmp` (slash-bearing paths are anchored)    |

### Environment variables inside build commands

- `$GRANIT_DEPENDENCIES/<package>/<label>` — the materialized output of a
  declared dependency. For `dependencies = ["a:hello"]`, the file is available at
  `$GRANIT_DEPENDENCIES/a/hello`.
- Declared `[outputs]` files are collected automatically after the command runs;
  produce them by their natural filename in the working directory.

> **Note:** each command (`build`, `test`, a custom command) runs as a standalone
> Nix derivation phase in a fresh working directory — it does **not** first run
> `build`. Make each command self-contained, and ensure it produces the
> package's declared outputs.

## Reproducibility & the lock file

`granit.lock` (in the workspace root) is granit's owned lock interface. It
records the exact resolved revision and hash of nixpkgs and every overlay. On the
first build granit resolves and writes it; afterwards it is reused as-is. Run
`granit update` to re-resolve. **Commit `granit.lock`**; the generated flake
(written to `.granit/flake.nix` plus its `.granit/flake.lock`) is an
implementation detail and is gitignored.

## Example

See [`examples/basic`](examples/basic): package `a` emits `hello.txt` as its
`hello` output; package `b` depends on `a:hello`, reads it, and produces a
`result` output.

```sh
cd examples/basic
granit graph      # b -> a:hello, order: a, b
granit build b    # builds a then b
```

## Development

```sh
cargo build
cargo test        # unit tests + integration tests (nix-gated e2e test skips if nix is absent)
```

## Roadmap

See [`backlog.md`](backlog.md) for deferred work and ideas.
