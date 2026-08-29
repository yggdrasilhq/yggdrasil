# The second backup leg — data snapshot export for the recovery stick

The host backup story has two legs:

1. **Leg 1 — live replication.** A daily incremental ZFS replication of
   irreplaceable datasets onto a second pool on the same machine
   (`jewel-backup`). Fast, automatic, and useless if the machine itself is
   lost.
2. **Leg 2 — the stick.** `scripts/data-snapshot-export.sh` exports each
   protected dataset's newest backup snapshot as a standalone ZFS **stream
   file** into a spool directory, so the data can leave the host entirely.
   The recovery stick (Ventoy) carries the bootable ISO **and** the spool:
   rebuild the host from the ISO, then restore the data with `zfs recv` —
   no network, no second pool, no cloud.

## Site configuration (generated, never committed)

`/etc/yggdrasil/data-export.conf`:

```sh
SOURCE_DATASETS="pool/data/one pool/data/two ..."   # the datasets worth a stick
SPOOL=/path/to/spool                                # stream files land here
KEEP_EXPORTS=3                                      # exports kept per dataset
STICK_DIR=/media/you/stick                          # optional; mirrors spool when mounted
```

## Cadence

A systemd timer runs the exporter shortly after the daily replication
(e.g. `OnCalendar=*-*-* 05:30:00 UTC`), so each export carries that day's
backup snapshot. When the stick is plugged in (detected by a marker file),
the spool mirrors onto it automatically; otherwise the spool waits.

## Restore (from a recovery boot)

```sh
zfs recv -F pool/data/<name> < graphs/<name>/<name>-FULL.zfs
zfs recv -F pool/data/<name> < graphs/<name>/<name>-incr-*.zfs   # filename order
```

`MANIFEST.txt` in the spool records what exists, with sizes, for the
version of you that is holding the stick at 3 a.m.

## Why streams and not a mirrored pool

A stream file is just a file: it survives any filesystem, copies with
`rsync`, and restores on any ZFS machine of the same or newer feature
level. The stick stays readable even where the pool layout it came from is
gone.
