# Changelog

This file tracks user-visible changes in `yggdrasil`.

## Unreleased

- start `yggdrasil-maker` inside this repo with a versioned Rust workspace, shared setup/build contracts, and a foundation automation CLI
- add a direct-install/update channel scaffold for `curl | sh` and `irm ... | iex` installs, following the `yggterm` release pattern
- lock v1 distribution around native GitHub Release downloads first, with `curl/iex` preserved as the secondary automation path
- add native release-packaging scripts and a release manifest for `yggdrasil-maker`
- rewrite the README front door around `yggdrasil-maker`, native `mkconfig.sh`, and the actual ecosystem boundaries
- extract a shared `maker-app` orchestration layer for setup storage, config emission, export bundles, and Docker build sessions
- add the first feature-gated native GUI shell over the shared maker crates
- make the build container emit structured JSON events plus an artifact manifest, including a distinct smoke-test failure path
- add a tag-driven GitHub Actions workflow that packages native app assets and pushes the version-matched build container
