#!/usr/bin/env bash
set -euo pipefail

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required to build the yggdrasil-maker image" >&2
  exit 1
fi

version="$(
  python3 - <<'PY'
import pathlib
import tomllib

with pathlib.Path("Cargo.toml").open("rb") as handle:
    payload = tomllib.load(handle)
print(payload["workspace"]["package"]["version"])
PY
)"

if [[ -z "$version" ]]; then
  echo "failed to resolve workspace version" >&2
  exit 1
fi

image_ref="${1:-ghcr.io/yggdrasilhq/yggdrasil-maker-build:v${version}}"

docker build \
  -f docker/yggdrasil-maker-build.Dockerfile \
  -t "$image_ref" \
  .

echo "$image_ref"
