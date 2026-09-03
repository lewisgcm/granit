# Granit

A simple monorepo build tool that wraps Nix flakes.

Granit lets you define a workspace and its packages in TOML. You declare
build-time tools by name (pulled from nixpkgs), label the outputs each package
produces (a built *artifact* or its *source* tree), and declare dependencies
between packages. Granit generates a `flake.nix`, manages a `granit.lock` for
reproducibility, and shells out to `nix build` — wrapping Nix's complexity
behind a familiar, ergonomic format.

## Goals

1. **Simplified interface over nixpkgs.** Bring in build-time tools (java, node,
   gcc, …) by attribute name and get reproducible builds without writing Nix.
2. **Dependencies between packages.** Package A produces something (a built
   *artifact*, or its *source* tree); package B depends on it. Granit builds A
   first when needed and mounts its output into B the right way — artifacts at
   `$GRANIT_DEPENDENCIES`, source repo-relative for compile-together libraries.
3. **Familiar syntax.** Concepts you already know: workspaces, dependencies,
   and labeled outputs (artifacts or shared source).

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

Any command can declare other commands to run first via
[`needs` hooks](#command-hooks-needs), and after a build each package's output
is linked under [`.granit/build/<pkg>/result`](#build-outputs).

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

# Labeled outputs. A bare string is an *artifact* (a built file/dir collected
# under the label). A `{ source = "." }` output exposes the package's source
# tree to consumers as compile-time source (see "Outputs & dependency kinds").
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
writing it; a normal build writes it to `flake.nix` at the workspace root.

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
run `build` first (see command hooks below to opt into ordering).

#### Command hooks (`needs`)

A command can declare other commands (in the same package) that must run first.
Use the tagged form of a command — `{ run = "...", needs = ["other", ...] }` —
instead of a bare string. This is the ergonomic way to say "generate before you
build" while keeping `generate` independently runnable for local dev:

```toml
[commands]
# A standalone dev task — still runnable on its own with `granit run generate`.
generate = "go generate ./..."

# `build` runs `generate` first, then compiles.
build = { run = "go build -o bin/app .", needs = ["generate"] }

test = "go test ./..."   # a bare string still works (no hooks)
```

```sh
granit build            # runs `generate`, then builds
granit run generate     # runs just `generate` (unchanged)
```

Semantics:

- **`needs` hooks run in-place** (the `nix develop` model), in your real source
  directory, in dependency-first order, each once. So a `generate` hook writes
  its output back into your working tree, and the subsequent `build` compiles
  the updated source. Hooks chain transitively (`build` → `generate` →
  `proto`), and cycles are rejected.
- Because hooks run in-place, **a `build` with `needs` mutates your working
  tree and is therefore not fully hermetic.** That is the intended dev-time
  tradeoff. A future fully-hermetic build mode (e.g. `granit build --release`)
  is tracked in [`backlog.md`](backlog.md).
- `needs` may only reference commands defined in the **same package**; granit
  validates the references and errors clearly if one is missing.

### Outputs & dependency kinds

An `[outputs]` entry declares something a package produces, under a label.
There are two *kinds*, and the **producer** decides which — a consumer just
depends on `package:label` and granit mounts it the right way.

```toml
[outputs]
# Artifact (the default): a built file or directory. A bare string is shorthand
# for `{ artifact = "..." }`.
schema  = "tsp-output/schema/openapi.yaml"
bundle  = { artifact = "dist/lib.js" }

# Source: the package's source tree, exposed to consumers as compile-time
# source. The path is usually ".".
src     = { source = "." }
```

How each kind is mounted into a consumer that depends on it:

| Output kind         | Consumer sees it at                         | Producer is… | Typical use |
| ------------------- | ------------------------------------------- | ------------ | ----------- |
| `artifact` (or bare string) | `$GRANIT_DEPENDENCIES/<pkg>/<label>` | built first  | a compiled binary, a generated schema, a `dist/` bundle, a `.jar` |
| `source`            | mounted **repo-relative** as a sibling dir  | not built    | a shared **source** library compiled *with* the consumer |

A **source dependency** is the answer to "package B needs package A's *source*
to compile against" — a Go/Rust/Python/plain-JS shared library. granit copies
A's source tree into B's build at A's real repo-relative location (e.g.
`../common`), so B's own language tooling resolves it by path exactly as it does
in your working tree:

```toml
# packages/common/package.toml  — a Go source library
[package]
name = "common"
tools = ["go"]
[outputs]
src = { source = "." }          # "my output is my source"

# packages/app/package.toml  — depends on common's SOURCE
[package]
name = "app"
tools = ["go"]
dependencies = ["common:src"]   # one list; granit mounts by kind
[outputs]
app = { artifact = "bin/app" }
[commands]
build = "go build -o bin/app ."
```

```go
// packages/app/go.mod keeps the ordinary monorepo config — same for your IDE:
//   require example.com/common v0.0.0
//   replace example.com/common => ../common
```

For **build-required** libraries (TypeScript compiled to `dist/`, a Java
`.jar`), use an `artifact` output instead: the producer is built first and the
consumer reads the built result from `$GRANIT_DEPENDENCIES`.

**Which kind for which language?** The rule of thumb: if the consumer's compiler
links the library *from source*, use `source`; if it links a *built* artifact,
use `artifact`.

| Ecosystem                                   | Shared library as… | Consumer wiring                                   |
| ------------------------------------------- | ------------------ | ------------------------------------------------- |
| Go                                          | `source`           | `replace mymod => ../common` (or `go.work`)       |
| Rust                                        | `source`           | `common = { path = "../common" }`                 |
| Python (pure)                               | `source`           | editable/path dep, or `PYTHONPATH`                |
| Node (plain JS)                             | `source`           | `"common": "file:../common"` or workspaces        |
| Node (TypeScript compiled to `dist/`)       | `artifact`         | depend on `common:dist`, read `$GRANIT_DEPENDENCIES` |
| Java / Kotlin / .NET                        | `artifact`         | depend on `common:jar`, put it on the classpath   |

Source dependencies work the same in a hermetic `granit build` (granit copies
the sibling source into the sandbox at its repo-relative path) and in
`granit run <cmd>` dev commands (which run in your real tree, where the sibling
already exists). Either way your language's own path-based resolution does the
linking — granit just makes sure the source is where that resolution expects it.

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
(`flake.nix`/`flake.lock` at the workspace root) and the `.granit/` working
directory are implementation details and are gitignored.

## Build outputs

After `granit build`, each built package's output is linked under a stable,
discoverable tree at the workspace root:

```
.granit/build/<package>/
  result       -> /nix/store/…-<package>-0.0.0   # GC root (survives nix-collect-garbage)
```

Each declared `[outputs]` label lives inside, at `result/<label>` — so
`.granit/build/api-spec/result/schema.yml` points straight at the built schema.
The per-package `result` link is a Nix **GC root**, so builds you've made won't
be removed by `nix-collect-garbage`. The tree is refreshed per built package —
building one package leaves other packages' links untouched — and `.granit/` is
gitignored. Only `granit build` populates it (custom `granit run` commands run
in-place and collect no outputs).

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
