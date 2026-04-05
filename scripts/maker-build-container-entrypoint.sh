#!/usr/bin/env bash
set -euo pipefail

CONFIG_PATH=""
INVOKE_PATH=""
REPO_ROOT="${YGGDRASIL_MAKER_REPO_ROOT:-/workspace/repo}"

usage() {
  cat <<'USAGE'
Usage: maker-build-container-entrypoint.sh --config /workspace/input/ygg.local.toml --invoke /workspace/input/invocation.json
USAGE
}

json_field() {
  local file="$1"
  local key="$2"
  python3 - "$file" "$key" <<'PY'
import json
import sys

path = sys.argv[1]
key = sys.argv[2]
with open(path, "r", encoding="utf-8") as handle:
    payload = json.load(handle)
value = payload.get(key)
if value is None:
    sys.exit(1)
if isinstance(value, bool):
    print("true" if value else "false")
else:
    print(value)
PY
}

emit_event() {
  local kind="$1"
  local payload_json="${2:-{}}"
  python3 - "$kind" "$payload_json" <<'PY'
import json
import sys

kind = sys.argv[1]
payload = json.loads(sys.argv[2])
payload["type"] = kind
print(json.dumps(payload), flush=True)
PY
}

emit_stage_started() {
  emit_event "stage-started" "{\"stage\":\"$1\"}"
}

emit_stage_finished() {
  emit_event "stage-finished" "{\"stage\":\"$1\"}"
}

emit_failure() {
  local code="$1"
  local message_key="$2"
  local detail="$3"
  python3 - "$code" "$message_key" "$detail" <<'PY'
import json
import sys

payload = {
    "type": "failure",
    "code": sys.argv[1],
    "message_key": sys.argv[2],
    "detail": sys.argv[3],
}
print(json.dumps(payload), flush=True)
PY
}

emit_artifact_ready() {
  local profile="$1"
  local path="$2"
  python3 - "$profile" "$path" <<'PY'
import json
import sys

payload = {
    "type": "artifact-ready",
    "profile": sys.argv[1],
    "path": sys.argv[2],
}
print(json.dumps(payload), flush=True)
PY
}

emit_log_line() {
  local stream="$1"
  local line="$2"
  python3 - "$stream" "$line" <<'PY'
import json
import sys

payload = {
    "type": "log-line",
    "stream": sys.argv[1],
    "line": sys.argv[2],
}
print(json.dumps(payload), flush=True)
PY
}

log_line() {
  printf '[yggdrasil-maker-build] %s\n' "$*" >&2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --config)
      CONFIG_PATH="${2:-}"
      shift 2
      ;;
    --invoke)
      INVOKE_PATH="${2:-}"
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

if [[ -z "$CONFIG_PATH" || -z "$INVOKE_PATH" ]]; then
  usage >&2
  exit 1
fi

emit_stage_started "preflight"

if [[ ! -f "$CONFIG_PATH" ]]; then
  emit_failure "build-config-invalid" "config_missing" "config file not found: $CONFIG_PATH"
  exit 1
fi

if [[ ! -f "$INVOKE_PATH" ]]; then
  emit_failure "build-config-invalid" "invoke_missing" "invocation file not found: $INVOKE_PATH"
  exit 1
fi

if [[ ! -d "$REPO_ROOT" ]]; then
  emit_failure "unsupported-platform" "repo_root_missing" "repo root not found: $REPO_ROOT"
  exit 1
fi

profile="$(json_field "$INVOKE_PATH" build_profile)"
skip_smoke="$(json_field "$INVOKE_PATH" skip_smoke)"
artifacts_dir="$(json_field "$INVOKE_PATH" artifacts_dir)"

if [[ ! -w "$REPO_ROOT" ]]; then
  emit_failure "output-permission-denied" "repo_not_writable" "repo root is not writable: $REPO_ROOT"
  exit 1
fi

mkdir -p "$artifacts_dir"

emit_stage_finished "preflight"
emit_stage_started "build"

cd "$REPO_ROOT"

cmd=(./mkconfig.sh --config "$CONFIG_PATH" --profile "$profile" --skip-smoke)
if [[ "$skip_smoke" == "true" ]]; then
  :
fi

set +e
"${cmd[@]}" \
  > >(while IFS= read -r line; do emit_log_line "stdout" "$line"; done) \
  2> >(while IFS= read -r line; do emit_log_line "stderr" "$line"; done)
build_status=$?
set -e

if [[ "$build_status" -ne 0 ]]; then
  emit_failure "build-process-failed" "mkconfig_failed" "mkconfig.sh failed for profile $profile"
  exit 1
fi

emit_stage_finished "build"
if [[ "$skip_smoke" != "true" ]]; then
  emit_stage_started "smoke"
  smoke_cmd=(
    ./tests/smoke/run.sh
    --profile "$profile"
    --require-artifacts
    --with-iso-rootfs
    --artifacts-dir ./artifacts
    --server-iso ./artifacts/server-latest.iso
    --kde-iso ./artifacts/kde-latest.iso
  )
  if [[ "${YGG_ENABLE_QEMU_SMOKE:-false}" == "true" ]]; then
    smoke_cmd+=(--with-qemu-boot)
  fi

  set +e
  "${smoke_cmd[@]}" \
    > >(while IFS= read -r line; do emit_log_line "stdout" "$line"; done) \
    2> >(while IFS= read -r line; do emit_log_line "stderr" "$line"; done)
  smoke_status=$?
  set -e
  if [[ "$smoke_status" -ne 0 ]]; then
    emit_failure "smoke-test-failed" "smoke_failed" "post-build smoke tests failed for profile $profile"
    exit 1
  fi
  emit_stage_finished "smoke"
fi

emit_stage_started "artifact_copy"

copy_if_present() {
  local source="$1"
  local target="$2"
  local profile_name="$3"
  if [[ -f "$source" ]]; then
    cp -f "$source" "$target"
    emit_artifact_ready "$profile_name" "$target"
  fi
}

case "$profile" in
  both)
    copy_if_present "./artifacts/server-latest.iso" "$artifacts_dir/server-latest.iso" "server"
    copy_if_present "./artifacts/kde-latest.iso" "$artifacts_dir/kde-latest.iso" "kde"
    ;;
  server)
    copy_if_present "./artifacts/server-latest.iso" "$artifacts_dir/server-latest.iso" "server"
    ;;
  kde)
    copy_if_present "./artifacts/kde-latest.iso" "$artifacts_dir/kde-latest.iso" "kde"
    ;;
esac

python3 - "$artifacts_dir" "$profile" "$(json_field "$INVOKE_PATH" app_version)" "$(json_field "$INVOKE_PATH" setup_name)" "$(json_field "$INVOKE_PATH" source_mode)" <<'PY'
import hashlib
import json
import pathlib
import sys

artifacts_dir = pathlib.Path(sys.argv[1])
build_profile = sys.argv[2]
app_version = sys.argv[3]
setup_name = sys.argv[4]
source_mode = sys.argv[5]

records = []
for profile_name, file_name in [("server", "server-latest.iso"), ("kde", "kde-latest.iso")]:
    path = artifacts_dir / file_name
    if not path.exists():
        continue
    records.append({
        "kind": "iso",
        "profile": profile_name,
        "path": str(path),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "size_bytes": path.stat().st_size,
    })

payload = {
    "app_version": app_version,
    "setup_name": setup_name,
    "build_profile": build_profile,
    "mode": "local-docker",
    "source_mode": source_mode,
    "artifacts": records,
}
(artifacts_dir / "artifact-manifest.json").write_text(
    json.dumps(payload, indent=2) + "\n",
    encoding="utf-8",
)
PY

emit_stage_finished "artifact_copy"
emit_stage_started "complete"
emit_stage_finished "complete"
