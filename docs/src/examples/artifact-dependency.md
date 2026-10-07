# Artifact dependency

The smallest useful workspace: package `a` produces a file, and package `b`
consumes it. This is [`examples/basic`](https://github.com/lewisgcm/granit/tree/main/examples/basic)
in the repository.

```text
.
├── granit.toml
└── packages/
    ├── a/package.toml
    └── b/package.toml
```

```toml
# granit.toml
[workspace]
name = "example"
members = ["packages/*"]

[nixpkgs]
ref = "github:NixOS/nixpkgs/nixos-24.05"
```

```toml
# packages/a/package.toml
[package]
name = "a"
tools = ["coreutils"]

[commands]
build = "echo 'hello world' > hello.txt"

[outputs]
hello = "hello.txt"
```

```toml
# packages/b/package.toml
[package]
name = "b"
tools = ["coreutils"]
dependencies = ["a:hello"]

[commands]
build = "cat $GRANIT_DEPENDENCIES/a/hello > from-a.txt && echo 'built by b' >> from-a.txt"

[outputs]
result = "from-a.txt"
```

```sh
$ granit graph        # b depends on a:hello; build order: a, b
$ granit build b      # builds a first, then b
$ cat .granit/build/b/result/result
hello world
built by b
```

What's going on:

1. `a` declares an artifact output `hello`, backed by the file `hello.txt` that
   its `build` command writes.
2. `b` depends on `a:hello`, so Granit builds `a` first.
3. Inside `b`'s build, `a`'s artifact is at `$GRANIT_DEPENDENCIES/a/hello`.
4. `b`'s own output, `result`, is collected from `from-a.txt`.
