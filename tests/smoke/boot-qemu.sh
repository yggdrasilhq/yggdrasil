#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./tests/smoke/boot-qemu.sh --iso PATH [options]

Options:
  --timeout-sec N      Boot timeout in seconds (default: 180)
  --memory-mb N        RAM size in MB (default: 8192)
  --smp N              vCPU count (default: 4)
  --ssh-port N         Host forwarded SSH port (default: 2222)
  --min-uptime-sec N   Consider VM booted if alive this long (default: 90)
USAGE
}

ISO=""
TIMEOUT_SEC="180"
MEMORY_MB="8192"
SMP="4"
SSH_PORT="2222"
MIN_UPTIME_SEC="90"
WORKDIR="$(mktemp -d)"
LOGFILE="$WORKDIR/serial.log"
ERRLOG="$WORKDIR/qemu.stderr.log"
PIDFILE="$WORKDIR/qemu.pid"
DISKFILE="$WORKDIR/vm.qcow2"
OVMF_VARS_LOCAL="$WORKDIR/OVMF_VARS.fd"

cleanup() {
  local preserve="${YGG_KEEP_QEMU_LOGS:-false}"
  if [[ -f "$PIDFILE" ]]; then
    pid=$(cat "$PIDFILE" || true)
    if [[ -n "${pid:-}" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" || true
      sleep 1
      kill -9 "$pid" 2>/dev/null || true
    fi
  fi
  if [[ "$preserve" == "true" ]]; then
    echo "Preserving QEMU workdir: $WORKDIR"
  else
    rm -rf "$WORKDIR"
  fi
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
    --memory-mb)
      MEMORY_MB="${2:-8192}"
      shift 2
      ;;
    --smp)
      SMP="${2:-4}"
      shift 2
      ;;
    --ssh-port)
      SSH_PORT="${2:-2222}"
      shift 2
      ;;
    --min-uptime-sec)
      MIN_UPTIME_SEC="${2:-90}"
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

for dev in /dev/kvm /dev/vhost-net /dev/net/tun; do
  if [[ ! -e "$dev" ]]; then
    echo "Missing required device: $dev" >&2
    echo "See /root/qemu_kvm.md for passthrough requirements." >&2
    exit 1
  fi
done

for cmd in qemu-system-x86_64 qemu-img; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing command: $cmd" >&2
    echo "Install prerequisites from /root/qemu_kvm.md" >&2
    exit 1
  fi
done

find_ovmf_code() {
  for p in \
    /usr/share/OVMF/OVMF_CODE.fd \
    /usr/share/OVMF/OVMF_CODE_4M.fd \
    /usr/share/OVMF/OVMF_CODE_4M.secboot.fd \
    /usr/share/ovmf/OVMF.fd \
    /usr/share/edk2/ovmf/OVMF_CODE.fd; do
    [[ -f "$p" ]] && { echo "$p"; return 0; }
  done
  return 1
}

find_ovmf_vars() {
  for p in \
    /usr/share/OVMF/OVMF_VARS.fd \
    /usr/share/OVMF/OVMF_VARS_4M.fd \
    /usr/share/OVMF/OVMF_VARS_4M.ms.fd \
    /usr/share/ovmf/OVMF_VARS.fd \
    /usr/share/edk2/ovmf/OVMF_VARS.fd; do
    [[ -f "$p" ]] && { echo "$p"; return 0; }
  done
  return 1
}

OVMF_CODE="$(find_ovmf_code || true)"
OVMF_VARS_TEMPLATE="$(find_ovmf_vars || true)"
if [[ -z "$OVMF_CODE" || -z "$OVMF_VARS_TEMPLATE" ]]; then
  echo "OVMF firmware files not found." >&2
  echo "Install prerequisites from /root/qemu_kvm.md" >&2
  exit 1
fi

cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS_LOCAL"
qemu-img create -f qcow2 "$DISKFILE" 24G >/dev/null

is_port_in_use() {
  local port="$1"
  if command -v ss >/dev/null 2>&1; then
    ss -ltn "( sport = :$port )" 2>/dev/null | rg -q ":$port\\b"
  elif command -v lsof >/dev/null 2>&1; then
    lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
  else
    return 1
  fi
}

resolve_ssh_port() {
  local requested="$1"
  local p="$requested"
  local tries=30
  local i
  for i in $(seq 1 "$tries"); do
    if ! is_port_in_use "$p"; then
      echo "$p"
      return 0
    fi
    p=$((p + 1))
  done
  return 1
}

SSH_PORT_RESOLVED="$(resolve_ssh_port "$SSH_PORT" || true)"
if [[ -z "$SSH_PORT_RESOLVED" ]]; then
  echo "Could not find a free host SSH forward port starting from $SSH_PORT" >&2
  exit 1
fi
if [[ "$SSH_PORT_RESOLVED" != "$SSH_PORT" ]]; then
  echo "Requested SSH port $SSH_PORT is busy; using $SSH_PORT_RESOLVED instead"
fi

qemu-system-x86_64 \
  -enable-kvm \
  -machine q35 \
  -cpu host \
  -smp "$SMP" \
  -m "$MEMORY_MB" \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS_LOCAL" \
  -drive file="$DISKFILE",if=virtio,format=qcow2 \
  -drive file="$ISO",media=cdrom,if=ide,readonly=on \
  -netdev user,id=n1,hostfwd=tcp::"$SSH_PORT_RESOLVED"-:22 \
  -device virtio-net-pci,netdev=n1 \
  -display none \
  -serial file:"$LOGFILE" \
  -no-reboot \
  >"$ERRLOG" 2>&1 &

qpid=$!
echo "$qpid" > "$PIDFILE"

deadline=$((SECONDS + TIMEOUT_SEC))
ok=0
first_alive_mark=0
while (( SECONDS < deadline )); do
  if ! kill -0 "$qpid" 2>/dev/null; then
    echo "QEMU exited before timeout; check serial log: $LOGFILE" >&2
    break
  fi

  if [[ $first_alive_mark -eq 0 ]]; then
    first_alive_mark=$SECONDS
  fi

  if [[ -f "$LOGFILE" ]]; then
    if rg -qi "(kernel panic|dracut.*error|failed to mount|emergency mode)" "$LOGFILE"; then
      echo "Detected fatal boot marker in serial log" >&2
      break
    fi

    if rg -qi "(grub|debian|yggdrasil|login:|started|live)" "$LOGFILE"; then
      ok=1
      break
    fi
  fi

  if (( SECONDS - first_alive_mark >= MIN_UPTIME_SEC )); then
    ok=1
    break
  fi

  sleep 3
done

if [[ $ok -ne 1 ]]; then
  echo "QEMU boot smoke did not reach expected markers or stable uptime in ${TIMEOUT_SEC}s" >&2
  echo "Serial log: $LOGFILE" >&2
  if [[ -s "$ERRLOG" ]]; then
    echo "QEMU stderr tail:" >&2
    tail -n 20 "$ERRLOG" >&2
  fi
  exit 1
fi

echo "QEMU boot smoke passed: $ISO"
echo "SSH forward available on localhost:$SSH_PORT_RESOLVED"
