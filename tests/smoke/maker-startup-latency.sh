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
  printf '[maker-startup-latency] %s\n' "$*" >&2
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

wait_for_window_id() {
  local pid="$1"
  local deadline=$((SECONDS + 20))
  while (( SECONDS < deadline )); do
    local window_id
    window_id="$(xdotool search --all --pid "$pid" 2>/dev/null | head -n 1 || true)"
    if [[ -n "$window_id" ]]; then
      printf '%s\n' "$window_id"
      return 0
    fi
    sleep 0.05
  done
  return 1
}

wait_for_new_client() {
  local before_file="$1"
  local output="$LOG_DIR/clients.json"
  local known_pids
  known_pids="$(json_eval "$before_file" '
print(" ".join(str(item["pid"]) for item in data.get("clients", [])))
')"
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

printf '[maker-startup-latency] building desktop binary\n'
(cd "$WORKSPACE_DIR" && cargo build -p yggdrasil-maker --features desktop-ui >/dev/null)

printf '[maker-startup-latency] creating isolated setup\n'
(
  cd "$WORKSPACE_DIR"
  YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" \
    "$APP_BIN" setup new \
      --name startup-check \
      --preset nas \
      --profile server \
      --hostname startup-host \
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

printf '[maker-startup-latency] launching GUI\n'
appctl clients > "$LOG_DIR/clients-before.json" || true
launch_started_ms="$(date +%s%3N)"
(
  cd "$WORKSPACE_DIR"
  exec env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" "$APP_BIN"
) >"$LOG_DIR/app.log" 2>&1 &
APP_PID=$!

window_id="$(wait_for_window_id "$APP_PID" || true)"
[[ -n "$window_id" ]] || fail "no top-level window appeared"
window_elapsed_ms="$(( $(date +%s%3N) - launch_started_ms ))"
(( window_elapsed_ms <= 1500 )) || fail "startup window latency too high: ${window_elapsed_ms}ms"

CLIENT_PID="$(wait_for_new_client "$LOG_DIR/clients-before.json" || true)"
[[ -n "$CLIENT_PID" ]] || fail "no GUI client appeared"
export YGGDRASIL_MAKER_APP_PID="$CLIENT_PID"

appctl state --timeout-ms 8000 > "$LOG_DIR/state.json"
journey_stage="$(json_eval "$LOG_DIR/state.json" '
print(data["data"]["current_setup"]["journey_stage"])
')"
[[ "$journey_stage" == "Choose" ]] || fail "untouched startup should reopen on Choose, got $journey_stage"

trace_file="$APP_HOME/event-trace.jsonl"
[[ -f "$trace_file" ]] || fail "trace file missing"
trace_window_elapsed_ms="$(python3 - "$trace_file" <<'PY'
import json
import pathlib
import sys

for line in pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    if not line.strip():
        continue
    row = json.loads(line)
    if row.get("category") == "startup" and row.get("name") == "window_spawned":
        print(int(row.get("payload", {}).get("elapsed_ms", 0)))
        break
else:
    print("")
PY
)"
[[ -n "$trace_window_elapsed_ms" ]] || fail "startup/window_spawned trace missing"
(( trace_window_elapsed_ms <= 1500 )) || fail "startup trace latency too high: ${trace_window_elapsed_ms}ms"

printf '[maker-startup-latency] ok\n'
