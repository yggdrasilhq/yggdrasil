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
trap 'rm -rf "$TMP_DIR"; if [[ -n "${APP_PID:-}" ]]; then kill "$APP_PID" >/dev/null 2>&1 || true; wait "$APP_PID" >/dev/null 2>&1 || true; fi' EXIT

fail() {
  printf '[maker-input-latency] %s\n' "$*" >&2
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

measure_ms() {
  local output_file="$1"
  shift
  python3 - "$output_file" "$@" <<'PY'
import pathlib
import subprocess
import sys
import time

output = pathlib.Path(sys.argv[1])
command = sys.argv[2:]
started = time.perf_counter()
completed = subprocess.run(command, capture_output=True, text=True)
elapsed_ms = round((time.perf_counter() - started) * 1000)
output.write_text(completed.stdout, encoding="utf-8")
sys.stderr.write(completed.stderr)
if completed.returncode != 0:
    raise SystemExit(completed.returncode)
print(elapsed_ms)
PY
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

first_client_pid() {
  local output="$1"
  json_eval "$output" '
clients = data.get("clients", [])
print(clients[0]["pid"] if clients else "")
'
}

count_slow_preview_events() {
  local trace_path="$APP_HOME/event-trace.jsonl"
  if [[ ! -f "$trace_path" ]]; then
    printf '0\n'
    return 0
  fi
  python3 - "$trace_path" <<'PY'
import json
import pathlib
import sys

count = 0
for line in pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    if not line.strip():
        continue
    row = json.loads(line)
    if row.get("category") == "perf" and row.get("name") == "slow_preview_refresh":
        count += 1
print(count)
PY
}

printf '[maker-input-latency] building desktop binary\n'
(cd "$WORKSPACE_DIR" && cargo build -p yggdrasil-maker --features desktop-ui >/dev/null)

printf '[maker-input-latency] creating isolated setup\n'
(
  cd "$WORKSPACE_DIR"
  YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" \
    "$APP_BIN" setup new \
      --name latency-check \
      --preset nas \
      --profile server \
      --hostname yggdrasil \
      --output "$LOG_DIR/bootstrap-setup.json" >/dev/null
)
python3 - "$LOG_DIR/bootstrap-setup.json" "$SETUPS_DIR" <<'PY'
import json
import pathlib
import sys

source = pathlib.Path(sys.argv[1])
setups_dir = pathlib.Path(sys.argv[2])
data = json.loads(source.read_text(encoding="utf-8"))
target = setups_dir / f'{data["setup"]["name"].lower().replace(" ", "-")}--{data["setup_id"]}.maker.json'
target.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
source.unlink()
PY

appctl clients > "$LOG_DIR/clients-before.json" || true
printf '[maker-input-latency] launching GUI\n'
(
  cd "$WORKSPACE_DIR"
  exec env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" "$APP_BIN"
) >"$LOG_DIR/app.log" 2>&1 &
APP_PID=$!

CLIENT_PID="$(wait_for_new_client "$LOG_DIR/clients-before.json" || true)"
if [[ -z "$CLIENT_PID" ]]; then
  CLIENT_PID="$(first_client_pid "$LOG_DIR/clients-before.json")"
fi
[[ -n "$CLIENT_PID" ]] || fail "no GUI client appeared"
export YGGDRASIL_MAKER_APP_PID="$CLIENT_PID"

set_stage_ms="$(measure_ms "$LOG_DIR/set-stage.json" env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" YGGDRASIL_MAKER_APP_PID="$CLIENT_PID" "$APP_BIN" server app set-stage personalize --timeout-ms 8000)"
set_panel_ms="$(measure_ms "$LOG_DIR/set-panel.json" env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" YGGDRASIL_MAKER_APP_PID="$CLIENT_PID" "$APP_BIN" server app set-right-panel plan --timeout-ms 8000)"
before_count="$(count_slow_preview_events)"
set_hostname_ms="$(measure_ms "$LOG_DIR/set-hostname.json" env YGGDRASIL_MAKER_SETUP_ROOT="$SETUPS_DIR" YGGDRASIL_MAKER_APP_PID="$CLIENT_PID" "$APP_BIN" server app set-hostname latency-check --timeout-ms 8000)"
after_count="$(count_slow_preview_events)"
appctl state --timeout-ms 8000 > "$LOG_DIR/state.json"

journey_stage="$(json_eval "$LOG_DIR/state.json" '
print(data["data"]["current_setup"]["journey_stage"])
')"
hostname="$(json_eval "$LOG_DIR/state.json" '
print(data["data"]["current_setup"]["hostname"])
')"
build_name="$(json_eval "$LOG_DIR/state.json" '
print(data["data"]["current_setup"]["name"])
')"

[[ "$journey_stage" == "Name" ]] || fail "expected Name stage, got $journey_stage"
[[ "$hostname" == "latency-check" ]] || fail "expected hostname latency-check, got $hostname"
[[ "$build_name" == latency-check-* ]] || fail "expected dynamic build name after hostname edit, got $build_name"
[[ "$before_count" == "$after_count" ]] || fail "hostname edit triggered slow preview refresh ($before_count -> $after_count)"
(( set_stage_ms <= 450 )) || fail "set-stage latency too high: ${set_stage_ms}ms"
(( set_panel_ms <= 450 )) || fail "set-right-panel latency too high: ${set_panel_ms}ms"
(( set_hostname_ms <= 450 )) || fail "set-hostname latency too high: ${set_hostname_ms}ms"

printf '[maker-input-latency] ok\n'
