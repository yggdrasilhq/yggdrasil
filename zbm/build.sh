#!/usr/bin/env bash
# Build a ZFSBootMenu EFI with optional recovery layers (wifi, ssh).
# Usage: sudo ./zbm/build.sh [--out DIR]
# Layers are opt-in via ygg.local.toml (zbm_wifi, zbm_ssh); private creds
# stay in the site layer — never in this repository.
set -euo pipefail
OUT=${1:-/boot/efi/EFI/ZBM}
TAG=${ZBM_TAG:-v3.0.0}          # pinned upstream zfsbootmenu
KERNEL=$(ls -1 /boot/vmlinuz-* 2>/dev/null | sort -V | tail -1)

command -v generate-zbm >/dev/null || { echo "install zfsbootmenu tooling first"; exit 1; }
[ -n "$KERNEL" ] || { echo "no /boot kernel found"; exit 1; }

echo "== building ZBM $TAG on $KERNEL"
mkdir -p "$OUT"
generate-zbm
echo "== prove-before-boot:"
INITRD="$OUT/vmlinuz.EFI"
[ -f "$INITRD" ] || { echo "no EFI produced"; exit 1; }
if grep -qa "tsc=reliable" "$INITRD"; then echo "REFUSING: forbidden param embedded"; exit 1; fi
echo "== ZBM EFI ready at $INITRD (backup the previous one before install)"
