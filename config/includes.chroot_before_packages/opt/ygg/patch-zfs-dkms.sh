#!/bin/bash
set -euo pipefail

cache_dir="/var/cache/apt/archives"
needs_patch=0

if ! [ -t 0 ]; then
    while read -r pkg; do
        if [[ "$pkg" == zfs-dkms* ]]; then
            needs_patch=1
            break
        fi
    done
fi

if [[ $needs_patch -eq 0 ]]; then
    exit 0
fi

shopt -s nullglob
for deb in "$cache_dir"/zfs-dkms_*.deb; do
    tmpdir=$(mktemp -d)
    dpkg-deb -R "$deb" "$tmpdir"
    srcdir=$(find "$tmpdir/usr/src" -maxdepth 1 -mindepth 1 -type d -name 'zfs-*' | head -n1 || true)
    if [[ -z "$srcdir" ]]; then
        rm -rf "$tmpdir"
        continue
    fi

    dkms_conf="$srcdir/dkms.conf"
    if grep -q -- '--enable-linux-experimental' "$dkms_conf"; then
        rm -rf "$tmpdir"
        continue
    fi

    echo "[ygg] Patching $(basename "$deb") for experimental kernel support"
    perl -0pi -e 's/(--with-linux-obj="\$\{kernel_source_dir\}"\n)/$1  --enable-linux-experimental\n/' "$dkms_conf"
    dpkg-deb -b "$tmpdir" "${deb}.patched" >/dev/null
    mv "${deb}.patched" "$deb"
    rm -rf "$tmpdir"
done
shopt -u nullglob
