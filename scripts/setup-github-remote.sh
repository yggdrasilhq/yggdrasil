#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: ./scripts/setup-github-remote.sh <github-repo-url>"
  echo "Example: ./scripts/setup-github-remote.sh git@github.com:avikalpa/yggdrasil.git"
  exit 1
fi

REPO_URL="$1"

if git remote get-url origin >/dev/null 2>&1; then
  git remote set-url origin "$REPO_URL"
else
  git remote add origin "$REPO_URL"
fi

echo "Origin set to: $REPO_URL"
echo "Next: git push -u origin main"
