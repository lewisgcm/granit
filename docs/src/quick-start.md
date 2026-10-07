# Quick start

Create a workspace in an empty directory:

```sh
mkdir my-repo && cd my-repo
granit init
```

This creates:

```text
my-repo/
├── .gitignore
├── granit.toml               # the workspace manifest
└── packages/
    └── hello/
        └── package.toml      # a sample package
```

The sample package is a complete, if tiny, build:

```toml
[package]
name = "hello"
tools = ["coreutils"]
dependencies = []

[outputs]
message = "message.txt"

[commands]
build = "echo 'hello from granit' > message.txt"
test = "echo 'hello from granit' > message.txt && test -s message.txt"
```

Now try the main commands:

```sh
granit doctor    # check that nix and flakes work
granit graph     # print the dependency graph and build order
granit build     # build every package
granit test hello  # run the hello package's test command
```

After `granit build`, the output is linked into your workspace:

```sh
cat .granit/build/hello/result/message
# hello from granit
```

The first build resolves nixpkgs and writes `granit.lock`. **Commit it.** It
pins the exact nixpkgs revision so every machine builds with the same tools.

## Adding a real tool

Need a tool you don't have installed? Look up its nixpkgs attribute name:

```sh
granit search node
```

Then add it to the package's `tools`:

```toml
[package]
name = "hello"
tools = ["nodejs_20"]

[outputs]
message = "message.txt"

[commands]
build = "node -e 'require(\"fs\").writeFileSync(\"message.txt\", process.version)'"
```

```sh
granit build && cat .granit/build/hello/result/message
```

## Next steps

- Learn how [workspaces](guide/workspaces.md) and [packages](guide/packages.md)
  are laid out.
- Connect packages with [outputs & dependencies](guide/outputs-and-dependencies.md).
- Browse the [examples](examples/artifact-dependency.md).
- Run Granit in [GitHub Actions](ci/github-actions.md).
