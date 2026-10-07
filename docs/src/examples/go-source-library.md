# Go shared source library

Two Go modules in one repository: `common` is a library, and `app` imports it.
Go compiles libraries from source, so `common` exposes a **source** output and
`app` finds it through an ordinary `replace` directive. Your editor and plain
`go build` work exactly as they do without Granit.

```text
.
├── granit.toml
└── packages/
    ├── common/
    │   ├── package.toml
    │   ├── go.mod
    │   └── greet.go
    └── app/
        ├── package.toml
        ├── go.mod
        └── main.go
```

## The library

```toml
# packages/common/package.toml
[package]
name = "common"
tools = ["go"]

[outputs]
src = { source = "." }

[commands]
test = "go test ./..."
```

```go
// packages/common/go.mod
module example.com/common

go 1.22
```

```go
// packages/common/greet.go
package common

func Greet(name string) string { return "hello, " + name }
```

## The application

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
test  = "go test ./... && go build -o bin/app ."
fmt   = "gofmt -w ."
```

```go
// packages/app/go.mod
module example.com/app

go 1.22

require example.com/common v0.0.0
replace example.com/common => ../common
```

```go
// packages/app/main.go
package main

import (
	"fmt"

	"example.com/common"
)

func main() { fmt.Println(common.Greet("granit")) }
```

## Build it

```sh
$ granit build app
$ .granit/build/app/result/app
hello, granit

$ granit run fmt app     # gofmt in place, using the pinned Go
```

Granit doesn't build `common` on its own for this. It copies `common`'s source
into `app`'s sandbox at `../common`, the same relative path as in your
checkout, so the `replace` directive resolves.

> This example has no third-party Go modules. If yours does, the sandbox can't
> download them. Add `vendor = "go mod vendor"` and make `build` depend on it with
> `build = { run = "go build -o bin/app .", needs = ["vendor"] }`, or commit
> `vendor/`. See [network access](../guide/commands.md#network-access-in-the-sandbox).
