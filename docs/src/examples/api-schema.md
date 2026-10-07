# Generated API schema

A common monorepo pattern: one package owns an API definition and builds a
single schema file from it. Other packages depend on that file to generate code.
Change the schema, and `granit build server` rebuilds the schema before the
server.

```text
packages/
├── api-spec/
│   ├── package.toml
│   └── spec/
│       ├── base.yaml
│       ├── users.yaml
│       └── orders.yaml
└── server/
    ├── package.toml
    ├── go.mod
    └── main.go
```

## The schema producer

```toml
# packages/api-spec/package.toml
[package]
name = "api-spec"
tools = ["yq-go"]

[outputs]
schema = "openapi.yaml"

[commands]
# Merge the split spec files into one OpenAPI document.
build = "yq eval-all '. as $item ireduce ({}; . * $item)' spec/*.yaml > openapi.yaml"
test  = "yq eval-all '. as $item ireduce ({}; . * $item)' spec/*.yaml > openapi.yaml && yq -e '.openapi' openapi.yaml"
```

## The consumer

```toml
# packages/server/package.toml
[package]
name = "server"
tools = ["go", "oapi-codegen"]
dependencies = ["api-spec:schema"]

[outputs]
server = "bin/server"

[commands]
# Fetch Go modules in place; the sandbox has no network.
vendor = "go mod vendor"

# Generate types from the built schema, then compile.
build = { run = "oapi-codegen -generate types -package api -o api/types.gen.go $GRANIT_DEPENDENCIES/api-spec/schema && go build -o bin/server .", needs = ["vendor"] }
```

```sh
granit build server
cat .granit/build/api-spec/result/schema     # the merged schema
.granit/build/server/result/server
```

Because `server` depends on `api-spec:schema`, Granit always builds the schema
first and copies the result to `$GRANIT_DEPENDENCIES/api-spec/schema`. You can
add any number of consumers (a TypeScript client, docs) that depend on the same
output.
