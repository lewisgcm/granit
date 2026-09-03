# Granit Backlog

Deferred work and ideas, grouped by theme. Items here were intentionally left
out of the initial version to keep scope focused; each is a candidate for a
future iteration.

## Build orchestration
- [ ] **Hermetic `build` mode (e.g. `granit build --release`).** Command hooks
  (`needs`) run in-place in the real source tree (dev-style), which means a
  `build` with `needs` mutates your working tree and is therefore **not
  hermetic**. Add an opt-in fully-hermetic build (working name
  `granit build --release`) that runs any `needs` steps *inside* the sandbox
  (or requires the tree to already be generated) and guarantees no working-tree
  mutation. Track and, where possible, warn about the non-hermetic aspects a
  normal `build` introduces (in-place hooks, source-tree writes).
- [ ] **Cross-model / cross-package command hooks.** `needs` currently chains
  commands within a single package and runs them in-place. Consider hooks that
  depend on commands in *other* packages, and finer control over which
  execution model a hook uses (in-place vs sandboxed).
- [ ] **Per-package, granit-driven build orchestration.** Today granit emits a
  single `flake.nix` and lets Nix resolve the whole graph in one `nix build`.
  An alternative is for granit to topologically drive `nix build` per package,
  passing dependency outputs explicitly. This would enable finer progress
  reporting, partial rebuilds, and per-package caching policies.
- [ ] **Parallelism / caching controls.** Expose flags for `nix build`
  parallelism (`--max-jobs`, `--cores`) and remote/binary cache configuration.
- [ ] **`build`/`test` chaining.** Optionally run a package's `build` before its
  `test` so test commands can rely on build outputs (currently each command is
  a standalone derivation phase and must be self-contained).
- [ ] **Multi-system / cross builds.** Granit currently generates the flake for
  exactly the host system (`builtins.currentSystem`). Add an opt-in to target
  other systems (e.g. `granit build --system x86_64-linux`) or to emit packages
  for a configured set of systems.

## Dependency & overlay model
- [ ] **Package-level overlays.** Overlays are workspace-level only. Allow a
  package to declare additional overlays scoped to itself.
- [ ] **Logical tool names.** Map friendly names (e.g. `node@20`) to nixpkgs
  attributes via an internal mapping, instead of requiring raw attribute names.
- [ ] **Unified `inputs` option.** Explore an alternative model where tools and
  workspace packages share one namespace (pure-overlay model), for users who
  prefer it.
- [ ] **Non-github overlay pinning ergonomics.** Improve `locked_ref`
  synthesis for non-github flake refs (git+https, path, tarball).

## Artifacts & outputs
- [ ] **Package source files as build inputs.** Generated derivations set
  `dontUnpack = true` and have no `src`, so a package build can use tools and
  consume declared dependency outputs, but cannot yet read its own source files
  from the package directory. Add a way to bring package sources into the build
  (e.g. copy the package dir into the sandbox, or a declared `src`/`files`
  field), pinned for reproducibility.
- [ ] **Post-build artifact verification.** Currently granit validates that
  referenced outputs are *declared*, but does not verify the build actually
  produced each declared file before collection (the `cp` in the install phase
  fails if missing). Add an explicit, friendlier check with a clear error.
- [ ] **Directory outputs / globs.** Allow an output label to map to a directory
  or a glob of files, not just a single filename.

## Diagnostics & UX
- [ ] **Rich TOML diagnostics.** Surface line/column and field context on parse
  errors (e.g. via `toml`'s spanned errors).
- [ ] **Prettier cycle reports.** Render dependency cycles as a diagram and
  suggest which edge to remove.
- [ ] **`doctor` auto-enable option.** Optionally have granit pass
  `--extra-experimental-features` (already done for granit's own invocations)
  and offer to write it to the user's `nix.conf`.
- [ ] **`granit clean`.** Remove generated build artifacts — the `.granit/`
  directory (generated `flake.nix`/`flake.lock`) and any `result` symlinks.
- [ ] **Output abstraction for CLI messaging.** Progress/status output uses
  `println!` directly across modules. Route user-facing output through a small
  abstraction to support `--quiet`, `--verbose`, JSON output, and stderr vs
  stdout separation.

## Internal quality
- [ ] **Cache `nix` binary resolution.** `SystemRunner::resolve_program`
  re-scans `PATH` and the fallback profile dirs on every invocation. Resolve
  once and cache it (minor; only matters for graphs with many invocations).
- [ ] **Broaden integration coverage.** The nix-gated e2e test covers the happy
  path; add gated cases for overlays, custom `run` commands, and build failures.

## Ecosystem
- [ ] **Language recipes.** Optional higher-level helpers for common ecosystems
  (Rust/Cargo, Node, Go) layered on top of the generic command model.
- [ ] **`granit.lock` schema evolution.** Version negotiation and migration when
  the lock format changes.
- [ ] **CI recipes.** Documented patterns for using granit in CI with a shared
  Nix binary cache.
