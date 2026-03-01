# yggdrasil

Debian live-build system for Yggdrasil server/KDE ISOs.

## Repository Boundaries

- This repo: build pipeline and ISO composition only.
- TUI/config UX: `ygg-cli`.
- Documentation/wiki: `ygg-docs`.

## Local Config

Use a local untracked config file.

- tracked example: `ygg.example.toml`
- tracked private-profile template: `ygg.legacy-infra.example.toml`
- local file: `ygg.local.toml` (gitignored)

`mkconfig.sh` accepts `--config` with either:
- TOML (`*.toml`)
- env-file (`YGG_*` key/value lines)

## Build

```bash
./mkconfig.sh --config ./ygg.local.toml --profile both
./mkconfig.sh --config ./ygg.local.toml --profile server --skip-smoke
```

## License

Apache-2.0
