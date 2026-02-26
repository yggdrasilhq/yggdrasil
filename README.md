# Yggdrasil

Yggdrasil builds Debian Unstable live ISOs for USB boot with a ZFS + LXC host focus.
The project now ships two first-class profiles on every release cycle:

- `server`: minimal host profile
- `kde`: desktop/laptop profile

Both profiles are required release artifacts and both must pass smoke tests before publish.

## License

Apache-2.0, Copyright 2026 Avikalpa Kundu <avi@gour.top>.

## Quick Start

```bash
# 1) Guided setup for non-technical users (recommended)
./scripts/mkconfig-tui.sh

# 2) Build both profiles (default release behavior)
./mkconfig.sh --profile both

# 3) Run smoke checks against repo/hook definitions
./tests/smoke/run.sh
```

## Build Modes

- `recommended`: embeds SSH keys and asks for secure network settings.
- `quick-try`: allows skipping keys/network customization; emits clear security reminders.

## Repository Layout

- `mkconfig.sh`: top-level build entrypoint
- `scripts/mkconfig-tui.sh`: guided terminal setup wizard
- `scripts/build-profile.sh`: per-profile build wrapper
- `tests/smoke/run.sh`: smoke test bench (pre-ship gate)
- `docs/`: docs and release runbooks
- `config/hooks/`: live-build chroot hooks

## Release Policy

- Always build both `server` and `kde` profiles.
- Never publish if either profile fails smoke checks.
- Retention policy is profile-specific:
  - keep latest ISO from last 3 days
  - keep last 2 releases older than 3 days

## Documentation

Docs are maintained as code in `docs/` and intended for publication at `yggdrasil.gour.top`.
See [`docs/index.md`](docs/index.md).
