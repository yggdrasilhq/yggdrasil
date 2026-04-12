#!/usr/bin/env bash
set -euo pipefail

docker_cmd=()
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  docker_cmd=(docker)
elif command -v sudo >/dev/null 2>&1 && sudo -n docker info >/dev/null 2>&1; then
  docker_cmd=(sudo -n docker)
else
  echo "docker daemon access is required to build the yggdrasil-maker image" >&2
  exit 1
fi

version="$(
  python3 - <<'PY'
import pathlib
import tomllib

with pathlib.Path("yggdrasil-maker/Cargo.toml").open("rb") as handle:
    payload = tomllib.load(handle)
print(payload["workspace"]["package"]["version"])
PY
)"

if [[ -z "$version" ]]; then
  echo "failed to resolve workspace version" >&2
  exit 1
fi

image_ref="${1:-ghcr.io/yggdrasilhq/yggdrasil-maker-build:v${version}}"

"${docker_cmd[@]}" build \
  -f docker/yggdrasil-maker-build.Dockerfile \
  -t "$image_ref" \
  .

echo "$image_ref"
