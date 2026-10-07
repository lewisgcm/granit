# GitHub Actions

Running Granit in CI takes three steps: install Nix, install the `granit`
binary, and run `granit build` / `granit test`. Because `granit.lock` pins
nixpkgs, CI uses exactly the same tools as your laptop.

## Basic workflow

```yaml
# .github/workflows/build.yml
name: Build

on:
  push:
    branches: [main]
  pull_request:

env:
  GRANIT_VERSION: v0.1.0

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Nix
        uses: cachix/install-nix-action@v31

      - name: Install granit
        run: |
          curl -fsSL "https://github.com/lewisgcm/granit/releases/download/${GRANIT_VERSION}/granit-x86_64-unknown-linux-musl.tar.gz" \
            | sudo tar -xz -C /usr/local/bin granit
          granit --version
          granit doctor

      - name: Build
        run: granit build

      - name: Test
        run: granit test
```

`cachix/install-nix-action` enables flakes by default. Granit turns on the flake
features for its own calls either way.

Pin `GRANIT_VERSION` instead of using `latest`, so a new Granit release can't
change your builds without a commit.

## Choosing the binary per runner

If you run on several runner types, pick the matching binary:

```yaml
      - name: Install granit
        run: |
          case "${RUNNER_OS}-${RUNNER_ARCH}" in
            Linux-X64)   target=x86_64-unknown-linux-musl ;;
            Linux-ARM64) target=aarch64-unknown-linux-musl ;;
            macOS-X64)   target=x86_64-apple-darwin ;;
            macOS-ARM64) target=aarch64-apple-darwin ;;
          esac
          mkdir -p "$HOME/.local/bin"
          curl -fsSL "https://github.com/lewisgcm/granit/releases/download/${GRANIT_VERSION}/granit-${target}.tar.gz" \
            | tar -xz -C "$HOME/.local/bin" granit
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
```

## Building packages in parallel

Use a matrix with one job per package. Each job builds only that package and
its dependencies:

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        package: [api-spec, server, web]
    steps:
      - uses: actions/checkout@v4
      - uses: cachix/install-nix-action@v31
      - name: Install granit
        run: |
          curl -fsSL "https://github.com/lewisgcm/granit/releases/download/${GRANIT_VERSION}/granit-x86_64-unknown-linux-musl.tar.gz" \
            | sudo tar -xz -C /usr/local/bin granit
      - run: granit build ${{ matrix.package }}
      - run: granit test ${{ matrix.package }}
```

## Uploading build outputs

Built outputs are symlinks into the Nix store under `.granit/build/<package>/result`.
Copy them out with `cp -rL` (which follows symlinks) before uploading:

```yaml
      - name: Build server
        run: granit build server

      - name: Collect outputs
        run: |
          mkdir -p out
          cp -rL .granit/build/server/result/. out/

      - uses: actions/upload-artifact@v4
        with:
          name: server
          path: out/
```

## Caching Nix builds

Without a cache, every CI run downloads the toolchains and rebuilds every
package from scratch. A binary cache such as [Cachix](https://www.cachix.org)
stores build results, so unchanged packages are downloaded instead of rebuilt:

```yaml
      - uses: cachix/install-nix-action@v31

      - uses: cachix/cachix-action@v15
        with:
          name: my-cache                      # your Cachix cache name
          authToken: ${{ secrets.CACHIX_AUTH_TOKEN }}

      # ... install granit, then build as usual. Results are pushed to the
      # cache at the end of the job.
```

Granit generates one derivation per package, so a package is only rebuilt when
its source, tools or dependencies change.

## Keeping the lock file fresh

To update nixpkgs on a schedule, run `granit update` and open a pull request:

```yaml
# .github/workflows/update-lock.yml
name: Update granit.lock

on:
  schedule:
    - cron: "0 6 * * 1"     # Mondays 06:00 UTC
  workflow_dispatch:

permissions:
  contents: write
  pull-requests: write

env:
  GRANIT_VERSION: v0.1.0

jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: cachix/install-nix-action@v31
      - name: Install granit
        run: |
          curl -fsSL "https://github.com/lewisgcm/granit/releases/download/${GRANIT_VERSION}/granit-x86_64-unknown-linux-musl.tar.gz" \
            | sudo tar -xz -C /usr/local/bin granit
      - run: granit update
      - uses: peter-evans/create-pull-request@v7
        with:
          branch: granit-lock-update
          title: Update granit.lock
          commit-message: Update granit.lock
```

The pull request's own CI run checks that everything still builds with the new
pins.
