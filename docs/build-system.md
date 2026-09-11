# Build system notes — how the ISO build actually works, and where it bites

Dated: 2026-09-11, after the locale/extra-pools/proxy fix wave (8a28f12..2f0485e).
Read this before changing `scripts/mkconfig-core.sh` or `scripts/build-profile.sh`.

## The layering

- `mkconfig.sh` is the front door: parses profiles/args, renders nothing
  itself, delegates.
- `scripts/mkconfig-core.sh` is the generator AND the builder: it renders
  the ENTIRE `config/` tree (hooks, package lists, includes, lb args) from
  inline emission blocks plus site values, then drives live-build.
- `scripts/build-profile.sh` builds the per-profile core invocation and
  owns the **export allowlist**: the explicit list of `YGG_*` variables
  passed through to the generator. Everything else in the environment is
  invisible to the render.

## Laws (each one cost a wasted 40-minute build)

1. **Never fix behaviour by editing `config/`.** The render wipes and
   regenerates hooks, package lists and includes on every run. A
   repo-tracked file you add under `config/` silently disappears at the
   next render — the build then runs WITHOUT your fix and nothing warns
   you. Emit it from `mkconfig-core.sh` instead (see the `1030-` hook and
   the `ygg.list.chroot` locales line for the pattern).
2. **A new site knob needs three pieces**: the consumer in
   `mkconfig-core.sh` (usually an emission block expanding `${YGG_X:-}`),
   the key in the private `ygg.local.toml` (gitignored), and the variable
   in **build-profile.sh's allowlist**. Forget the third and the knob
   renders empty — with no error anywhere.
3. **Heredoc terminators collide.** Core emits hook files via heredocs;
   an inner `<<EOF`/`EOF` pair inside that body terminates the OUTER
   emission early, and the remaining lines execute on the build host with
   host permissions. Use unique terminators (`YGGPOOL`, `YGGEOF`) and
   prefer emitting content as `config/includes.chroot/...` files over
   host-side `tee /etc/...` mutations.
4. **Builds must run as root** (`sudo ./mkconfig.sh`) — the chroot apt
   phase needs it. Root leaves `config/ .build/ cache/ artifacts/ chroot/`
   root-owned; `chown -R pi:datashare` those after a build or the next
   user-level build dies on permission errors.
5. **Never `pkill -f mkconfig` from a shell whose own command line
   contains that string** — including the nohup line of the restart you
   are about to run. Kill by PID from `ps` output.
6. **A debootstrap `tar failed` right after an interrupted build** is a
   corrupted bootstrap cache, not a pool problem. Clear
   `cache/debootstrap*` and `cache/packages.bootstrap*` and re-run.
7. **Smoke tests check unit files and conf semantics, not promises:**
   boot params can promise things the image does not contain (the
   `locales=en_US.UTF-8` case: live-build 2025 removed native locale
   handling, the param stayed, no image ever contained the locale).
   When you add a boot promise, add a post-build existence check for the
   thing it promises.

## The apt proxy

`apt_http_proxy` / `apt_https_proxy` in `ygg.local.toml` flow through
`YGG_APT_HTTP_PROXY` into an active `/etc/apt/apt.conf.d/02proxy` include
plus `--apt-http-proxy` for the build fetches. Unset, the stock commented
template ships instead. The same include serves the booted system's apt —
point it at your fleet's apt-cacher-ng to make builds (and runtime apt)
cache-friendly.

## Extra pools at boot

`extra_pools = "poolname ..."` in `ygg.local.toml` renders
`/etc/default/ygg-import-zpool` into the image; the
`ygg-import-zpool-at-boot` service sources it and best-effort imports each
pool after `zroot` (never failing the boot when a disk is absent).
Cross-mount datasets (mountpoint under `/zroot/data/...`) come up with it,
which is what services bound to those paths need before they start —
`ygg-lxc-autostart` already orders `Requires=`/`After=` this service.

## Boot-chain reality check: ZBM systems ignore refind_linux.conf (2026-09-11)

The ISO installs BOTH rEFInd (with `refind_linux.conf`) and ZFSBootMenu.
On systems where the boot actually flows **rEFInd → ZFSBootMenu → kernel**,
`refind_linux.conf` is **dead config**: its per-option kernel command lines
never reach the kernel. ZBM assembles the command line itself and carries
it **embedded (compressed) inside the generated EFI** — so:

- The live `/proc/cmdline` will NOT match any `refind_linux.conf` entry.
- `grep -r "your-option" /etc /boot` finds nothing: the cmdline lives
  inside the compressed initramfs embedded in the ZBM EFI. Raw `strings`
  or grep on the binary cannot see it.
- Editing `refind_linux.conf` or `refind.conf` defaults cannot change
  kernel parameters on such systems (rEFInd selection tweaks are harmless
  but irrelevant here).

To change a kernel parameter on a ZBM system: find the generation source
(zbm config / dracut conf used at generate time), strip the parameter,
REGENERATE the ZBM EFI, then **prove it** — unpack the new EFI, extract the
initramfs, and grep for the parameter — before rebooting. ZBM EFIs also
regenerate without human action (kernel/dracut triggers), so re-check the
embedded cmdline after any kernel event: an old parameter can ride back in
on a regeneration from a stale conf.

Related: boot-parameter promises are exactly the class of thing the smoke
tests should verify against the built image (see the locale law above).

## THE ZBM COMMANDLINE LIVES IN A ZFS PROPERTY (learned the hard way, 2026-09-11)

The kernel command line a ZBM system actually boots with comes from the ZFS
user property `org.zfsbootmenu:commandline` — set on the pool root and/or
the boot environment, inherited by every snapshot. It appears NOWHERE as a
file: not in /etc, /boot, /boot/efi, refind_linux.conf, the zbm config
yaml, or the raw EFI binary (the cmdline travels inside the compressed
initramfs or via the property — greps find nothing).

Debug/fix procedure:

    zfs get -H -o name,value,source org.zfsbootmenu:commandline -r POOL
    sudo zfs set org.zfsbootmenu:commandline="quiet loglevel=4" POOL BE...

- Check BEFORE grepping files: `zfs get all -r POOL | grep param`.
- The property is settable per dataset AND per snapshot; sweep the pool
  recursively or old snapshots resurrect it.
- Layering observed: the property overrides the config.yaml
  `Kernel: CommandLine` embed. The BE rootfs also has /etc/kernel/cmdline
  (empty on jojo — ZBM honours it when non-empty).
- The tsc=reliable incident: the parameter survived four reboots and a ZBM
  regeneration because it rode this property on zroot, zroot/ROOT/debian,
  and ~50 hourly snapshots. A recursive `zfs set` swept all 51 datasets.
