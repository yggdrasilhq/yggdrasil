#!/usr/bin/env bash
set -euo pipefail

# ventoy-inject.sh: ship freshly built yggdrasil ISOs to the Ventoy stick.
#
# Contract (full spec in docs/ventoy-injection.md):
#   - copy timestamped yggdrasil-*.hybrid.iso files from iso_source_dir
#     that the stick does not have yet;
#   - retention is DATE-AWARE: the last-working keep_previous_count ISOs
#     per profile are always from a date BEFORE today, so a dev day full
#     of fresh builds can never rotate the real fallback copies off the
#     stick;
#   - pin the Ventoy boot-menu default (VTOY_DEFAULT_IMAGE) to the
#     default_rank-th latest yggdrasil ISO on the stick.
#
# Config: ventoy.local.toml (gitignored, create from ventoy.example.toml).
# The stick may live on another host (target_host); that host needs ssh
# access, lsblk, find, mount and a kernel that mounts the stick's exfat.

usage() {
  cat <<'USAGE'
Usage: ./scripts/ventoy-inject.sh [options]

Options:
  --config PATH   Config file (.toml). Default: ./ventoy.local.toml.
  --dry-run       Read-only planning pass. Reads the stick for real (it
                  must already be mounted), fakes every mutation.
  -h, --help      Show this help.
USAGE
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

CONFIG=""
DRY_RUN="false"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --config) CONFIG="${2:-}"; shift 2 ;;
    --dry-run) DRY_RUN="true"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

die() { echo "ventoy-inject: $*" >&2; exit 1; }

if [[ -z "$CONFIG" ]]; then
  CONFIG="./ventoy.local.toml"
fi
if [[ ! -f "$CONFIG" ]]; then
  echo "Config file not found: $CONFIG" >&2
  echo "Create it from ./ventoy.example.toml and re-run." >&2
  exit 1
fi

JSON_TMP=""
CONFIG_ENV="$(mktemp /tmp/ygg-ventoy-XXXXXX.env)"
trap 'rm -f "$CONFIG_ENV" "$JSON_TMP" 2>/dev/null' EXIT
"$SCRIPT_DIR/toml-to-env.sh" "$CONFIG" > "$CONFIG_ENV"
# Exported on purpose: the YGG_VENTOY_* control scan below reads the env.
set -a
# shellcheck disable=SC1090
source "$CONFIG_ENV"
set +a

TARGET_HOST="${YGG_TARGET_HOST:-}"
USB_MOUNT="${YGG_USB_MOUNT:-/mnt/ventoy}"
USB_LABEL="${YGG_USB_LABEL:-Ventoy}"
USB_DEVICE="${YGG_USB_DEVICE:-}"
SKIP_MOUNT="${YGG_SKIP_MOUNT:-false}"
ISO_SOURCE_DIR="${YGG_ISO_SOURCE_DIR:-.}"
ISO_STICK_DIR="${YGG_ISO_STICK_DIR:-/}"
KEEP_PREVIOUS="${YGG_KEEP_PREVIOUS_COUNT:-2}"
DEFAULT_RANK="${YGG_DEFAULT_RANK:-3}"
MIN_FREE_MB="${YGG_MIN_FREE_MB:-4096}"
UNMOUNT_AFTER="${YGG_UNMOUNT_AFTER:-true}"

# Stick-side ISO directory, "/yggdrasil" style (no trailing slash; "/" = root).
STICK_DIR="${ISO_STICK_DIR%/}"
[[ -z "$STICK_DIR" ]] && STICK_DIR="/"
if [[ "$STICK_DIR" == "/" ]]; then
  STICK_DIR_PATH="$USB_MOUNT"
  DEFAULT_IMAGE_BASE=""
else
  STICK_DIR_PATH="$USB_MOUNT$STICK_DIR"
  DEFAULT_IMAGE_BASE="$STICK_DIR"
fi

# Any YGG_VENTOY_* config key becomes an extra Ventoy control entry, so a
# stick's existing behaviour (menu timeout, secondary menu, ...) survives
# the rewrite. ventoy_menu_timeout = "10" -> { "VTOY_MENU_TIMEOUT": "10" }.
EXTRA_CONTROL=""
while IFS='=' read -r k v; do
  [[ -n "${k:-}" ]] || continue
  [[ "$k" == YGG_VENTOY_* ]] || continue
  key="VTOY_${k#YGG_VENTOY_}"
  [[ "$v" == *'"'* ]] && die "ventoy control value must not contain double quotes: $k=$v"
  EXTRA_CONTROL+="        { \"$key\": \"$v\" },"$'\n'
done < <(env | sort)

[[ "$KEEP_PREVIOUS" =~ ^[0-9]+$ ]] || die "keep_previous_count must be a number, got: $KEEP_PREVIOUS"
[[ "$DEFAULT_RANK" =~ ^[0-9]+$ ]] || die "default_rank must be a number, got: $DEFAULT_RANK"
[[ "$DEFAULT_RANK" -ge 1 ]] || die "default_rank must be >= 1"
[[ -d "$ISO_SOURCE_DIR" ]] || die "iso_source_dir does not exist: $ISO_SOURCE_DIR"
case "$SKIP_MOUNT" in true|false) ;; *) die "skip_mount must be true or false" ;; esac
case "$UNMOUNT_AFTER" in true|false) ;; *) die "unmount_after must be true or false" ;; esac

# probe(): always executes (reads are safe). act(): faked under --dry-run.
if [[ -n "$TARGET_HOST" ]]; then
  probe() { ssh "$TARGET_HOST" "$@"; }
  push_file() { scp -q "$1" "$TARGET_HOST:$2"; }
else
  probe() { "$@"; }
  push_file() { cp -f "$1" "$2"; }
fi
if [[ "$DRY_RUN" == "true" ]]; then
  act() { printf '[dry-run]'; printf ' %q' "$@"; printf '\n'; }
  push_file() { printf '[dry-run] copy %s -> %s\n' "$1" "$2"; }
else
  act() { "$@"; }
fi

# -- stick plumbing ---------------------------------------------------------

ISO_NAME_RE='^yggdrasil-[0-9]{12}-[a-z0-9.-]+\.hybrid\.iso$'
TS_RE='^[0-9]{12}$'

find_part() {
  if [[ -n "$USB_DEVICE" ]]; then
    printf '%s\n' "$USB_DEVICE"
    return
  fi
  probe lsblk -rno NAME,LABEL,FSTYPE,SIZE \
    | awk -v lbl="$USB_LABEL" '$2 == lbl && $3 != "vfat" { print "/dev/" $1; exit }'
}

ensure_mounted() {
  if [[ "$SKIP_MOUNT" == "true" ]]; then
    [[ -d "$USB_MOUNT" ]] || die "skip_mount=true but usb_mount is not a directory: $USB_MOUNT"
    return
  fi
  local part
  part="$(find_part)"
  [[ -n "$part" ]] || die "no partition labelled '$USB_LABEL' found (set usb_device to force one)"
  echo "Ventoy data partition: $part"
  if probe mountpoint -q "$USB_MOUNT"; then
    echo "Already mounted at: $USB_MOUNT"
  else
    act mkdir -p "$USB_MOUNT"
    if ! act mount "$part" "$USB_MOUNT"; then
      die "mount $part failed. If the target host is live-booted FROM this
  stick, the Ventoy runtime holds the partition behind device-mapper:
  mount the dm passthrough instead (mknod the missing /dev/mapper/<dm>
  node from 'dmsetup table', or set usb_device to it and skip_mount=true)."
    fi
    echo "Mounted $part at $USB_MOUNT"
  fi
}

# One yggdrasil ISO name per line, from the stick.
stick_list() {
  probe find "$STICK_DIR_PATH" -maxdepth 1 -name 'yggdrasil-*.hybrid.iso' -printf '%f\n' | sort
}

# name -> 12-digit build timestamp on stdout; fails when the name has none.
iso_ts() {
  local ts
  ts="$(printf '%s' "$1" | sed -nE 's/^yggdrasil-([0-9]{12})-.+/\1/p')"
  [[ "$ts" =~ $TS_RE ]] || return 1
  printf '%s\n' "$ts"
}

# stdin: ISO names, newest first. stdout: "keep <name>" / "delete <name>"
# / "keep-unknown <name>" lines. The retention law: everything from today
# stays, then the first KEEP_PREVIOUS strictly-older builds stay, the rest
# go. ISOs without a parseable timestamp are never touched.
plan_retention() {
  local today keep_prev name ts tsdate
  today="$(date +%Y%m%d)"
  keep_prev=0
  while IFS= read -r name; do
    [[ -n "$name" ]] || continue
    if ! ts="$(iso_ts "$name")"; then
      echo "keep-unknown $name"
      continue
    fi
    tsdate="${ts:0:8}"
    if [[ "$tsdate" == "$today" || "$tsdate" > "$today" ]]; then
      echo "keep $name"
    elif [[ "$keep_prev" -lt "$KEEP_PREVIOUS" ]]; then
      echo "keep $name"
      keep_prev=$((keep_prev + 1))
    else
      echo "delete $name"
    fi
  done
}

stick_free_mb() {
  probe df -Pm "$USB_MOUNT" | awk 'NR == 2 { print $4 }'
}

# -- run --------------------------------------------------------------------

ensure_mounted

# The stick-side ISO directory must exist before anything lists or copies.
act mkdir -p "$STICK_DIR_PATH"

STICK_BEFORE="$(stick_list | sort -u)"
if [[ -z "$STICK_BEFORE" ]]; then
  echo "note: no yggdrasil ISOs on the stick yet"
fi

declare -A ON_STICK=()
while IFS= read -r n; do [[ -n "$n" ]] && ON_STICK["$n"]=1; done <<< "$STICK_BEFORE"

# 1. Copy ISOs the stick lacks (plus their .sha256 sidecars).
shopt -s nullglob
copied=0
for src in "$ISO_SOURCE_DIR"/yggdrasil-*.hybrid.iso; do
  name="$(basename "$src")"
  [[ "$name" =~ $ISO_NAME_RE ]] || { echo "skip (not a timestamped build ISO): $name"; continue; }
  if [[ -n "${ON_STICK[$name]:-}" ]]; then
    echo "already on stick: $name"
    continue
  fi
  size_mb=$(( $(stat -c %s "$src") / 1048576 ))
  free_mb="$(stick_free_mb)"
  if (( free_mb < size_mb + MIN_FREE_MB )); then
    die "stick too full for $name: ${free_mb}MB free, need ${size_mb}MB + ${MIN_FREE_MB}MB headroom"
  fi
  act push_file "$src" "$STICK_DIR_PATH/$name"
  if [[ -f "$src.sha256" ]]; then
    act push_file "$src.sha256" "$STICK_DIR_PATH/$name.sha256"
  fi
  echo "inject: $name (${size_mb}MB)"
  copied=$((copied + 1))
  ON_STICK["$name"]=1
done
shopt -u nullglob
[[ "$copied" -eq 0 ]] && echo "nothing new to inject"

# 2. Date-aware retention, per profile group (server and kde independently).
TO_DELETE=()
for group_list in "$(printf '%s\n' "$STICK_BEFORE" | grep -v -- '-kde-' || true)" \
                  "$(printf '%s\n' "$STICK_BEFORE" | grep -- '-kde-' || true)"; do
  [[ -n "$(printf '%s' "$group_list" | tr -d '[:space:]')" ]] || continue
  while IFS= read -r line; do
    case "$line" in
      delete\ *)
        victim="${line#delete }"
        [[ "$victim" =~ $ISO_NAME_RE ]] || die "refusing to delete non-ISO name: $victim"
        TO_DELETE+=("$victim")
        ;;
    esac
  done < <(printf '%s\n' "$group_list" | sort -r | plan_retention)
done
delete_count=0
for victim in ${TO_DELETE[@]+"${TO_DELETE[@]}"}; do
  act rm -f "$STICK_DIR_PATH/$victim" "$STICK_DIR_PATH/$victim.sha256"
  if [[ "$DRY_RUN" != "true" ]]; then
    echo "prune: $victim"
  fi
  delete_count=$((delete_count + 1))
done

# 3. Boot-menu default: the default_rank-th latest ISO now on the stick.
if [[ "$DRY_RUN" == "true" ]]; then
  # Simulate the post-run inventory: before-set plus copies minus deletes.
  declare -A SIM=()
  while IFS= read -r n; do [[ -n "$n" ]] && SIM["$n"]=1; done <<< "$STICK_BEFORE"
  for n in "${!ON_STICK[@]}"; do SIM["$n"]=1; done
  for n in ${TO_DELETE[@]+"${TO_DELETE[@]}"}; do unset "SIM[$n]"; done
  STICK_AFTER="$(printf '%s\n' "${!SIM[@]}" | sort -r)"
else
  STICK_AFTER="$(stick_list | sort -r)"
fi

default_iso=""
rank=0
while IFS= read -r n; do
  [[ -n "$n" ]] || continue
  # Only timestamped build ISOs count for the rank; stray names on the
  # stick are never deleted, but they never hijack the default either.
  iso_ts "$n" >/dev/null || continue
  rank=$((rank + 1))
  if [[ "$rank" -eq "$DEFAULT_RANK" ]]; then default_iso="$n"; break; fi
done <<< "$STICK_AFTER"
if [[ -z "$default_iso" ]]; then
  default_iso="$(printf '%s\n' "$STICK_AFTER" | sed -n '1p')"
fi
[[ -n "$default_iso" ]] || die "no yggdrasil ISO on the stick; nothing to pin as default"

JSON_TMP="$(mktemp /tmp/ygg-ventoy-XXXXXX.json)"
cat > "$JSON_TMP" <<EOF
{
    "control": [
${EXTRA_CONTROL}        { "VTOY_DEFAULT_IMAGE": "$DEFAULT_IMAGE_BASE/$default_iso" }
    ]
}
EOF
if probe test -f "$USB_MOUNT/ventoy/ventoy.json"; then
  act cp -f "$USB_MOUNT/ventoy/ventoy.json" "$USB_MOUNT/ventoy/ventoy.json.bak"
  echo "backed up existing ventoy.json to ventoy.json.bak"
fi
act mkdir -p "$USB_MOUNT/ventoy"
act push_file "$JSON_TMP" "$USB_MOUNT/ventoy/ventoy.json"

if [[ "$UNMOUNT_AFTER" == "true" && "$SKIP_MOUNT" != "true" ]]; then
  act umount "$USB_MOUNT"
  echo "Unmounted $USB_MOUNT"
fi

echo
echo "ventoy-inject summary"
echo "  copied:  $copied"
echo "  pruned:  $delete_count"
echo "  default: $DEFAULT_IMAGE_BASE/$default_iso (rank $DEFAULT_RANK of $(printf '%s\n' "$STICK_AFTER" | grep -c .) on the stick)"
