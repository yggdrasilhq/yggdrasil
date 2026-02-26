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
3. smoke tests pass for both profiles

## Retention

Retention is profile-specific:

- keep newest ISO from last 3 days
- keep last 2 older profile-specific ISOs
