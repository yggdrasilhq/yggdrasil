#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./tests/smoke/boot-qemu.sh --iso PATH [--timeout-sec 180]
USAGE
}

ISO=""
TIMEOUT_SEC="180"
WORKDIR="$(mktemp -d)"
LOGFILE="$WORKDIR/serial.log"
PIDFILE="$WORKDIR/qemu.pid"

cleanup() {
  if [[ -f "$PIDFILE" ]]; then
    pid=$(cat "$PIDFILE" || true)
    if [[ -n "${pid:-}" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" || true
      sleep 1
      kill -9 "$pid" 2>/dev/null || true
    fi
  fi
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iso)
      ISO="${2:-}"
      shift 2
      ;;
    --timeout-sec)
      TIMEOUT_SEC="${2:-180}"
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

if [[ -z "$ISO" || ! -f "$ISO" ]]; then
  echo "ISO not found: $ISO" >&2
  exit 1
fi

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
  echo "Missing qemu-system-x86_64" >&2
  exit 1
fi

QEMU_ACCEL=()
if [[ -r /dev/kvm ]]; then
  QEMU_ACCEL=(-enable-kvm)
else
  echo "KVM unavailable; using TCG emulation (slower)."
fi

qemu-system-x86_64 \
  -m 4096 \
  -smp 2 \
  "${QEMU_ACCEL[@]}" \
  -boot d \
  -cdrom "$ISO" \
  -display none \
  -serial file:"$LOGFILE" \
  -no-reboot \
  >/dev/null 2>&1 &

qpid=$!
echo "$qpid" > "$PIDFILE"

deadline=$((SECONDS + TIMEOUT_SEC))
ok=0
while (( SECONDS < deadline )); do
  if ! kill -0 "$qpid" 2>/dev/null; then
    ok=1
    break
  fi

  if [[ -f "$LOGFILE" ]]; then
    if rg -qi "(debian|login:|started|welcome)" "$LOGFILE"; then
      ok=1
      break
    fi
  fi

  sleep 3
done

if [[ $ok -ne 1 ]]; then
  echo "QEMU boot smoke did not reach expected serial markers in ${TIMEOUT_SEC}s" >&2
  exit 1
fi

echo "QEMU boot smoke passed: $ISO"
