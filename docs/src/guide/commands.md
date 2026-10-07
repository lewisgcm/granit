# Commands & hooks

A package's `[commands]` table maps names to shell commands. Each command runs
with the package's `tools` on `PATH` and its dependencies' outputs under
`$GRANIT_DEPENDENCIES`.

```toml
[commands]
build    = "go build -o bin/server ./cmd/server"
test     = "go test ./... && go build -o bin/server ./cmd/server"
generate = "go generate ./..."
lint     = "gofmt -l . && go vet ./..."
```

```sh
granit build            # run `build`
granit test             # run `test`
granit run generate     # run any other command by name
granit run lint api     # ...in a specific package, from anywhere in the workspace
```

## Two ways commands run

| Command             | Runs as                         | Working directory        | `[outputs]`   |
| ------------------- | ------------------------------- | ------------------------ | ------------- |
| `build`, `test`     | a sandboxed Nix build           | a sandboxed copy of the source | collected     |
| `granit run <name>` | a command in `nix develop`      | your **real** package directory | not collected |

**`build` and `test` are hermetic.** They run in the Nix sandbox on a copy of
the package source. Write your declared outputs by their normal file names, and
Granit collects them. You never touch `$out`.

**`granit run` runs in place.** The command runs in your actual package
directory with the pinned tools, so things like `go generate`, `npm install` or a
formatter write their changes back into your working tree.

> **Each command is self-contained.** `test` doesn't run `build` first, and each
> sandboxed command starts in a fresh directory. A `test` command has to produce
> the package's declared outputs too, which is why the examples run the build
> step inside `test`.

### Network access in the sandbox

`build` and `test` run in the Nix sandbox, which (on Linux, by default) has **no
network access**. Commands that download dependencies, like `npm ci`, `go mod
download` or `pip install`, won't work inside `build`. Fetch them in place with
a `needs` hook instead (below), or vendor them into the repository. The
[Node example](../examples/node-app.md) shows the pattern.

## Command hooks: `needs`

A command can list other commands from the same package that must run first.
Write it as a table, `{ run = "...", needs = [...] }`, instead of a plain
string:

```toml
[commands]
# Still runnable on its own with `granit run generate`.
generate = "go generate ./..."

# `build` runs `generate` first, then compiles.
build = { run = "go build -o bin/app .", needs = ["generate"] }

test = "go test ./... && go build -o bin/app ."
```

```sh
granit build          # runs generate, then build
granit run generate   # runs only generate
```

How hooks behave:

- Hooks run **in place**, in your real package directory, like `granit run`.
  Whatever they write, such as generated code or `node_modules`, is then part of
  the source that `build` copies into the sandbox.
- They run dependency-first, each one once. Hooks can chain (`build` →
  `generate` → `proto`). Cycles are rejected.
- `needs` can only refer to commands in the **same package**.
- Because hooks change your working tree, a `build` with `needs` isn't fully
  hermetic. That's a deliberate trade-off for development.

## Environment inside commands

| Variable                                  | Meaning                                                           |
| ----------------------------------------- | ----------------------------------------------------------------- |
| `$GRANIT_DEPENDENCIES/<package>/<label>`  | The output of a declared artifact dependency.                     |
| `$HOME`, `$XDG_CACHE_HOME`                | Writable directories in the sandbox, so tools that cache under `$HOME` (npm, pip, cargo, gradle) work. |
