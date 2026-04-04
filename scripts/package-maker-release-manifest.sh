#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DIST_DIR="${ROOT_DIR}/dist"
OUT_PATH="${1:-${DIST_DIR}/yggdrasil-maker-release-manifest.json}"

mkdir -p "$DIST_DIR"

python3 - "$DIST_DIR" "$OUT_PATH" <<'PY'
import json
import pathlib
import sys

dist = pathlib.Path(sys.argv[1])
out_path = pathlib.Path(sys.argv[2])
entries = []
for path in sorted(dist.glob("yggdrasil-maker-*.json")):
    if path.name == out_path.name:
        continue
    entries.append(json.loads(path.read_text(encoding="utf-8")))

payload = {
    "product": "yggdrasil-maker",
    "channels": {
        "public": "native-downloads",
        "automation": ["curl-sh", "irm-iex"],
    },
    "targets": entries,
}
out_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print(out_path)
PY

