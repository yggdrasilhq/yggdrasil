#!/usr/bin/env bash
set -euo pipefail

PROFILE="both"
ARTIFACT_DIR="."
REQUIRE_ARTIFACTS="false"
WITH_ISO_ROOTFS="false"
WITH_QEMU_BOOT="false"
SERVER_ISO=""
KDE_ISO=""

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
    --with-iso-rootfs)
      WITH_ISO_ROOTFS="true"
      shift
      ;;
    --with-qemu-boot)
      WITH_QEMU_BOOT="true"
      shift
      ;;
    --server-iso)
      SERVER_ISO="${2:-}"
      shift 2
      ;;
    --kde-iso)
      KDE_ISO="${2:-}"
      shift 2
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

case "$PROFILE" in
  server|kde|both) ;;
  *)
    echo "Invalid --profile: $PROFILE" >&2
    exit 1
    ;;
esac

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

check_file "scripts/mkconfig-core.sh"
check_file "scripts/build-profile.sh"
check_file "scripts/prune-isos.sh"
check_file "tests/smoke/iso-rootfs-check.sh"
check_file "tests/smoke/boot-qemu.sh"
check_file "ygg.example.toml"

check_contains "scripts/mkconfig-core.sh" "ygg-import-zpool-at-boot.service"
check_contains "scripts/mkconfig-core.sh" "ygg-lxc-autostart.service"
check_contains "scripts/mkconfig-core.sh" "ygg-infisical-ensure.service"
check_contains "scripts/mkconfig-core.sh" "/etc/lxc/lxc.conf"
check_contains "scripts/mkconfig-core.sh" "/etc/lxc/default.conf"
check_contains "scripts/mkconfig-core.sh" "YGG_SSH_AUTHORIZED_KEYS_FILE"
check_contains "scripts/mkconfig-core.sh" "YGG_STATIC_IP"
check_contains "ygg.example.toml" "build_profile = \"both\""
check_contains "ygg.example.toml" "setup_mode = \"recommended\""

if [[ "$PROFILE" == "kde" || "$PROFILE" == "both" ]]; then
  check_contains "scripts/mkconfig-core.sh" "--with-kde"
fi
if [[ "$PROFILE" == "server" || "$PROFILE" == "both" ]]; then
  check_contains "scripts/mkconfig-core.sh" "nvidia-firstboot.service"
fi

if [[ "$REQUIRE_ARTIFACTS" == "true" ]]; then
  if [[ "$PROFILE" == "server" || "$PROFILE" == "both" ]]; then
    if [[ -n "$SERVER_ISO" && -f "$SERVER_ISO" ]]; then
      pass "server iso artifact exists (explicit path)"
    elif [[ -f "$ARTIFACT_DIR/server-latest.iso" ]]; then
      pass "server iso artifact exists (server-latest.iso)"
    elif ls "$ARTIFACT_DIR"/yggdrasil-*-amd64.hybrid.iso >/dev/null 2>&1; then
      pass "server iso artifact exists"
    else
      failf "server iso artifact missing"
    fi
  fi

  if [[ "$PROFILE" == "kde" || "$PROFILE" == "both" ]]; then
    if [[ -n "$KDE_ISO" && -f "$KDE_ISO" ]]; then
      pass "kde iso artifact exists (explicit path)"
    elif [[ -f "$ARTIFACT_DIR/kde-latest.iso" ]]; then
      pass "kde iso artifact exists (kde-latest.iso)"
    elif ls "$ARTIFACT_DIR"/yggdrasil-*-kde-amd64.hybrid.iso >/dev/null 2>&1; then
      pass "kde iso artifact exists"
    else
      failf "kde iso artifact missing"
    fi
  fi
fi

latest_iso() {
  local mode="$1"
  if [[ "$mode" == "server" ]]; then
    if [[ -f "$ARTIFACT_DIR/server-latest.iso" ]]; then
      echo "$ARTIFACT_DIR/server-latest.iso"
    else
      ls -1t "$ARTIFACT_DIR"/yggdrasil-*-amd64.hybrid.iso 2>/dev/null | rg -v -- '-kde-amd64\.hybrid\.iso$' | head -n1
    fi
  else
    if [[ -f "$ARTIFACT_DIR/kde-latest.iso" ]]; then
      echo "$ARTIFACT_DIR/kde-latest.iso"
    else
      ls -1t "$ARTIFACT_DIR"/yggdrasil-*-kde-amd64.hybrid.iso 2>/dev/null | head -n1
    fi
  fi
}

if [[ -z "$SERVER_ISO" ]]; then
  SERVER_ISO="$(latest_iso server || true)"
fi
if [[ -z "$KDE_ISO" ]]; then
  KDE_ISO="$(latest_iso kde || true)"
fi

run_optional_checks() {
  local mode="$1"
  local iso="$2"

  if [[ -z "$iso" || ! -f "$iso" ]]; then
    failf "${mode} iso not found for optional checks"
    return
  fi

  if [[ "$WITH_ISO_ROOTFS" == "true" ]]; then
    if [[ "$mode" == "kde" ]]; then
      ./tests/smoke/iso-rootfs-check.sh --iso "$iso" --expect-kde || fail=1
    else
      ./tests/smoke/iso-rootfs-check.sh --iso "$iso" || fail=1
    fi
  fi

  if [[ "$WITH_QEMU_BOOT" == "true" ]]; then
    if [[ -z "${YGG_QEMU_SSH_PRIVATE_KEY:-}" ]]; then
      echo "[FAIL] YGG_QEMU_SSH_PRIVATE_KEY is required for QEMU guest SSH smoke" >&2
      fail=1
    else
      ./tests/smoke/boot-qemu.sh --iso "$iso" --mode "$mode" --ssh-private-key "$YGG_QEMU_SSH_PRIVATE_KEY" || fail=1
    fi
  fi
}

if [[ "$WITH_ISO_ROOTFS" == "true" || "$WITH_QEMU_BOOT" == "true" ]]; then
  if [[ "$PROFILE" == "server" || "$PROFILE" == "both" ]]; then
    run_optional_checks "server" "$SERVER_ISO"
  fi
  if [[ "$PROFILE" == "kde" || "$PROFILE" == "both" ]]; then
    run_optional_checks "kde" "$KDE_ISO"
  fi
fi

if [[ $fail -ne 0 ]]; then
  echo "Smoke tests failed"
  exit 1
fi

echo "Smoke tests passed"
