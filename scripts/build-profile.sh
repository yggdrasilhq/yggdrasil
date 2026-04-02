#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./scripts/build-profile.sh --profile server|kde [--config PATH]
  --config accepts either .toml or env (YGG_*) format.
USAGE
}

PROFILE=""
USER_CONFIG=""
USER_CONFIG_ENV=""
TEMP_CONFIG_ENV=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="${2:-}"
      shift 2
      ;;
    --config)
      USER_CONFIG="${2:-}"
      shift 2
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

if [[ "$PROFILE" != "server" && "$PROFILE" != "kde" ]]; then
  echo "--profile must be server or kde" >&2
  exit 1
fi

if [[ -n "$USER_CONFIG" && ! -f "$USER_CONFIG" ]]; then
  echo "Config file not found: $USER_CONFIG" >&2
  exit 1
fi

if ! command -v lb >/dev/null 2>&1; then
  echo "Missing dependency: lb (live-build)." >&2
  exit 1
fi

cleanup() {
  if [[ -n "${TEMP_CONFIG_ENV:-}" && -f "$TEMP_CONFIG_ENV" ]]; then
    rm -f "$TEMP_CONFIG_ENV"
  fi
}
trap cleanup EXIT

if [[ -n "$USER_CONFIG" ]]; then
  USER_CONFIG_ENV="$USER_CONFIG"
  if [[ "$USER_CONFIG" == *.toml ]]; then
    TEMP_CONFIG_ENV="$(mktemp /tmp/ygg-config-XXXXXX.env)"
    ./scripts/toml-to-env.sh "$USER_CONFIG" > "$TEMP_CONFIG_ENV"
    USER_CONFIG_ENV="$TEMP_CONFIG_ENV"
  fi
  # shellcheck disable=SC1090
  source "$USER_CONFIG_ENV"
fi

bool_or_default() {
  local value="$1"
  local fallback="$2"
  case "$value" in
    true|false) echo "$value" ;;
    "") echo "$fallback" ;;
    *) echo "$fallback" ;;
  esac
}

YGG_SETUP_MODE="${YGG_SETUP_MODE:-recommended}"
YGG_EMBED_SSH_KEYS="$(bool_or_default "${YGG_EMBED_SSH_KEYS:-}" "true")"
YGG_SSH_AUTHORIZED_KEYS_FILE="${YGG_SSH_AUTHORIZED_KEYS_FILE:-/root/.ssh/authorized_keys}"
YGG_NET_MODE="${YGG_NET_MODE:-dhcp}"
YGG_LXC_PARENT_IF="${YGG_LXC_PARENT_IF:-eno1}"
YGG_MACVLAN_CIDR="${YGG_MACVLAN_CIDR:-10.10.0.250/24}"
YGG_MACVLAN_ROUTE="${YGG_MACVLAN_ROUTE:-10.10.0.0/24}"
YGG_STATIC_IFACE="${YGG_STATIC_IFACE:-$YGG_LXC_PARENT_IF}"
YGG_STATIC_IP="${YGG_STATIC_IP:-}"
YGG_STATIC_GATEWAY="${YGG_STATIC_GATEWAY:-}"
YGG_STATIC_DNS="${YGG_STATIC_DNS:-}"
YGG_HOSTNAME="${YGG_HOSTNAME:-}"
YGG_APT_PROXY_MODE="${YGG_APT_PROXY_MODE:-off}"
YGG_APT_HTTP_PROXY="${YGG_APT_HTTP_PROXY:-}"
YGG_APT_HTTPS_PROXY="${YGG_APT_HTTPS_PROXY:-$YGG_APT_HTTP_PROXY}"
YGG_APT_PROXY_BYPASS_HOST="${YGG_APT_PROXY_BYPASS_HOST:-}"
YGG_WITH_NVIDIA="$(bool_or_default "${YGG_WITH_NVIDIA:-${YGG_ENABLE_NVIDIA:-}}" "true")"
YGG_WITH_LTS="$(bool_or_default "${YGG_WITH_LTS:-}" "false")"
YGG_ENABLE_INTEL_ARC_SRIOV="$(bool_or_default "${YGG_ENABLE_INTEL_ARC_SRIOV:-}" "false")"
YGG_INTEL_ARC_SRIOV_RELEASE="${YGG_INTEL_ARC_SRIOV_RELEASE:-2026.03.05}"
YGG_INTEL_ARC_SRIOV_VF_COUNT="${YGG_INTEL_ARC_SRIOV_VF_COUNT:-7}"
YGG_INTEL_ARC_SRIOV_PF_PCI="${YGG_INTEL_ARC_SRIOV_PF_PCI:-}"
YGG_INTEL_ARC_SRIOV_DEVICE_ID="${YGG_INTEL_ARC_SRIOV_DEVICE_ID:-0x56a0}"
YGG_INTEL_ARC_SRIOV_BIND_VFS="${YGG_INTEL_ARC_SRIOV_BIND_VFS:-vfio-pci}"

if [[ "$YGG_SETUP_MODE" != "recommended" ]]; then
  echo "Invalid YGG_SETUP_MODE: $YGG_SETUP_MODE" >&2
  exit 1
fi

if [[ "$YGG_NET_MODE" != "dhcp" && "$YGG_NET_MODE" != "static" ]]; then
  echo "Invalid YGG_NET_MODE: $YGG_NET_MODE (use dhcp or static)" >&2
  exit 1
fi

if [[ "$YGG_NET_MODE" == "static" && -z "$YGG_STATIC_IP" ]]; then
  echo "YGG_STATIC_IP is required when YGG_NET_MODE=static" >&2
  exit 1
fi

if ! [[ "$YGG_INTEL_ARC_SRIOV_VF_COUNT" =~ ^[0-9]+$ ]]; then
  echo "YGG_INTEL_ARC_SRIOV_VF_COUNT must be an integer" >&2
  exit 1
fi

if [[ "$YGG_INTEL_ARC_SRIOV_BIND_VFS" != "vfio-pci" && "$YGG_INTEL_ARC_SRIOV_BIND_VFS" != "none" ]]; then
  echo "YGG_INTEL_ARC_SRIOV_BIND_VFS must be vfio-pci or none" >&2
  exit 1
fi

export \
  YGG_SETUP_MODE \
  YGG_EMBED_SSH_KEYS \
  YGG_SSH_AUTHORIZED_KEYS_FILE \
  YGG_NET_MODE \
  YGG_LXC_PARENT_IF \
  YGG_MACVLAN_CIDR \
  YGG_MACVLAN_ROUTE \
  YGG_STATIC_IFACE \
  YGG_STATIC_IP \
  YGG_STATIC_GATEWAY \
  YGG_STATIC_DNS \
  YGG_HOSTNAME \
  YGG_APT_PROXY_MODE \
  YGG_APT_HTTP_PROXY \
  YGG_APT_HTTPS_PROXY \
  YGG_APT_PROXY_BYPASS_HOST \
  YGG_WITH_LTS \
  YGG_ENABLE_INTEL_ARC_SRIOV \
  YGG_INTEL_ARC_SRIOV_RELEASE \
  YGG_INTEL_ARC_SRIOV_VF_COUNT \
  YGG_INTEL_ARC_SRIOV_PF_PCI \
  YGG_INTEL_ARC_SRIOV_DEVICE_ID \
  YGG_INTEL_ARC_SRIOV_BIND_VFS

cmd=("./scripts/mkconfig-core.sh")
if [[ "$PROFILE" == "kde" ]]; then
  cmd+=("--with-kde")
fi
if [[ "$YGG_WITH_NVIDIA" != "true" ]]; then
  cmd+=("--without-nvidia")
fi
if [[ "$YGG_WITH_LTS" == "true" ]]; then
  cmd+=("--with-lts")
fi

echo "Starting build pipeline for profile: $PROFILE"
"${cmd[@]}"

./scripts/prune-isos.sh

mkdir -p artifacts
if [[ "$PROFILE" == "server" ]]; then
  latest_iso="$(ls -1t yggdrasil-*-amd64.hybrid.iso 2>/dev/null | rg -v -- '-kde-amd64\\.hybrid\\.iso$' | head -n1 || true)"
  if [[ -n "${latest_iso:-}" && -f "$latest_iso" ]]; then
    cp -f "$latest_iso" "artifacts/server-latest.iso"
  fi
else
  latest_iso="$(ls -1t yggdrasil-*-kde-amd64.hybrid.iso 2>/dev/null | head -n1 || true)"
  if [[ -n "${latest_iso:-}" && -f "$latest_iso" ]]; then
    cp -f "$latest_iso" "artifacts/kde-latest.iso"
  fi
fi

echo "Build complete for profile: $PROFILE"
