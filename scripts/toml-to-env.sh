#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <config.toml>" >&2
  exit 1
fi

toml_file="$1"
if [[ ! -f "$toml_file" ]]; then
  echo "TOML config not found: $toml_file" >&2
  exit 1
fi

upper() {
  echo "$1" | tr '[:lower:]' '[:upper:]'
}

while IFS= read -r raw; do
  line="${raw%%#*}"
  line="$(echo "$line" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
  [[ -z "$line" ]] && continue
  [[ "$line" =~ ^\[ ]] && continue
  [[ "$line" != *"="* ]] && continue

  key="$(echo "${line%%=*}" | sed -E 's/[[:space:]]+$//' | tr '-' '_' )"
  val="$(echo "${line#*=}" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"

  case "$val" in
    true|false)
      out="$val"
      ;;
    \"*\")
      out="${val#\"}"
      out="${out%\"}"
      ;;
    *)
      out="$val"
      ;;
  esac

  env_key="YGG_$(upper "$key")"
  printf '%s="%s"\n' "$env_key" "${out//\"/\\\"}"
done < "$toml_file"
