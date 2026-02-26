# Smoke Test Bench

The smoke bench is split into:

- static checks (hook/service definitions in repo)
- artifact presence checks (optional strict mode)
- post-boot runtime checks (manual for now)

## Local Static Bench

```bash
./tests/smoke/run.sh --profile both
```

## Artifact Gate

```bash
./tests/smoke/run.sh --profile both --require-artifacts --artifacts-dir .
```

## Rootfs Content Gate (recommended)

```bash
./tests/smoke/run.sh --profile both --require-artifacts --with-iso-rootfs
```

## Optional VM Boot Gate (QEMU/KVM hosts)

```bash
./tests/smoke/run.sh --profile both --require-artifacts --with-qemu-boot
```

Prerequisites for this gate are documented in `~/qemu_kvm.md` (device passthrough, packages, OVMF).

## Post-Boot Runtime Bench (Required Before Ship)

1. `systemctl status ygg-import-zpool-at-boot`
2. `systemctl status ygg-lxc-autostart`
3. `systemctl status ygg-infisical-ensure`
4. `systemctl status nvidia-firstboot` (if NVIDIA enabled)
5. Validate `/etc/lxc/lxc.conf` and `/etc/lxc/default.conf`
6. On KDE profile, verify `zfs` userland works (`zpool status`, `zfs list`)
