#!/bin/bash
# data-snapshot-export.sh — the SECOND backup leg for data datasets.
#
# Leg 1 (jewel-backup) replicates live datasets to a second pool on the same
# host. This leg exports standalone ZFS streams to a spool directory, so the
# data can leave the host entirely — onto the recovery stick that carries the
# yggdrasil ISO (Ventoy). Rebuilding the host from the stick then restores the
# data with `zfs recv`, no network, no second pool needed.
#
# Site values come from /etc/yggdrasil/data-export.conf (generated, never
# committed):
#   SOURCE_DATASETS="pool/data/one pool/data/two ..."
#   SPOOL=/path/to/spool            # stream files land here
#   KEEP_EXPORTS=3                  # exports kept per dataset
#   STICK_DIR=/path/to/stick-mount  # optional; when present, spool is copied there
#
# The exported chain per dataset: one FULL stream of the newest backup
# snapshot, then incrementals. Restore order = filename sort order:
#   zfs recv -F <ds> < full-*.zfs && zfs recv -F <ds> < incr-*.zfs ...
set -uo pipefail

CONF=/etc/yggdrasil/data-export.conf
[ -r "$CONF" ] || { echo "data-export: missing config $CONF" >&2; exit 1; }
# shellcheck source=/dev/null
. "$CONF"

SPOOL="${SPOOL:-}"
SOURCE_DATASETS="${SOURCE_DATASETS:-}"
KEEP_EXPORTS="${KEEP_EXPORTS:-3}"
STICK_DIR="${STICK_DIR:-}"
MARKER="${MARKER:-.yggdrasil-stick}"

[ -n "$SPOOL" ] && [ -n "$SOURCE_DATASETS" ] || { echo "data-export: SPOOL and SOURCE_DATASETS required" >&2; exit 1; }
mkdir -p "$SPOOL/.state"
LOG=/var/log/data-snapshot-export.log
log(){ echo "$(date -u +%FT%TZ) $*" | tee -a "$LOG"; }

fail=0
for SRC in $SOURCE_DATASETS; do
  NAME="${SRC##*/}"
  OUT="$SPOOL/$NAME"; mkdir -p "$OUT" "$SPOOL/.state"
  LATEST="$(zfs list -H -o name -t snapshot -d 1 "$SRC" 2>/dev/null \
            | grep -E "@jewel-" | sed "s#^${SRC}@##" | sort | tail -1)"
  if [ -z "$LATEST" ]; then
    log "SKIP $NAME (no jewel snapshot found — run the daily backup first)"
    continue
  fi
  PREV="$(cat "$SPOOL/.state/$NAME" 2>/dev/null || true)"

  # Skip if this snapshot is already exported.
  if [ "$PREV" = "$LATEST" ]; then
    log "up-to-date $NAME @$LATEST"
  else
    STREAM="$OUT/${NAME}-$(date -u +%Y%m%d)-${LATEST}.zfs"
    if [ -n "$PREV" ]; then
      log "incremental $NAME @$PREV..@$LATEST -> $STREAM"
      zfs send -I "@$PREV" "$SRC@$LATEST" > "$STREAM" || { log "ERR send $NAME"; fail=1; continue; }
    else
      log "FULL $NAME @$LATEST -> $STREAM"
      zfs send "$SRC@$LATEST" > "$STREAM" || { log "ERR send $NAME"; fail=1; continue; }
    fi
    echo "$LATEST" > "$SPOOL/.state/$NAME"
    # Prune old exports, keeping the newest KEEP_EXPORTS streams.
    ls -1t "$OUT"/${NAME}-*.zfs 2>/dev/null | tail -n "+$((KEEP_EXPORTS + 1))" | while read -r old; do
      log "prune $old"; rm -f "$old"
    done
  fi
done

# Manifest: what a recovery boot needs to know, without reading this script.
{
  echo "# data-snapshot-export manifest — $(date -u +%FT%TZ)"
  echo "# restore: zfs recv -F <dataset> < full stream, then each incremental in filename order"
  for d in "$SPOOL"/*/; do
    [ -d "$d" ] || continue
    echo "# $(basename "$d")"
    ls -lh "$d" 2>/dev/null | awk 'NR>1 {print "#   " $9, $5}'
  done
} > "$SPOOL/MANIFEST.txt"

# Optional third hop: a mounted recovery stick mirrors the spool.
if [ -n "$STICK_DIR" ] && [ -f "$STICK_DIR/$MARKER" ]; then
  log "stick detected at $STICK_DIR — mirroring spool"
  rsync -a --delete "$SPOOL/" "$STICK_DIR/graphs/" || { log "ERR stick mirror"; fail=1; }
else
  log "no stick mounted (STICK_DIR=$STICK_DIR) — spool only"
fi

log "data-snapshot-export complete (fail=$fail)"
exit "$fail"
