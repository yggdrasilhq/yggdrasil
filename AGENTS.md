# AGENTS

## Mission

Build and release dual-profile Debian live ISOs (`server` and `kde`) with ZFS + LXC defaults.

## Release Rules

- Always build both profiles.
- Never ship if smoke tests fail for either profile.
- Keep profile-specific ISO retention policy:
  - latest ISO from last 3 days
  - last 2 older releases

## Build Entrypoints

- `./mkconfig.sh --profile both`
- `./scripts/mkconfig-tui.sh`
- `./tests/smoke/run.sh`

## Testing Expectations

Smoke tests must cover, at minimum:

- `ygg-import-zpool-at-boot.service`
- `ygg-lxc-autostart.service`
- `ygg-infisical-ensure.service`
- `/etc/lxc/lxc.conf` and `/etc/lxc/default.conf` semantics
- KDE profile post-boot `zfs` userland check

## Public Docs

Documentation is docs-as-code and published to `yggdrasil.gour.top`.
