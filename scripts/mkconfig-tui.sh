#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-./config/includes.chroot/etc/yggdrasil}"
OUT_FILE="$OUT_DIR/user-config.env"
mkdir -p "$OUT_DIR"

if command -v whiptail >/dev/null 2>&1; then
  UI="whiptail"
elif command -v dialog >/dev/null 2>&1; then
  UI="dialog"
else
  echo "Install whiptail or dialog to use TUI."
  exit 1
fi

menu() {
  local title="$1"
  shift
  if [[ "$UI" == "whiptail" ]]; then
    whiptail --title "$title" --menu "Choose:" 18 78 8 "$@" 3>&1 1>&2 2>&3
  else
    dialog --clear --title "$title" --menu "Choose:" 18 78 8 "$@" 2>&1 >/dev/tty
  fi
}

input() {
  local title="$1"
  local prompt="$2"
  local initial="${3:-}"
  if [[ "$UI" == "whiptail" ]]; then
    whiptail --title "$title" --inputbox "$prompt" 12 78 "$initial" 3>&1 1>&2 2>&3
  else
    dialog --clear --title "$title" --inputbox "$prompt" 12 78 "$initial" 2>&1 >/dev/tty
  fi
}

msg() {
  local title="$1"
  local text="$2"
  if [[ "$UI" == "whiptail" ]]; then
    whiptail --title "$title" --msgbox "$text" 14 78
  else
    dialog --clear --title "$title" --msgbox "$text" 14 78
  fi
}

MODE=$(menu "Yggdrasil Setup Mode" \
  recommended "Secure defaults with SSH key flow" \
  quick-try "Skip hardening for evaluation only")

case "$MODE" in
  recommended|quick-try) ;;
  *) echo "Setup canceled."; exit 1 ;;
esac

EMBED_SSH_KEYS="false"
SSH_KEYS_FILE=""
if [[ "$MODE" == "recommended" ]]; then
  EMBED_SSH_KEYS="true"
  SSH_KEYS_FILE=$(input "SSH Keys" "Path to authorized_keys file:" "/root/.ssh/authorized_keys")
  if [[ ! -f "$SSH_KEYS_FILE" ]]; then
    msg "Missing file" "SSH key file not found: $SSH_KEYS_FILE\nYou can still continue, but this is not recommended."
    EMBED_SSH_KEYS="false"
    SSH_KEYS_FILE=""
  fi
else
  msg "Security Reminder" "Quick-try is for test drives only.\nUse recommended mode before production deployment."
fi

HOSTNAME=$(input "Hostname" "Live hostname (leave empty for profile default):" "")
LXC_PARENT_IF=$(input "LXC Parent Iface" "Parent interface for LXC macvlan:" "eno1")
MACVLAN_CIDR=$(input "Macvlan CIDR" "Host macvlan CIDR for LXC reachability:" "192.168.0.250/22")
MACVLAN_ROUTE=$(input "Macvlan Route" "Route CIDR for guest subnet:" "192.168.0.0/22")

NET_MODE=$(menu "Networking" \
  dhcp "Use DHCP" \
  static "Set static IPv4")

STATIC_IP=""
STATIC_GW=""
STATIC_DNS=""
STATIC_IFACE="$LXC_PARENT_IF"
if [[ "$NET_MODE" == "static" ]]; then
  STATIC_IFACE=$(input "Static Iface" "Interface for static IP:" "$LXC_PARENT_IF")
  STATIC_IP=$(input "Static IP" "IPv4 CIDR (example: 192.168.1.20/24):" "")
  STATIC_GW=$(input "Gateway" "Default gateway IPv4:" "")
  STATIC_DNS=$(input "DNS" "DNS servers (space-separated):" "1.1.1.1 8.8.8.8")
fi

cat > "$OUT_FILE" <<EOC
YGG_SETUP_MODE="$MODE"
YGG_EMBED_SSH_KEYS="$EMBED_SSH_KEYS"
YGG_SSH_AUTHORIZED_KEYS_FILE="$SSH_KEYS_FILE"
YGG_HOSTNAME="$HOSTNAME"
YGG_NET_MODE="$NET_MODE"
YGG_LXC_PARENT_IF="$LXC_PARENT_IF"
YGG_MACVLAN_CIDR="$MACVLAN_CIDR"
YGG_MACVLAN_ROUTE="$MACVLAN_ROUTE"
YGG_STATIC_IFACE="$STATIC_IFACE"
YGG_STATIC_IP="$STATIC_IP"
YGG_STATIC_GATEWAY="$STATIC_GW"
YGG_STATIC_DNS="$STATIC_DNS"
EOC

msg "Saved" "Configuration saved to:\n$OUT_FILE\n\nNext:\n./mkconfig.sh --profile both --config $OUT_FILE"

echo "Wrote $OUT_FILE"
