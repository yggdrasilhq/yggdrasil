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
  if [[ -n "${USER_CONFIG_ENV:-}" && "$USER_CONFIG_ENV" == /tmp/ygg-config-*.env ]]; then
    rm -f "$USER_CONFIG_ENV"
  fi
}
trap cleanup EXIT

if [[ -n "$USER_CONFIG" ]]; then
  USER_CONFIG_ENV="$USER_CONFIG"
  if [[ "$USER_CONFIG" == *.toml ]]; then
    USER_CONFIG_ENV="$(mktemp /tmp/ygg-config-XXXXXX.env)"
    ./scripts/toml-to-env.sh "$USER_CONFIG" > "$USER_CONFIG_ENV"
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
YGG_APT_HTTP_PROXY="${YGG_APT_HTTP_PROXY:-}"
YGG_APT_HTTPS_PROXY="${YGG_APT_HTTPS_PROXY:-$YGG_APT_HTTP_PROXY}"
YGG_APT_PROXY_BYPASS_HOST="${YGG_APT_PROXY_BYPASS_HOST:-}"

if [[ "$YGG_SETUP_MODE" != "recommended" && "$YGG_SETUP_MODE" != "quick-try" ]]; then
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

if [[ "${YGG_SETUP_MODE:-recommended}" == "quick-try" ]]; then
  echo "WARNING: quick-try mode selected. This mode is for evaluation only."
  echo "WARNING: configure SSH keys and hardened networking before production use."
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
  YGG_APT_HTTP_PROXY \
  YGG_APT_HTTPS_PROXY \
  YGG_APT_PROXY_BYPASS_HOST

cmd=("./scripts/mkconfig-core.sh")
if [[ "$PROFILE" == "kde" ]]; then
  cmd+=("--with-kde")
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
