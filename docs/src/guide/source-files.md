# Package source files

`build` and `test` run on a copy of the package directory in the Nix sandbox,
so commands can read `package.json`, `go.mod`, source trees and so on. `.git`
and `.granit` are always left out.

Use `exclude` to keep generated or bulky files out of the copy:

```toml
[package]
name = "web"
exclude = ["dist", "coverage", "*.log"]
```

## Pattern rules

The patterns work like `.gitignore`, with one deliberate difference: **a plain
name only matches at the package root.** `exclude = ["dist"]` removes
`./dist`, but leaves directories called `dist` deeper in the tree alone. That
matters for things like `node_modules/**/dist`, which you usually want to keep.

| Pattern     | Matches                                       |
| ----------- | --------------------------------------------- |
| `dist`      | `./dist` only                                 |
| `/dist`     | `./dist` only (explicitly anchored)           |
| `**/dist`   | any directory named `dist`, at any depth      |
| `*.log`     | any `.log` file, at any depth                 |
| `build/tmp` | `./build/tmp` (paths with a slash are anchored) |
