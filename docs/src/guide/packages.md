# Packages

Each workspace member has a `package.toml`:

```toml
[package]
name = "b"
tools = ["coreutils", "nodejs_20"]
dependencies = ["a:hello"]
exclude = ["dist"]

[outputs]
result = "from-a.txt"

[commands]
build = "cat $GRANIT_DEPENDENCIES/a/hello > from-a.txt"
test  = "cat $GRANIT_DEPENDENCIES/a/hello > from-a.txt && test -s from-a.txt"
```

## `[package]`

| Key            | Description |
| -------------- | ----------- |
| `name`         | The package name. Other packages refer to it by this name, and CLI commands accept it. |
| `tools`        | Build-time tools, as nixpkgs attribute names. They're on `PATH` for every command. |
| `dependencies` | Outputs of other packages this one needs, each written `"package:label"`. |
| `exclude`      | Files or directories to leave out of the build sandbox. See [Package source files](source-files.md). |

### Finding tool names

Tools are nixpkgs **attribute names**, which aren't always the same as the
program name. Search the workspace's pinned nixpkgs:

```sh
granit search python
```

This prints each matching attribute name with its version and description.
Some common ones:

| You want          | Attribute name                  |
| ----------------- | ------------------------------- |
| Go                | `go`                            |
| Node.js 20        | `nodejs_20`                     |
| Python 3          | `python3`                       |
| Java 21           | `jdk21`                         |
| Rust              | `cargo`, `rustc`                |
| GNU coreutils     | `coreutils`                     |
| C compiler        | `gcc`                           |

## `[outputs]`

The things this package produces, each under a label. Other packages depend on
them as `"<package>:<label>"`. See
[Outputs & dependencies](outputs-and-dependencies.md).

## `[commands]`

Shell commands that run with the package's tools on `PATH`. `build` and `test`
get their own CLI subcommands; anything else runs with `granit run <name>`. See
[Commands & hooks](commands.md).
