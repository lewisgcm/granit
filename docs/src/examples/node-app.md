# Node app with installed dependencies

`npm ci` needs the network, which the build sandbox doesn't have. The fix is a
`needs` hook. The hook runs `npm ci` **in place** in your package directory with
the pinned Node.js, and then `build` copies the resulting `node_modules` into
the sandbox along with the rest of the source.

```toml
# packages/web/package.toml
[package]
name = "web"
tools = ["nodejs_20"]
# Keep the previous build's output out of the sandbox copy.
exclude = ["dist"]

[outputs]
dist = "dist"

[commands]
install = "npm ci"
build   = { run = "npm run build", needs = ["install"] }
test    = { run = "npm test && npm run build", needs = ["install"] }
dev     = "npm run dev"
lint    = "npx eslint ."
```

```sh
granit build web       # npm ci (in place), then npm run build (sandboxed)
granit test web        # npm ci, then the tests
granit run dev web     # start the dev server with the pinned Node
ls .granit/build/web/result/dist
```

Notes:

- `dist` is an **artifact** output that points at a directory. Consumers see it
  at `$GRANIT_DEPENDENCIES/web/dist`.
- `exclude = ["dist"]` only matches `./dist` at the package root, so
  `node_modules/**/dist` folders are still copied. See
  [pattern rules](../guide/source-files.md#pattern-rules).
- The `install` hook changes your working tree, so this build isn't fully
  hermetic. Your `package-lock.json` still pins dependency versions.

## Sharing a TypeScript library

If `ui-kit` compiles to `dist/`, expose that as an artifact and have the app copy
it in before building:

```toml
# packages/ui-kit/package.toml
[package]
name = "ui-kit"
tools = ["nodejs_20"]
exclude = ["dist"]

[outputs]
dist = "dist"

[commands]
install = "npm ci"
build   = { run = "npm run build", needs = ["install"] }
```

```toml
# packages/web/package.toml
[package]
name = "web"
tools = ["nodejs_20"]
dependencies = ["ui-kit:dist"]

[outputs]
dist = "dist"

[commands]
install = "npm ci"
build   = { run = "mkdir -p vendor && cp -r $GRANIT_DEPENDENCIES/ui-kit/dist vendor/ui-kit && npm run build", needs = ["install"] }
```
