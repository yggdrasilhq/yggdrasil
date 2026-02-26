#!/usr/bin/env bash
set -euo pipefail

PROFILE="both"
ARTIFACT_DIR="."
REQUIRE_ARTIFACTS="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="${2:-both}"
      shift 2
      ;;
    --artifacts-dir)
      ARTIFACT_DIR="${2:-.}"
      shift 2
      ;;
    --require-artifacts)
      REQUIRE_ARTIFACTS="true"
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

fail=0
pass() { echo "[PASS] $1"; }
failf() { echo "[FAIL] $1"; fail=1; }
check_file() {
  local p="$1"
  [[ -f "$p" ]] && pass "file exists: $p" || failf "missing file: $p"
}
check_contains() {
  local p="$1"
  local s="$2"
  rg -q -- "$s" "$p" && pass "$p contains: $s" || failf "$p missing: $s"
}

check_file "scripts/mkconfig-legacy.sh"
check_file "scripts/build-profile.sh"
check_file "scripts/mkconfig-tui.sh"
check_file "scripts/prune-isos.sh"

check_contains "scripts/mkconfig-legacy.sh" "ygg-import-zpool-at-boot.service"
check_contains "scripts/mkconfig-legacy.sh" "ygg-lxc-autostart.service"
check_contains "scripts/mkconfig-legacy.sh" "ygg-infisical-ensure.service"
check_contains "scripts/mkconfig-legacy.sh" "/etc/lxc/lxc.conf"
check_contains "scripts/mkconfig-legacy.sh" "/etc/lxc/default.conf"

if [[ "$PROFILE" == "kde" || "$PROFILE" == "both" ]]; then
  check_contains "scripts/mkconfig-legacy.sh" "--with-kde"
fi
if [[ "$PROFILE" == "server" || "$PROFILE" == "both" ]]; then
  check_contains "scripts/mkconfig-legacy.sh" "nvidia-firstboot.service"
fi

if [[ "$REQUIRE_ARTIFACTS" == "true" ]]; then
  if [[ "$PROFILE" == "server" || "$PROFILE" == "both" ]]; then
    if ls "$ARTIFACT_DIR"/yggdrasil-*-amd64.hybrid.iso >/dev/null 2>&1; then
      pass "server iso artifact exists"
    else
      failf "server iso artifact missing"
    fi
  fi

  if [[ "$PROFILE" == "kde" || "$PROFILE" == "both" ]]; then
    if ls "$ARTIFACT_DIR"/yggdrasil-*-kde-amd64.hybrid.iso >/dev/null 2>&1; then
      pass "kde iso artifact exists"
    else
      failf "kde iso artifact missing"
    fi
  fi
fi

if [[ $fail -ne 0 ]]; then
  echo "Smoke tests failed"
  exit 1
fi

echo "Smoke tests passed"
