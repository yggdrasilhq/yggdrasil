# Build and Release

## Standard Release Command

```bash
./mkconfig.sh --profile both
```

## With Guided Config

```bash
./scripts/mkconfig-tui.sh
./mkconfig.sh --profile both --config ./config/includes.chroot/etc/yggdrasil/user-config.env
```

## Publish Gate

Release only if:

1. `server` build succeeds
2. `kde` build succeeds
3. rootfs smoke tests pass for both profile artifacts
4. optional QEMU boot smoke passes when `YGG_ENABLE_QEMU_SMOKE=true`

## Optional QEMU/KVM Boot Gate

Enable VM boot checks on hosts with QEMU/KVM available:

```bash
YGG_ENABLE_QEMU_SMOKE=true ./mkconfig.sh --profile both
```

Use `~/qemu_kvm.md` for harness prerequisites and environment validation.

## Retention

Retention is profile-specific:

- keep newest ISO from last 3 days
- keep last 2 older profile-specific ISOs
