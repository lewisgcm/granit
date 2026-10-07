# Installation

## Requirements

Granit shells out to [Nix](https://nixos.org/download). Install it first:

```sh
# Official multi-user installer (Linux and macOS)
sh <(curl -L https://nixos.org/nix/install) --daemon
```

Granit uses the `nix-command` and `flakes` features. It turns them on for its own
`nix` calls, so you don't need to change your Nix configuration. If `nix` isn't
on your `PATH` (common right after a multi-user install), Granit still finds it in
the standard profile location, `/nix/var/nix/profiles/default/bin`.

## Pre-built binaries

Every [GitHub release](https://github.com/lewisgcm/granit/releases) includes
binaries for these platforms:

| Platform              | Target                       |
| --------------------- | ---------------------------- |
| Linux x86_64          | `x86_64-unknown-linux-musl`  |
| Linux arm64           | `aarch64-unknown-linux-musl` |
| macOS Intel           | `x86_64-apple-darwin`        |
| macOS Apple Silicon   | `aarch64-apple-darwin`       |

The Linux binaries are statically linked and run on any distribution.

Install the latest release:

```sh
target=aarch64-apple-darwin   # pick from the table above
curl -fsSL "https://github.com/lewisgcm/granit/releases/latest/download/granit-$target.tar.gz" \
  | sudo tar -xz -C /usr/local/bin granit
```

Or detect your platform automatically:

```sh
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)          target=x86_64-unknown-linux-musl ;;
  Linux-aarch64)         target=aarch64-unknown-linux-musl ;;
  Darwin-x86_64)         target=x86_64-apple-darwin ;;
  Darwin-arm64)          target=aarch64-apple-darwin ;;
  *) echo "unsupported platform" >&2; exit 1 ;;
esac
curl -fsSL "https://github.com/lewisgcm/granit/releases/latest/download/granit-$target.tar.gz" \
  | sudo tar -xz -C /usr/local/bin granit
```

To install a specific version, replace `latest/download` with
`download/<tag>`, for example `download/v0.1.0`.

### Verifying the download

Each archive has a matching `.sha256` file:

```sh
base="https://github.com/lewisgcm/granit/releases/latest/download"
curl -fsSLO "$base/granit-$target.tar.gz"
curl -fsSLO "$base/granit-$target.tar.gz.sha256"
shasum -a 256 -c "granit-$target.tar.gz.sha256"
```

> **macOS:** the binaries aren't notarized. Installing with `curl` (as above)
> avoids Gatekeeper's quarantine. If you downloaded the archive in a browser, run
> `xattr -d com.apple.quarantine /usr/local/bin/granit` once.

## From source

With a Rust toolchain installed:

```sh
cargo install --git https://github.com/lewisgcm/granit
```

## Check your setup

```sh
granit --version
granit doctor
```

`granit doctor` checks that Nix is installed and that flakes work.
