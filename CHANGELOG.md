# Changelog

This file tracks user-visible changes in `yggdrasil`.

## Unreleased

- harden the server profile against the zfs 2.4.x block-clone txg wedge:
  ship a modprobe.d config with `zfs_bclone_enabled=0` so
  copy_file_range/reflink copies fall back to plain copies instead of
  being able to stall the whole pool; large image copies should use dd
- new ygg-txg-watchdog service+timer (1 min): logs a kern.crit entry
  and drops /run/ygg-txg-stuck when a pool stops committing txgs while
  txgs sit pending - the early-warning sign that preceded full hosts
  freezing under D-state load
- add apfs-dkms and qemu-utils to the server package list so hosts that
  mount apfs images or serve qcow devices keep that ability across
  reboots without hand-installing
- fix the kde profile: track the upstream helium-browser to helium-bin
  rename and stop pointing signed-by at an /etc/apt/keyrings/ path that
  live-build never creates (the key file lands in trusted.gpg.d); site
  tomls should set apt_proxy_bypass_host for hosts whose apt cache
  cannot pass through the helium https repo

- rebuild `yggdrasil-maker` as a libyggterm app: a CLI that takes over the
  yggterm viewport by declaring a web surface on its own PTY and serving its UI
  from loopback, and that degrades to printing its URL in a plain terminal
- `yggdrasil-maker` home view lists what the checkout really contains — profiles
  parsed from `mkconfig.sh`'s own usage block, every `ygg*.toml` and its knobs,
  package lists, hooks, built ISOs, and whether live-build is installed
- `yggdrasil-maker` plan view renders exactly what a build would run without
  running it, with each stage's real cost, the config delta against
  `ygg.example.toml`, and the environment resolved by really executing
  `scripts/toml-to-env.sh`
- `yggdrasil-maker` run view executes the config stage for real and streams its
  log with a true running/failed/done state and the real exit code
- remove the Rust/Dioxus desktop app and its release, packaging, container and
  installer machinery: it built only against a sibling checkout of the yggterm
  repo by relative path, so it could not be built from a clone of this
  repository at all
- remove the shell config wizard (`mkconfig-tui.sh` and its legacy shim), which
  duplicated `mkconfig.sh`'s flag surface in a second place that could drift
- rewrite the README front door and the `DESIGN.md` project overlay around the
  new app, and drop the absolute local paths the old README had leaked into a
  public repository
