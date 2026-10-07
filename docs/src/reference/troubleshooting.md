# Troubleshooting

Start with:

```sh
granit doctor
```

It checks that `nix` can be found and that flakes work.

## "nix: command not found" after installing Nix

Open a new shell, or source the Nix profile. Granit itself also looks in
`/nix/var/nix/profiles/default/bin`, so `granit build` usually works anyway.

## A tool isn't found

Tools are nixpkgs **attribute names**, not program names. Use
`granit search <name>` to find the right one, for example `nodejs_20` rather
than `node`. If you need something newer than your pinned nixpkgs, run
`granit update` or point `[nixpkgs].ref` at a newer branch.

## The build can't download dependencies

`build` and `test` run in the Nix sandbox without network access. Fetch
dependencies in place with a [`needs` hook](../guide/commands.md#command-hooks-needs)
(`npm ci`, `go mod vendor`), or commit them to the repository.

## A declared output is missing

Each sandboxed command starts in a fresh directory, and `test` doesn't run
`build` first. Make sure the command that's running (including `test`) writes
every file listed in `[outputs]`.

## A tool fails writing to `$HOME`

Granit gives sandboxed builds a writable `$HOME` and `$XDG_CACHE_HOME`. If a
tool still writes somewhere else, point it at `$HOME` with an environment
variable in the command, for example `GRADLE_USER_HOME=$HOME/.gradle gradle build`.

## Seeing what Granit runs

```sh
granit build --emit-only
```

This prints the generated flake, so you can see exactly how each package is
built.
