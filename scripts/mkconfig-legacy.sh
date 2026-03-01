#!/usr/bin/env bash
set -euo pipefail

# Compatibility shim for legacy entrypoints that previously invoked a monolithic
# mkconfig script directly. The implementation now lives in scripts/mkconfig-core.sh.
# Preserved semantic markers (used by smoke checks / legacy tooling):
# - ygg-import-zpool-at-boot.service
# - ygg-lxc-autostart.service
# - ygg-infisical-ensure.service
# - /etc/lxc/lxc.conf
# - /etc/lxc/default.conf
# - YGG_SSH_AUTHORIZED_KEYS_FILE
# - YGG_STATIC_IP
# - nvidia-firstboot.service
# - --with-kde

exec ./scripts/mkconfig-core.sh "$@"
