#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
WORKSPACE_DIR="$ROOT_DIR/yggdrasil-maker"
APP_BIN="$WORKSPACE_DIR/target/debug/yggdrasil-maker"
TMP_DIR="$(mktemp -d)"
LOG_DIR="$TMP_DIR/logs"
APP_HOME="$TMP_DIR/app-home"
SETUPS_DIR="$APP_HOME/setups"
mkdir -p "$LOG_DIR" "$SETUPS_DIR"

APP_PID=""
cleanup() {
  if [[ -n "$APP_PID" ]]; then
    kill "$APP_PID" >/dev/null 2>&1 || true
    wait "$APP_PID" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

fail() {
  printf '[maker-window-regressions] %s\n' "$*" >&2
  exit 1
}

appctl() {
  (
    cd "$WORKSPACE_DIR"
    YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" \
      YGGDRASIL_MAKER_APP_PID="${YGGDRASIL_MAKER_APP_PID:-}" \
      "$APP_BIN" server app "$@"
  )
}

main_window_id_for_pid() {
  local pid="$1"
  local best_id=""
  local best_area=0
  local window_id
  while read -r window_id; do
    [[ -n "$window_id" ]] || continue
    local info
    info="$(xwininfo -id "$window_id" 2>/dev/null || true)"
    [[ "$info" == *"Class: InputOutput"* ]] || continue
    local geometry
    geometry="$(python3 - "$info" <<'PY'
import re
import sys

text = sys.argv[1]
mw = re.search(r"^\s*Width:\s+(\d+)", text, re.M)
mh = re.search(r"^\s*Height:\s+(\d+)", text, re.M)
if not mw or not mh:
    print("")
else:
    print(f"{mw.group(1)} {mh.group(1)}")
PY
)"
    local width height
    read -r width height <<<"$geometry"
    [[ -n "$width" && -n "$height" ]] || continue
    local area=$((width * height))
    if (( area > best_area )); then
      best_area=$area
      best_id="$window_id"
    fi
  done < <(xdotool search --all --pid "$pid" 2>/dev/null || true)
  printf '%s\n' "$best_id"
}

wait_for_window_id() {
  local pid="$1"
  local deadline=$((SECONDS + 20))
  while (( SECONDS < deadline )); do
    local window_id
    window_id="$(main_window_id_for_pid "$pid")"
    if [[ -n "$window_id" ]]; then
      printf '%s\n' "$window_id"
      return 0
    fi
    sleep 0.25
  done
  return 1
}

window_position() {
  local window_id="$1"
  xwininfo -id "$window_id" | python3 -c '
import re, sys
text = sys.stdin.read()
mx = re.search(r"Absolute upper-left X:\s+(-?\d+)", text)
my = re.search(r"Absolute upper-left Y:\s+(-?\d+)", text)
if not mx or not my:
    raise SystemExit(1)
print(f"{mx.group(1)} {my.group(1)}")
'
}

json_eval() {
  local file="$1"
  local code="$2"
  python3 - "$file" "$code" <<'PY'
import json
import pathlib
import sys

data = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
namespace = {"data": data}
exec(sys.argv[2], namespace)
PY
}

wait_for_new_client() {
  local before_file="$1"
  local known_pids
  known_pids="$(json_eval "$before_file" '
print(" ".join(str(item["pid"]) for item in data.get("clients", [])))
')"
  local output="$LOG_DIR/clients-after.json"
  local deadline=$((SECONDS + 30))
  while (( SECONDS < deadline )); do
    appctl clients > "$output" || true
    local pid
    pid="$(json_eval "$output" '
clients = data.get("clients", [])
known = {value for value in """'"$known_pids"'""".split() if value}
for client in clients:
    pid = str(client["pid"])
    if pid not in known:
        print(pid)
        break
else:
    print("")
')"
    if [[ -n "$pid" ]]; then
      printf '%s\n' "$pid"
      return 0
    fi
    sleep 0.25
  done
  return 1
}

printf '[maker-window-regressions] building desktop binary\n'
(cd "$WORKSPACE_DIR" && cargo build -p yggdrasil-maker --features desktop-ui >/dev/null)

printf '[maker-window-regressions] creating isolated saved Build state\n'
(
  cd "$WORKSPACE_DIR"
  YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" \
    "$APP_BIN" setup new \
      --name regression-check \
      --preset nas \
      --profile server \
      --hostname regression-host \
      --output "$LOG_DIR/bootstrap-setup.json" >/dev/null
)
python3 - "$LOG_DIR/bootstrap-setup.json" "$SETUPS_DIR" <<'PY'
import json
import pathlib
import sys

source = pathlib.Path(sys.argv[1])
setups_dir = pathlib.Path(sys.argv[2])
data = json.loads(source.read_text(encoding="utf-8"))
data["journey_stage"] = "review"
target = setups_dir / f'{data["setup"]["name"].lower().replace(" ", "-")}--{data["setup_id"]}.maker.json'
target.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
source.unlink()
PY
rm -f "$APP_HOME/active-build.json"

printf '[maker-window-regressions] launching isolated GUI client\n'
appctl clients > "$LOG_DIR/clients-before.json" || true
(
  cd "$WORKSPACE_DIR"
  exec env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" "$APP_BIN"
) >"$LOG_DIR/app.log" 2>&1 &
APP_PID=$!

CLIENT_PID="$(wait_for_new_client "$LOG_DIR/clients-before.json" || true)"
[[ -n "$CLIENT_PID" ]] || fail "no GUI client appeared"
export YGGDRASIL_MAKER_APP_PID="$CLIENT_PID"

appctl focus --timeout-ms 12000 > "$LOG_DIR/focus.json"
appctl state --timeout-ms 12000 > "$LOG_DIR/state-initial.json"

journey_stage="$(json_eval "$LOG_DIR/state-initial.json" '
print(data["data"]["current_setup"]["journey_stage"])
')"
[[ "$journey_stage" == "Choose" ]] || fail "cold relaunch should reopen on Choose, got $journey_stage"

appctl screenshot "$LOG_DIR/startup-shell.png" --timeout-ms 12000 > "$LOG_DIR/screenshot-startup.json"
python3 - "$LOG_DIR/startup-shell.png" <<'PY'
from PIL import Image
import sys

path = sys.argv[1]
img = Image.open(path).convert("RGBA")
width, height = img.size
left_seam = 8 + 248
right_seam = width - (8 + 318)
sample_ys = [int(height * 0.68), int(height * 0.82)]
threshold = 16

def channel_delta(a, b):
    return max(abs(a[i] - b[i]) for i in range(3))

def patch_color(x, y):
    pixels = []
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            px = min(max(x + dx, 0), width - 1)
            py = min(max(y + dy, 0), height - 1)
            pixels.append(img.getpixel((px, py))[:3])
    return tuple(round(sum(pixel[i] for pixel in pixels) / len(pixels)) for i in range(3))

def seam_jump(seam_x, y):
    xs = [seam_x - 24, seam_x - 16, seam_x - 8, seam_x, seam_x + 8, seam_x + 16, seam_x + 24]
    colors = [patch_color(x, y) for x in xs]
    return max(channel_delta(colors[i], colors[i + 1]) for i in range(len(colors) - 1))

for seam_x in (left_seam, right_seam):
    for y in sample_ys:
        jump = seam_jump(seam_x, y)
        if jump > threshold:
            raise SystemExit(
                f"shell seam too abrupt at x={seam_x}, y={y}, jump={jump}, threshold={threshold}"
            )
PY

printf '[maker-window-regressions] probing stale active-build restore contract\n'
kill "$APP_PID" >/dev/null 2>&1 || true
wait "$APP_PID" >/dev/null 2>&1 || true
APP_PID=""
python3 - "$SETUPS_DIR" "$APP_HOME" <<'PY'
import json
import pathlib
import sys

setups_dir = pathlib.Path(sys.argv[1])
app_home = pathlib.Path(sys.argv[2])
setup_file = next(setups_dir.glob("*.maker.json"))
setup = json.loads(setup_file.read_text(encoding="utf-8"))
record = {
    "setup_id": setup["setup_id"],
    "setup_name": setup["setup"]["name"],
    "setup_path": str(setup_file),
    "artifacts_dir": str(app_home / "artifacts"),
    "repo_root": "/home/pi/gh/yggdrasil",
    "log_path": str(app_home / "build-jobs" / "stale" / "build.log"),
    "completion_path": str(app_home / "build-jobs" / "stale" / "completion.json"),
    "pid": 999999,
    "started_at_ms": 0,
}
(app_home / "build-jobs" / "stale").mkdir(parents=True, exist_ok=True)
(app_home / "active-build.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
PY

appctl clients > "$LOG_DIR/clients-before-relaunch.json" || true
(
  cd "$WORKSPACE_DIR"
  exec env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" "$APP_BIN"
) >"$LOG_DIR/app-relaunch.log" 2>&1 &
APP_PID=$!

CLIENT_PID="$(wait_for_new_client "$LOG_DIR/clients-before-relaunch.json" || true)"
[[ -n "$CLIENT_PID" ]] || fail "no GUI client appeared after stale-build relaunch"
export YGGDRASIL_MAKER_APP_PID="$CLIENT_PID"
appctl state --timeout-ms 12000 > "$LOG_DIR/state-stale-active-build.json"
journey_stage="$(json_eval "$LOG_DIR/state-stale-active-build.json" '
print(data["data"]["current_setup"]["journey_stage"])
')"
[[ "$journey_stage" != "Build" ]] || fail "stale active-build record reopened on Build"

printf '[maker-window-regressions] probing live active-build reattach contract\n'
kill "$APP_PID" >/dev/null 2>&1 || true
wait "$APP_PID" >/dev/null 2>&1 || true
APP_PID=""

mkdir -p "$APP_HOME/build-jobs/live-reattach"
cat > "$APP_HOME/build-jobs/live-reattach/build.log" <<'EOF'
{"type":"stage","status":"started","stage":"preflight"}
EOF
python3 - "$SETUPS_DIR" "$APP_HOME" <<'PY'
import json
import pathlib
import subprocess
import sys
import time

setups_dir = pathlib.Path(sys.argv[1])
app_home = pathlib.Path(sys.argv[2])
setup_file = next(setups_dir.glob("*.maker.json"))
setup = json.loads(setup_file.read_text(encoding="utf-8"))
worker = subprocess.Popen(["sleep", "30"])
record = {
    "setup_id": setup["setup_id"],
    "setup_name": setup["setup"]["name"],
    "setup_path": str(setup_file),
    "artifacts_dir": str(app_home / "artifacts"),
    "repo_root": "/home/pi/gh/yggdrasil",
    "log_path": str(app_home / "build-jobs" / "live-reattach" / "build.log"),
    "completion_path": str(app_home / "build-jobs" / "live-reattach" / "completion.json"),
    "pid": worker.pid,
    "started_at_ms": int(time.time() * 1000),
}
(app_home / "active-build.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
PY

appctl clients > "$LOG_DIR/clients-before-live-build.json" || true
(
  cd "$WORKSPACE_DIR"
  exec env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" "$APP_BIN"
) >"$LOG_DIR/app-live-reattach.log" 2>&1 &
APP_PID=$!

CLIENT_PID="$(wait_for_new_client "$LOG_DIR/clients-before-live-build.json" || true)"
[[ -n "$CLIENT_PID" ]] || fail "no GUI client appeared after live-build relaunch"
export YGGDRASIL_MAKER_APP_PID="$CLIENT_PID"
appctl state --timeout-ms 12000 > "$LOG_DIR/state-live-active-build.json"
journey_stage="$(json_eval "$LOG_DIR/state-live-active-build.json" '
print(data["data"]["current_setup"]["journey_stage"])
')"
[[ "$journey_stage" == "Build" ]] || fail "live active-build reattach did not reopen on Build"

printf '[maker-window-regressions] probing titlebar drag contract\n'
window_id="$(wait_for_window_id "$CLIENT_PID" || true)"
[[ -n "$window_id" ]] || fail "could not find X11 window for pid $CLIENT_PID"

read -r before_x before_y < <(window_position "$window_id") || fail "initial X11 window position missing"

xdotool windowactivate --sync "$window_id"
xdotool mousemove --window "$window_id" 620 18
xdotool mousedown 1
sleep 0.2
xdotool mousemove_relative --sync 140 110
sleep 0.2
xdotool mouseup 1
sleep 0.4

read -r after_x after_y < <(window_position "$window_id") || fail "post-drag X11 window position missing"

if [[ "$before_x" == "$after_x" && "$before_y" == "$after_y" ]]; then
  fail "titlebar drag did not move the window ($before_x,$before_y)"
fi

printf '[maker-window-regressions] ok\n'
