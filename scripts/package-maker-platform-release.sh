#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DIST_DIR="${ROOT_DIR}/dist"
TARGET_LABEL="${1:?usage: package-maker-platform-release.sh <label> [target-triple] [--skip-build] [--input PATH]}"
shift

TARGET_TRIPLE=""
SKIP_BUILD="false"
INPUT_PATH=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build)
      SKIP_BUILD="true"
      shift
      ;;
    --input)
      INPUT_PATH="${2:-}"
      shift 2
      ;;
    *)
      if [[ -z "$TARGET_TRIPLE" ]]; then
        TARGET_TRIPLE="$1"
        shift
      else
        echo "Unknown argument: $1" >&2
        exit 1
      fi
      ;;
  esac
done

mkdir -p "$DIST_DIR"

BIN_NAME="yggdrasil-maker"
ARCHIVE_EXT="tar.gz"
CARGO_FEATURES="${YGGDRASIL_MAKER_CARGO_FEATURES:-desktop-ui}"
case "$TARGET_LABEL" in
  windows-*)
    BIN_NAME="yggdrasil-maker.exe"
    ARCHIVE_EXT="zip"
    ;;
esac

BIN_PATH="${ROOT_DIR}/target/release/${BIN_NAME}"
BUILD_CMD=(cargo build --release -p yggdrasil-maker --bin yggdrasil-maker --features "$CARGO_FEATURES")
if [[ -n "$TARGET_TRIPLE" ]]; then
  BUILD_CMD+=(--target "$TARGET_TRIPLE")
  BIN_PATH="${ROOT_DIR}/target/${TARGET_TRIPLE}/release/${BIN_NAME}"
fi

if [[ -n "$INPUT_PATH" ]]; then
  BIN_PATH="$INPUT_PATH"
fi

if [[ "$SKIP_BUILD" != "true" && -z "$INPUT_PATH" ]]; then
  (
    cd "$ROOT_DIR"
    "${BUILD_CMD[@]}"
  )
fi

if [[ ! -f "$BIN_PATH" ]]; then
  echo "binary not found: $BIN_PATH" >&2
  exit 1
fi

OUT_BASENAME="yggdrasil-maker-${TARGET_LABEL}"
if [[ "$BIN_NAME" == *.exe ]]; then
  OUT_BASENAME="${OUT_BASENAME}.exe"
fi

cp "$BIN_PATH" "${DIST_DIR}/${OUT_BASENAME}"

checksum_file() {
  local file="$1"
  local out="$2"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" > "$out"
  else
    shasum -a 256 "$file" > "$out"
  fi
}

checksum_file "${DIST_DIR}/${OUT_BASENAME}" "${DIST_DIR}/${OUT_BASENAME}.sha256"

ARCHIVE_PATH="${DIST_DIR}/yggdrasil-maker-${TARGET_LABEL}.${ARCHIVE_EXT}"
if [[ "$ARCHIVE_EXT" == "tar.gz" ]]; then
  tar -C "$DIST_DIR" -czf "$ARCHIVE_PATH" \
    "${OUT_BASENAME}" \
    "${OUT_BASENAME}.sha256"
else
  python3 - "$DIST_DIR" "$ARCHIVE_PATH" "$OUT_BASENAME" "${OUT_BASENAME}.sha256" <<'PY'
import pathlib
import sys
import zipfile

dist = pathlib.Path(sys.argv[1])
archive = pathlib.Path(sys.argv[2])
files = [sys.argv[3], sys.argv[4]]
with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for name in files:
        zf.write(dist / name, arcname=name)
PY
fi

checksum_file "$ARCHIVE_PATH" "${ARCHIVE_PATH}.sha256"

python3 - "$DIST_DIR" "$TARGET_LABEL" "$OUT_BASENAME" "$ARCHIVE_PATH" <<'PY'
import hashlib
import json
import pathlib
import sys

dist = pathlib.Path(sys.argv[1])
target_label = sys.argv[2]
binary_name = sys.argv[3]
archive_path = pathlib.Path(sys.argv[4])

def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

payload = {
    "target": target_label,
    "binary": {
        "name": binary_name,
        "sha256": sha256(dist / binary_name),
        "size_bytes": (dist / binary_name).stat().st_size,
    },
    "archive": {
        "name": archive_path.name,
        "sha256": sha256(archive_path),
        "size_bytes": archive_path.stat().st_size,
    },
}
(dist / f"yggdrasil-maker-{target_label}.json").write_text(
    json.dumps(payload, indent=2) + "\n",
    encoding="utf-8",
)
PY

echo "Release binary: ${DIST_DIR}/${OUT_BASENAME}"
echo "Release archive: ${ARCHIVE_PATH}"
echo "Target metadata: ${DIST_DIR}/yggdrasil-maker-${TARGET_LABEL}.json"
