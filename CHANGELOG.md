# Changelog

This file tracks user-visible changes in `yggdrasil`.

## Unreleased

- start `yggdrasil-maker` inside this repo with a versioned Rust workspace, shared setup/build contracts, and a foundation automation CLI
- add a direct-install/update channel scaffold for `curl | sh` and `irm ... | iex` installs, following the `yggterm` release pattern
- lock v1 distribution around native GitHub Release downloads first, with `curl/iex` preserved as the secondary automation path
- add native release-packaging scripts and a release manifest for `yggdrasil-maker`
- rewrite the README front door around `yggdrasil-maker`, native `mkconfig.sh`, and the actual ecosystem boundaries
