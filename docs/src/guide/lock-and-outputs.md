# Lock file & build outputs

## `granit.lock`

`granit.lock` at the workspace root records the exact revision and hash of
nixpkgs and every overlay. The first build resolves and writes it. Later builds
reuse it as-is, so everyone gets identical tools until you choose to update.

```sh
granit update     # re-resolve nixpkgs and overlays, rewrite granit.lock
```

**Commit `granit.lock`.** The generated `flake.nix`, `flake.lock` and the
`.granit/` directory are implementation details. `granit init` adds them to
`.gitignore`.

## Build outputs

After `granit build`, each built package is linked under `.granit/build`:

```text
.granit/build/<package>/
  result -> /nix/store/…-<package>-0.0.0
```

Each declared output label is inside `result`:

```sh
cat .granit/build/api-spec/result/schema
ls  .granit/build/web/result/dist/
```

- Each `result` link is a Nix **garbage-collection root**, so `nix-collect-garbage`
  won't delete your latest builds.
- Building one package only refreshes that package's link. Other packages'
  links are left alone.
- Only `granit build` writes here. `granit run` commands run in place and don't
  collect outputs.

## Seeing the generated flake

To see the Nix that Granit generates without building anything:

```sh
granit build --emit-only
```

This prints the flake for the whole workspace to stdout.
