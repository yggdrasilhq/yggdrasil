#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./tests/smoke/boot-qemu.sh --iso PATH [options]

Options:
  --mode server|kde   Profile mode for in-guest checks (default: server)
  --timeout-sec N      Boot timeout in seconds (default: 180)
  --memory-mb N        RAM size in MB (default: 8192)
  --smp N              vCPU count (default: 4)
  --ssh-port N         Host forwarded SSH port (default: 2222)
  --ssh-private-key P  Private key for root SSH into guest (or YGG_QEMU_SSH_PRIVATE_KEY)
  --min-uptime-sec N   Consider VM booted if alive this long (default: 90)
USAGE
}

ISO=""
MODE="server"
TIMEOUT_SEC="180"
MEMORY_MB="8192"
SMP="4"
SSH_PORT="2222"
SSH_PRIVATE_KEY="${YGG_QEMU_SSH_PRIVATE_KEY:-}"
SSH_USER="root"
MIN_UPTIME_SEC="90"
WORKDIR="$(mktemp -d)"
LOGFILE="$WORKDIR/serial.log"
ERRLOG="$WORKDIR/qemu.stderr.log"
PIDFILE="$WORKDIR/qemu.pid"
DISKFILE="$WORKDIR/vm.qcow2"
OVMF_VARS_LOCAL="$WORKDIR/OVMF_VARS.fd"
GUEST_SMOKE_SCRIPT="$WORKDIR/guest-smoke.sh"

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
    --mode)
      MODE="${2:-server}"
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
    --ssh-private-key)
      SSH_PRIVATE_KEY="${2:-}"
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

case "$MODE" in
  server|kde) ;;
  *)
    echo "Invalid --mode: $MODE (expected server|kde)" >&2
    exit 1
    ;;
esac

if [[ -z "$SSH_PRIVATE_KEY" || ! -f "$SSH_PRIVATE_KEY" ]]; then
  echo "Missing SSH private key for in-guest smoke: $SSH_PRIVATE_KEY" >&2
  echo "Pass --ssh-private-key or set YGG_QEMU_SSH_PRIVATE_KEY." >&2
  exit 1
fi

for dev in /dev/kvm /dev/vhost-net /dev/net/tun; do
  if [[ ! -e "$dev" ]]; then
    echo "Missing required device: $dev" >&2
    echo "See /root/qemu_kvm.md for passthrough requirements." >&2
    exit 1
  fi
done

for cmd in qemu-system-x86_64 qemu-img ssh; do
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

SSH_OPTS=(
  -o BatchMode=yes
  -o StrictHostKeyChecking=no
  -o UserKnownHostsFile=/dev/null
  -o ConnectTimeout=5
  -i "$SSH_PRIVATE_KEY"
  -p "$SSH_PORT_RESOLVED"
)
SSH_TARGET="${SSH_USER}@127.0.0.1"

ssh_ready=0
ssh_deadline=$((SECONDS + TIMEOUT_SEC))
while (( SECONDS < ssh_deadline )); do
  if ! kill -0 "$qpid" 2>/dev/null; then
    echo "QEMU exited before SSH became reachable; serial log: $LOGFILE" >&2
    break
  fi
  if ssh "${SSH_OPTS[@]}" "$SSH_TARGET" "true" >/dev/null 2>&1; then
    ssh_ready=1
    break
  fi
  sleep 3
done

if [[ $ssh_ready -ne 1 ]]; then
  echo "SSH did not become reachable on localhost:$SSH_PORT_RESOLVED" >&2
  echo "Serial log: $LOGFILE" >&2
  exit 1
fi

cat > "$GUEST_SMOKE_SCRIPT" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

profile="${1:-server}"

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing command in guest: $1" >&2
    exit 1
  }
}

require_service_healthy() {
  local svc="$1"
  local load_state active_state
  load_state="$(systemctl show -p LoadState --value "$svc" || true)"
  [[ "$load_state" == "loaded" ]] || {
    echo "Service not loaded: $svc (LoadState=$load_state)" >&2
    exit 1
  }
  systemctl is-enabled "$svc" >/dev/null 2>&1 || {
    echo "Service not enabled: $svc" >&2
    exit 1
  }
  active_state="$(systemctl show -p ActiveState --value "$svc" || true)"
  [[ "$active_state" != "failed" ]] || {
    echo "Service failed: $svc" >&2
    exit 1
  }
}

require_service_timeout_finite() {
  local svc="$1"
  local timeout
  timeout="$(systemctl show -p TimeoutStartUSec --value "$svc" || true)"
  [[ -n "$timeout" && "$timeout" != "infinity" ]] || {
    echo "Service has infinite startup timeout: $svc" >&2
    exit 1
  }
}

need_cmd systemctl
need_cmd grep
need_cmd awk
need_cmd zpool
need_cmd zfs
need_cmd modprobe
need_cmd lsmod
need_cmd lxc-ls
need_cmd lxc-start
need_cmd lxc-attach
need_cmd codex
need_cmd codex-litellm
need_cmd codex-session-tui

require_service_healthy ygg-import-zpool-at-boot.service
require_service_healthy ygg-lxc-autostart.service
require_service_healthy ygg-infisical-ensure.service
require_service_timeout_finite ygg-lxc-autostart.service
require_service_timeout_finite ygg-infisical-ensure.service

failed_ygg_units="$(systemctl --failed --no-legend 2>/dev/null | awk '{print $1}' | grep '^ygg-' || true)"
[[ -z "$failed_ygg_units" ]] || {
  echo "Some ygg* units are failed:" >&2
  echo "$failed_ygg_units" >&2
  exit 1
}

[[ -s /etc/lxc/lxc.conf ]] || { echo "/etc/lxc/lxc.conf missing/empty" >&2; exit 1; }
[[ -s /etc/lxc/default.conf ]] || { echo "/etc/lxc/default.conf missing/empty" >&2; exit 1; }
[[ -s /etc/default/ygg-infisical-ensure ]] || { echo "/etc/default/ygg-infisical-ensure missing/empty" >&2; exit 1; }
grep -q 'YGG_INFISICAL_BOOT_MODE=' /etc/default/ygg-infisical-ensure
grep -q 'YGG_POST_BOOT_HOOK' /usr/local/sbin/ygg-ensure-infisical
! grep -q 'update-stack.sh' /usr/local/sbin/ygg-ensure-infisical
grep -q '^lxc.net.0.type = macvlan' /etc/lxc/default.conf
grep -q '^lxc.net.0.macvlan.mode = bridge' /etc/lxc/default.conf
grep -q '^lxc.apparmor.profile = generated' /etc/lxc/default.conf

modprobe zfs
lsmod | awk '{print $1}' | grep -qx zfs
[[ -c /dev/zfs ]] || { echo "/dev/zfs missing" >&2; exit 1; }
zpool --version >/dev/null
zfs --version >/dev/null 2>&1 || zfs version >/dev/null

pool="yggsmoke"
img="/root/ygg-smoke-zpool.img"
mount_base="/mnt/ygg-smoke"
cleanup_pool() {
  zpool destroy -f "$pool" >/dev/null 2>&1 || true
  rm -f "$img"
}
trap cleanup_pool EXIT

truncate -s 256M "$img" 2>/dev/null || dd if=/dev/zero of="$img" bs=1M count=256 status=none
mkdir -p "$mount_base"
zpool create -f -m "$mount_base" "$pool" "$img"
zfs create -o mountpoint="$mount_base/test" "$pool/test"
echo "smoke-ok" > "$mount_base/test/probe.txt"
grep -q 'smoke-ok' "$mount_base/test/probe.txt"
zpool status "$pool" >/dev/null
zpool destroy -f "$pool"
rm -f "$img"

if [[ "$profile" == "kde" ]]; then
  need_cmd sddm
  need_cmd startplasma-x11
  systemctl is-enabled sddm.service >/dev/null 2>&1 || {
    echo "sddm.service is not enabled" >&2
    exit 1
  }
  systemctl show -p LoadState --value sddm.service | grep -q '^loaded$'
  if ! compgen -G "/usr/share/xsessions/plasma*.desktop" >/dev/null && \
     ! compgen -G "/usr/share/wayland-sessions/plasma*.desktop" >/dev/null; then
    echo "Missing Plasma session desktop file(s)" >&2
    exit 1
  fi
else
  systemctl show -p LoadState --value nvidia-firstboot.service >/dev/null 2>&1 || {
    echo "nvidia-firstboot.service missing in server profile" >&2
    exit 1
  }
fi
EOF
chmod 0755 "$GUEST_SMOKE_SCRIPT"

ssh "${SSH_OPTS[@]}" "$SSH_TARGET" "bash -s -- $MODE" < "$GUEST_SMOKE_SCRIPT"

echo "QEMU boot smoke passed: $ISO"
echo "SSH forward available on localhost:$SSH_PORT_RESOLVED"
