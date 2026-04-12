#!/usr/bin/env bash
set -euo pipefail

PATH="/usr/sbin:/sbin:$PATH"

repo_root="$(git rev-parse --show-toplevel)"
workspace_root="$repo_root/yggdrasil-maker"
app_bin="$workspace_root/target/debug/yggdrasil-maker"
run_id="$(date +%Y%m%d-%H%M%S)"
out_root="$repo_root/.gstack/manual-maker-observe/$run_id"
artifacts_dir="$out_root/artifacts"
shots_dir="$out_root/screenshots"
recordings_dir="$out_root/recordings"
logs_dir="$out_root/logs"
timeout_sec=7200
skip_image_build=false
keep_app=false
setup_name="Release Gate Both ${run_id}"
hostname="yggdrasil-e2e"
preset="nas"
profile="both"

log() {
  printf '[manual-maker-observe] %s\n' "$*" >&2
}

usage() {
  cat <<USAGE
Usage: ./scripts/manual-maker-observe.sh [options]

Options:
  --out-dir PATH         Override output root
  --timeout-sec N        Build wait timeout in seconds (default: 7200)
  --skip-image-build     Reuse existing builder image
  --keep-app             Leave the launched GUI running
  --setup-name NAME      Setup name to drive through the GUI
  --hostname NAME        Hostname to set in the GUI
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-dir)
      out_root="$2"
      artifacts_dir="$out_root/artifacts"
      shots_dir="$out_root/screenshots"
      recordings_dir="$out_root/recordings"
      logs_dir="$out_root/logs"
      shift 2
      ;;
    --timeout-sec)
      timeout_sec="$2"
      shift 2
      ;;
    --skip-image-build)
      skip_image_build=true
      shift
      ;;
    --keep-app)
      keep_app=true
      shift
      ;;
    --setup-name)
      setup_name="$2"
      shift 2
      ;;
    --hostname)
      hostname="$2"
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

mkdir -p "$artifacts_dir" "$shots_dir" "$recordings_dir" "$logs_dir"

if [[ -z "${YGG_QEMU_SSH_PRIVATE_KEY:-}" && -f "$HOME/.ssh/id_ed25519" ]]; then
  export YGG_QEMU_SSH_PRIVATE_KEY="$HOME/.ssh/id_ed25519"
fi

json_eval() {
  local input="$1"
  local code="$2"
  local extra="${3:-}"
  python3 - "$input" "$code" "$extra" <<'PY'
import json
import pathlib
import sys

payload = pathlib.Path(sys.argv[1]).read_text(encoding='utf-8')
data = json.loads(payload)
namespace = {"data": data, "extra": sys.argv[3]}
exec(sys.argv[2], namespace)
PY
}

appctl() {
  (cd "$workspace_root" && "$app_bin" server app "$@")
}

capture_state() {
  local name="$1"
  appctl state --timeout-ms 12000 > "$logs_dir/${name}-state.json"
}

capture_shot() {
  local name="$1"
  appctl screenshot "$shots_dir/${name}.png" --timeout-ms 15000 > "$logs_dir/${name}-shot.json"
}

capture_trace() {
  local name="$1"
  appctl trace-tail --lines 200 > "$logs_dir/${name}-trace.json"
}

wait_for_new_client() {
  local before_file="$1"
  local before_pids after_file after_pid
  before_pids="$(json_eval "$before_file" 'print(" ".join(str(item["pid"]) for item in data.get("clients", [])))')"
  after_file="$logs_dir/clients-after.json"
  local deadline=$((SECONDS + 45))
  while (( SECONDS < deadline )); do
    appctl clients > "$after_file"
    after_pid="$(json_eval "$after_file" '
clients = data.get("clients", [])
known = {value for value in extra.split() if value}
for client in clients:
    pid = str(client.get("pid"))
    if pid not in known:
        print(pid)
        break
else:
    print("")
' "$before_pids" 2>/dev/null || true)"
    if [[ -n "$after_pid" ]]; then
      printf '%s\n' "$after_pid"
      return 0
    fi
    sleep 0.25
  done
  return 1
}

app_pid=""
cleanup() {
  if [[ -n "$app_pid" && "$keep_app" != true ]]; then
    kill "$app_pid" >/dev/null 2>&1 || true
    wait "$app_pid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

log "building maker desktop binary"
(
  cd "$workspace_root"
  cargo build -p yggdrasil-maker --features desktop-ui
) | tee "$logs_dir/cargo-build.log"

if [[ "$skip_image_build" != true ]]; then
  log "building maker container image"
  (
    cd "$repo_root"
    ./scripts/build-maker-image.sh
  ) | tee "$logs_dir/builder-image.log"
fi

log "capturing existing GUI clients"
appctl clients > "$logs_dir/clients-before.json" || true

log "launching fresh maker GUI client"
(
  cd "$workspace_root"
  exec "$app_bin"
) > "$logs_dir/app.log" 2>&1 &
app_pid=$!

client_pid="$(wait_for_new_client "$logs_dir/clients-before.json")"
if [[ -z "$client_pid" ]]; then
  log "failed to observe a newly launched maker GUI client"
  exit 1
fi
printf '%s\n' "$client_pid" > "$logs_dir/client-pid.txt"
export YGGDRASIL_MAKER_APP_PID="$client_pid"
log "new GUI client pid=$client_pid"

appctl focus --timeout-ms 12000 > "$logs_dir/focus.json"
appctl set-build-context --artifacts-dir "$artifacts_dir" --repo-root "$repo_root" --timeout-ms 12000 > "$logs_dir/build-context.json"
appctl new-setup --name "$setup_name" --preset "$preset" --profile "$profile" --hostname "$hostname" --timeout-ms 12000 > "$logs_dir/new-setup.json"
appctl save-setup --timeout-ms 12000 > "$logs_dir/save-setup.json"

appctl set-stage outcome --timeout-ms 12000 > "$logs_dir/stage-outcome.json"
capture_state outcome
capture_shot outcome
capture_trace outcome

appctl set-stage profile --timeout-ms 12000 > "$logs_dir/stage-profile.json"
capture_state profile
capture_shot profile

appctl set-stage personalize --timeout-ms 12000 > "$logs_dir/stage-personalize.json"
appctl set-setup-name "$setup_name" --timeout-ms 12000 > "$logs_dir/set-setup-name.json"
appctl set-hostname "$hostname" --timeout-ms 12000 > "$logs_dir/set-hostname.json"
capture_state personalize
capture_shot personalize

appctl set-stage review --timeout-ms 12000 > "$logs_dir/stage-review.json"
capture_state review
capture_shot review
capture_trace review

log "starting build through the live GUI"
appctl set-stage build --timeout-ms 12000 > "$logs_dir/stage-build.json"
appctl start-build --timeout-ms 12000 > "$logs_dir/start-build.json"
capture_state build-start
capture_shot build-start

if appctl wait-build --timeout-ms "$((timeout_sec * 1000))" --poll-ms 1000 --trace-lines 240 > "$logs_dir/wait-build.json" 2> "$logs_dir/wait-build.stderr.log"; then
  wait_build_rc=0
else
  wait_build_rc=$?
fi
if ! capture_trace build-finish > /dev/null 2> "$logs_dir/build-finish-trace.stderr.log"; then
  : > "$logs_dir/build-finish-trace.json"
fi
if ! capture_state build-finish > /dev/null 2> "$logs_dir/build-finish-state.stderr.log"; then
  : > "$logs_dir/build-finish-state.json"
fi

if [[ -s "$logs_dir/wait-build.json" ]]; then
  build_succeeded="$(json_eval "$logs_dir/wait-build.json" 'print(str(data.get("build", {}).get("succeeded", False)).lower())')"
  build_failed="$(json_eval "$logs_dir/wait-build.json" 'print(str(data.get("build", {}).get("failed", False)).lower())')"
  build_timed_out="$(json_eval "$logs_dir/wait-build.json" 'print(str(data.get("wait", {}).get("timed_out", False)).lower())')"
else
  build_succeeded=false
  build_failed=false
  build_timed_out=true
fi

if [[ "$build_succeeded" == "true" ]]; then
  capture_shot success
  appctl screenrecord "$recordings_dir/success.mp4" --duration-sec 6 --timeout-ms 20000 > "$logs_dir/success-recording.json" 2> "$logs_dir/success-recording.stderr.log"
else
  capture_shot failure || true
fi

log "running ISO smoke checks against built artifacts"
smoke_args=(--profile both --artifacts-dir "$artifacts_dir" --require-artifacts --with-iso-rootfs)
if [[ -n "${YGG_QEMU_SSH_PRIVATE_KEY:-}" && -f "${YGG_QEMU_SSH_PRIVATE_KEY}" ]]; then
  smoke_args+=(--with-qemu-boot)
fi
smoke_cmd=(./tests/smoke/run.sh "${smoke_args[@]}")
if [[ " ${smoke_args[*]} " == *" --with-qemu-boot "* ]] && sudo -n true >/dev/null 2>&1; then
  smoke_cmd=(sudo -n env "YGG_QEMU_SSH_PRIVATE_KEY=${YGG_QEMU_SSH_PRIVATE_KEY}" "${smoke_cmd[@]}")
fi
(
  cd "$repo_root"
  "${smoke_cmd[@]}"
) | tee "$logs_dir/smoke.log"

python3 - <<'PY' "$out_root" "$artifacts_dir" "$logs_dir" "$wait_build_rc" "$build_succeeded" "$build_failed" "$build_timed_out"
import json
import pathlib
import sys

out_root = pathlib.Path(sys.argv[1])
artifacts_dir = pathlib.Path(sys.argv[2])
logs_dir = pathlib.Path(sys.argv[3])
summary = {
    "out_root": str(out_root),
    "artifacts_dir": str(artifacts_dir),
    "wait_build_exit_code": int(sys.argv[4]),
    "build_succeeded": sys.argv[5] == "true",
    "build_failed": sys.argv[6] == "true",
    "build_timed_out": sys.argv[7] == "true",
    "artifact_manifest": str(artifacts_dir / "artifact-manifest.json"),
    "artifact_manifest_exists": (artifacts_dir / "artifact-manifest.json").is_file(),
    "shots": sorted(str(path) for path in (out_root / "screenshots").glob("*.png")),
    "recordings": sorted(str(path) for path in (out_root / "recordings").glob("*.mp4")),
    "logs": sorted(str(path) for path in logs_dir.iterdir() if path.is_file()),
}
(out_root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
PY

if [[ ! -f "$artifacts_dir/artifact-manifest.json" ]]; then
  log "artifact manifest missing at $artifacts_dir/artifact-manifest.json"
  exit 1
fi

if [[ "$build_succeeded" != "true" || "$build_failed" == "true" || "$build_timed_out" == "true" ]]; then
  log "GUI build did not finish successfully; see $logs_dir/wait-build.json and $logs_dir/wait-build.stderr.log"
  exit 1
fi

log "manual maker observability run complete: $out_root"
