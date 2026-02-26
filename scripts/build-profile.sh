#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./scripts/build-profile.sh --profile server|kde [--config PATH]
USAGE
}

PROFILE=""
USER_CONFIG=""

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

if [[ -n "$USER_CONFIG" ]]; then
  # shellcheck disable=SC1090
  source "$USER_CONFIG"
fi

if [[ "${YGG_SETUP_MODE:-recommended}" == "quick-try" ]]; then
  echo "WARNING: quick-try mode selected. This mode is for evaluation only."
  echo "WARNING: configure SSH keys and hardened networking before production use."
fi

cmd=("./scripts/mkconfig-legacy.sh")
if [[ "$PROFILE" == "kde" ]]; then
  cmd+=("--with-kde")
fi

echo "Starting legacy build pipeline for profile: $PROFILE"
"${cmd[@]}"

./scripts/prune-isos.sh

echo "Build complete for profile: $PROFILE"
