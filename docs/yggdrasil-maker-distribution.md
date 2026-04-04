# yggdrasil-maker Distribution Contract

This document locks the v1 distribution story for `yggdrasil-maker`.

## Public Contract

`yggdrasil-maker` is a GUI-first product.

That means the public story is:

- download a native app build from GitHub Releases
- launch it directly
- use the GUI as the canonical front door

The automation story is still real, but secondary:

- `curl | sh` on Linux and macOS
- `irm ... | iex` on Windows
- stable CLI flags for agents, cron, and power users

## Source Of Truth

GitHub Releases is the only source of truth in v1.

Each release should publish:

- `yggdrasil-maker-linux-x86_64.tar.gz`
- `yggdrasil-maker-linux-aarch64.tar.gz`
- `yggdrasil-maker-macos-x86_64.tar.gz`
- `yggdrasil-maker-macos-aarch64.tar.gz`
- `yggdrasil-maker-windows-x86_64.zip`
- `yggdrasil-maker-windows-aarch64.zip`
- `.sha256` files for every binary and archive
- one `yggdrasil-maker-release-manifest.json`

## Why This Is The Right v1 Shape

- normal users should click Download, not paste a shell one-liner
- GitHub Releases already solves hosting and latest-version resolution
- `curl/iex` remains ideal for automation and direct installs
- package managers can come after the binary/install shape stabilizes

## Post-v1 Additions

Once the native asset names and install behavior stop moving:

1. add a Homebrew tap for macOS and Linux power users
2. add WinGet manifests for Windows
3. only then consider heavier distribution paths like MSIX/App Installer

## Packaging Commands

Package one target:

```bash
./scripts/package-maker-platform-release.sh linux-x86_64
./scripts/package-maker-platform-release.sh linux-aarch64 aarch64-unknown-linux-gnu
./scripts/package-maker-platform-release.sh windows-x86_64 x86_64-pc-windows-gnu
```

Build the manifest from generated target metadata:

```bash
./scripts/package-maker-release-manifest.sh
```
