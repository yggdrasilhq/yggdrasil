#!/usr/bin/env bash
set -euo pipefail

prune_group() {
  local label="$1"
  local list_cmd="$2"
  mapfile -t files < <(eval "$list_cmd")
  [[ ${#files[@]} -eq 0 ]] && return 0

  local cutoff keep_recent="" keep_old_1="" keep_old_2=""
  cutoff=$(date -d '3 days ago' +%s)

  for f in "${files[@]}"; do
    mtime=$(stat -c %Y "$f")
    if [[ "$mtime" -ge "$cutoff" ]]; then
      if [[ -z "$keep_recent" ]]; then
        keep_recent="$f"
      fi
    else
      if [[ -z "$keep_old_1" ]]; then
        keep_old_1="$f"
      elif [[ -z "$keep_old_2" ]]; then
        keep_old_2="$f"
      fi
    fi
  done

  for f in "${files[@]}"; do
    if [[ "$f" != "$keep_recent" && "$f" != "$keep_old_1" && "$f" != "$keep_old_2" ]]; then
      echo "Pruning ${label} ISO artifact: $f"
      rm -f -- "$f"
    fi
  done
}

prune_group "server" "ls -1t yggdrasil-*-amd64.hybrid.iso 2>/dev/null | rg -v -- '-kde-amd64\\.hybrid\\.iso$' || true"
prune_group "kde" "ls -1t yggdrasil-*-kde-amd64.hybrid.iso 2>/dev/null || true"
