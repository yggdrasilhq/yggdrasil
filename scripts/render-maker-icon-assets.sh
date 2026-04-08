#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SVG_PATH="${1:-$ROOT_DIR/yggdrasil-maker/assets/brand/yggdrasil-maker-icon.svg}"
PNG_PATH="${2:-$ROOT_DIR/yggdrasil-maker/assets/brand/yggdrasil-maker-icon-512.png}"
SIZE="${3:-512}"

if ! command -v rsvg-convert >/dev/null 2>&1; then
  echo "missing dependency: rsvg-convert" >&2
  exit 1
fi

tmp_png="$(mktemp "${TMPDIR:-/tmp}/yggdrasil-maker-icon.XXXXXX.png")"
trap 'rm -f "$tmp_png"' EXIT

rsvg-convert \
  --width "$SIZE" \
  --height "$SIZE" \
  --keep-aspect-ratio \
  "$SVG_PATH" \
  >"$tmp_png"

mv "$tmp_png" "$PNG_PATH"
echo "rendered $(basename "$PNG_PATH") from $(basename "$SVG_PATH") at ${SIZE}px"
