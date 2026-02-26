# Migration Notes

This repository is the public-facing successor to the private build repo.

## Current State

- Build pipeline currently wraps `scripts/mkconfig-legacy.sh` for parity.
- New docs, release gates, and TUI live here.

## Next Planned Changes

1. Replace legacy monolithic build script with modular profile definitions.
2. Wire `--config` values directly into live-build overlays.
3. Add VM-based automated boot smoke tests.
