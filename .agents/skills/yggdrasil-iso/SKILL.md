---
name: yggdrasil-iso
description: Build, test, and ship the yggdrasil spine ISO (ZFS+LXC Debian
  live image with ZFSBootMenu recovery). Use when building the ISO, changing
  the generator, adding site knobs, or shipping to the Ventoy stick.
---

# yggdrasil-iso

Build a bootable, self-healing ZFS+LXC Debian live ISO from one config file.

## Why this exists

The ISO is the second brain: if the host dies, the stick rebuilds it knowing
everything the live machine knew. Anything done ad-hoc on a running host is
a change that dies at the next reboot — OS-layer changes MUST land here.

## The layering

- `mkconfig.sh` — front door: profiles, args, site config.
- `scripts/mkconfig-core.sh` — the generator AND builder: renders ALL of
  `config/` from inline emission blocks, then drives live-build.
- `scripts/build-profile.sh` — owns the export allowlist: the explicit
  `YGG_*` variables passed to the generator. A knob missing here renders
  empty, silently.
- Private values live in `ygg.local.toml` (gitignored) → `YGG_*` env.
  The public repo never sees site values.

## The build loop

1. Edit behaviour in `scripts/mkconfig-core.sh` (never in rendered `config/`).
2. `sudo ./mkconfig.sh --profile both` (root required for chroot apt).
3. `./tests/smoke/run.sh` must pass for both profiles.
4. Ship: ISO + sha256 to the marker-detected Ventoy stick.
5. Chown the generated dirs back to your user afterwards.

## The laws (each cost a wasted build)

1. The render wipes `config/` — emitted-from-core only.
2. New knob = consumer + ygg.local.toml key + allowlist entry. All three.
3. Unique heredoc terminators only; an inner EOF executes host-side code.
4. Promise-vs-artifact: if a boot param or package list promises something,
   smoke-test that the artifact contains it.
5. Kernel parameters on ZBM systems live inside the generated ZBM EFI —
   prove-before-boot by unpacking and grepping the initramfs.
6. The kernel cmdline on ZBM systems = the ZFS property
   org.zfsbootmenu:commandline (pool root + BE + snapshots). zfs get -r
   before grepping files; zfs set to change; it never appears as a file.
7. Never pkill -f a string that appears in your own command line.

## Ship

`scripts/build-and-ship.sh` (site overlay repo) discovers the stick by its
`.yggdrasil-stick` marker and copies ISO + sha256. Keep the currently-booted
ISO on the stick until one good boot from the new one.
