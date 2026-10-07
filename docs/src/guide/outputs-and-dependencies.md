# Outputs & dependencies

An `[outputs]` entry declares something a package produces, under a label. There
are two kinds, and **the producer chooses the kind**. A consumer just depends on
`package:label`, and Granit makes the output available the right way.

```toml
[outputs]
# Artifact (the default): a built file or directory. A bare string is
# shorthand for `{ artifact = "..." }`.
schema = "tsp-output/schema/openapi.yaml"
bundle = { artifact = "dist" }

# Source: the package's source tree, compiled together with the consumer.
src = { source = "." }
```

## Artifact outputs

An artifact is a file or directory that the producer's `build` command creates.
Granit **builds the producer first**, then copies the artifact into the
consumer's build at:

```text
$GRANIT_DEPENDENCIES/<package>/<label>
```

```toml
# packages/a/package.toml
[package]
name = "a"
tools = ["coreutils"]

[outputs]
hello = "hello.txt"

[commands]
build = "echo 'hello world' > hello.txt"
```

```toml
# packages/b/package.toml
[package]
name = "b"
tools = ["coreutils"]
dependencies = ["a:hello"]

[outputs]
result = "from-a.txt"

[commands]
build = "cat $GRANIT_DEPENDENCIES/a/hello > from-a.txt"
```

Use artifacts for anything that has to be built before it can be used, such as
binaries, generated schemas, a TypeScript `dist/` folder or a `.jar`.

## Source outputs

A source output exposes the package's **source tree**. The producer isn't
built. Instead, Granit copies its source into the consumer's build at the same
**repo-relative path** it has in your checkout (for example `../common`). The
consumer's own language tooling then finds it by path, exactly as it does on
your machine:

```toml
# packages/common/package.toml
[package]
name = "common"
tools = ["go"]

[outputs]
src = { source = "." }
```

```toml
# packages/app/package.toml
[package]
name = "app"
tools = ["go"]
dependencies = ["common:src"]

[outputs]
app = "bin/app"

[commands]
build = "go build -o bin/app ."
```

See the full [Go shared library example](../examples/go-source-library.md).

## Which kind should I use?

The rule of thumb: if the consumer's compiler links the library **from
source**, use `source`. If it links a **built** artifact, use `artifact`.

| Ecosystem                             | Shared library as | Consumer wiring                                      |
| ------------------------------------- | ----------------- | ---------------------------------------------------- |
| Go                                    | `source`          | `replace mymod => ../common` (or `go.work`)          |
| Rust                                  | `source`          | `common = { path = "../common" }`                    |
| Python (pure)                         | `source`          | path dependency, or `PYTHONPATH`                     |
| Node (plain JS)                       | `source`          | `"common": "file:../common"` or workspaces           |
| Node (TypeScript compiled to `dist/`) | `artifact`        | depend on `common:dist`, read `$GRANIT_DEPENDENCIES` |
| Java / Kotlin / .NET                  | `artifact`        | depend on `common:jar`, put it on the classpath      |

## Inspecting the graph

```sh
granit graph
```

This prints each package's dependencies and the resolved build order. Granit
rejects dependency cycles and references to undeclared outputs.
