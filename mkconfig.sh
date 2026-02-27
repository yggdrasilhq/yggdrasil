#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./mkconfig.sh [options]

Options:
  --profile server|kde|both   Build target profile(s). Default: both.
  --config PATH               Optional config file (.toml or env YGG_*).
  --skip-smoke                Skip post-build smoke tests (not recommended).
  --dry-run                   Print actions without executing build.
  -h, --help                  Show this help.
USAGE
}

PROFILE="both"
PROFILE_SET="false"
USER_CONFIG=""
USER_CONFIG_ENV=""
SKIP_SMOKE="false"
DRY_RUN="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="${2:-}"
      PROFILE_SET="true"
      shift 2
      ;;
    --config)
      USER_CONFIG="${2:-}"
      shift 2
      ;;
    --skip-smoke)
      SKIP_SMOKE="true"
      shift
      ;;
    --dry-run)
      DRY_RUN="true"
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

case "$PROFILE" in
  server|kde|both) ;;
  *)
    echo "Invalid profile: $PROFILE" >&2
    exit 1
    ;;
esac

if [[ -n "$USER_CONFIG" && ! -f "$USER_CONFIG" ]]; then
  echo "Config file not found: $USER_CONFIG" >&2
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

if [[ "$PROFILE_SET" != "true" && -n "${YGG_BUILD_PROFILE:-}" ]]; then
  PROFILE="$YGG_BUILD_PROFILE"
fi

build_one() {
  local p="$1"
  local cmd=("./scripts/build-profile.sh" "--profile" "$p")
  if [[ -n "$USER_CONFIG_ENV" ]]; then
    cmd+=("--config" "$USER_CONFIG_ENV")
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] %q ' "${cmd[@]}"
    echo
    return 0
  fi

  "${cmd[@]}"
}

if [[ "$PROFILE" == "both" ]]; then
  build_one "server"
  build_one "kde"
else
  build_one "$PROFILE"
fi

if [[ "$SKIP_SMOKE" != "true" ]]; then
  smoke_cmd=(
    ./tests/smoke/run.sh
    --profile "$PROFILE"
    --require-artifacts
    --with-iso-rootfs
    --artifacts-dir ./artifacts
    --server-iso ./artifacts/server-latest.iso
    --kde-iso ./artifacts/kde-latest.iso
  )
  if [[ "${YGG_ENABLE_QEMU_SMOKE:-false}" == "true" ]]; then
    smoke_cmd+=(--with-qemu-boot)
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] %q ' "${smoke_cmd[@]}"
    echo
  else
    "${smoke_cmd[@]}"
  fi
fi

echo "Build workflow completed for profile: $PROFILE"
