#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./tests/smoke/iso-rootfs-check.sh --iso PATH [--expect-kde]
USAGE
}

ISO=""
EXPECT_KDE="false"
WORKDIR="$(mktemp -d)"
MNT_DIR="$WORKDIR/mnt"
ROOTFS_DIR="$WORKDIR/rootfs"

cleanup() {
  if mountpoint -q "$MNT_DIR" 2>/dev/null; then
    umount "$MNT_DIR" || true
  fi
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iso)
      ISO="${2:-}"
      shift 2
      ;;
    --expect-kde)
      EXPECT_KDE="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$ISO" || ! -f "$ISO" ]]; then
  echo "ISO not found: $ISO" >&2
  exit 1
fi

for cmd in find; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "Missing command: $cmd" >&2; exit 1; }
done

mkdir -p "$MNT_DIR" "$ROOTFS_DIR"

if command -v mount >/dev/null 2>&1 && mount -o loop,ro "$ISO" "$MNT_DIR" 2>/dev/null; then
  :
elif command -v bsdtar >/dev/null 2>&1; then
  bsdtar -C "$MNT_DIR" -xf "$ISO"
else
  echo "Unable to open ISO: loop mount failed and bsdtar is unavailable" >&2
  exit 1
fi

SQUASHFS=$(find "$MNT_DIR" -type f -name filesystem.squashfs | head -n1 || true)
if [[ -z "$SQUASHFS" ]]; then
  echo "filesystem.squashfs not found in ISO" >&2
  exit 1
fi

if command -v unsquashfs >/dev/null 2>&1; then
  unsquashfs -no-progress -d "$ROOTFS_DIR" "$SQUASHFS" >/dev/null
else
  echo "unsquashfs not available; cannot inspect rootfs" >&2
  exit 1
fi

check_bin() {
  local path="$1"
  if [[ -x "$ROOTFS_DIR$path" ]]; then
    echo "[PASS] binary present: $path"
  else
    echo "[FAIL] missing binary: $path"
    return 1
  fi
}

check_file() {
  local path="$1"
  if [[ -f "$ROOTFS_DIR$path" ]]; then
    echo "[PASS] file present: $path"
  else
    echo "[FAIL] missing file: $path"
    return 1
  fi
}

status=0
check_bin "/usr/sbin/zpool" || status=1
check_bin "/usr/sbin/zfs" || status=1
check_file "/etc/systemd/system/ygg-import-zpool-at-boot.service" || status=1
check_file "/etc/systemd/system/ygg-lxc-autostart.service" || status=1
check_file "/etc/systemd/system/ygg-infisical-ensure.service" || status=1

if [[ "$EXPECT_KDE" == "true" ]]; then
  check_file "/usr/bin/startplasma-x11" || status=1
fi

if [[ $status -ne 0 ]]; then
  echo "ISO rootfs smoke check failed"
  exit 1
fi

echo "ISO rootfs smoke check passed: $ISO"
