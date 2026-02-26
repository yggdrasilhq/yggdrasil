# Contributing

## Principles

- Keep build behavior deterministic and auditable.
- Treat `server` and `kde` as equal release targets.
- Prefer small, reviewable patches.

## Development Flow

1. Open an issue for behavior changes or release-process changes.
2. Add/update docs for any operational change.
3. Run local checks:

```bash
bash -n mkconfig.sh
bash tests/smoke/run.sh
```

4. Keep shell scripts POSIX-friendly where practical.

## Commit Guidance

- Use clear scope prefixes like `build:`, `docs:`, `test:`, `tui:`.
- Include why a change is needed, not only what changed.
