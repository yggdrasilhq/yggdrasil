# Ventoy injection: shipping built ISOs to the boot stick without botching it

Dated: 2026-09-26. Written after a real lab incident: the lab machine
rebooted, Ventoy highlighted an old release by default (a ventoy.json
written during the Sep 11 build wave had pinned the then-latest ISO and
never moved on), and the freshest image on the stick came up as
`localhost` because no site config supplied the real hostname. This
page specifies the machinery that removes both difficulties from now on.

## The tool

`scripts/ventoy-inject.sh` is the only sanctioned way to put yggdrasil
ISOs on the Ventoy stick. Each run:

1. mounts the stick (by filesystem label, on `target_host` over ssh when
   the stick is not plugged into the build host);
2. copies every timestamped `yggdrasil-*.hybrid.iso` from
   `iso_source_dir` that the stick does not have yet, together with its
   `.sha256` sidecar, refusing the copy if it would leave less than
   `min_free_mb` free;
3. applies the retention law below, per profile (server and kde
   independently), deleting the `.sha256` sidecars of pruned ISOs;
4. rewrites `ventoy/ventoy.json` on the stick (backing up any previous
   copy to `ventoy.json.bak`) so `VTOY_DEFAULT_IMAGE` pins the
   `default_rank`-th latest timestamped ISO as the boot-menu default,
   keeping any extra `VTOY_*` control entries the local config supplies;
5. unmounts, and prints a copied / pruned / default summary.

`--dry-run` does every read for real and fakes every mutation. It needs
the stick already mounted.

## The retention law (why it is date-aware)

During a dev day the build produces ISO after ISO. A naive "keep the
newest N" rule measures N against that flood and rotates the REAL
previous working copies off the stick exactly when they matter.

The law instead:

- every ISO built TODAY stays (today's work is today's work);
- the `keep_previous_count` (default 2) most recent ISOs with a build
  date strictly BEFORE today stay, no matter how many builds today
  produces. Those are the last working copies, and they cannot be
  botched by today's build churn;
- everything older goes.

The build timestamp comes from the FILENAME (`yggdrasil-YYYYMMDDHHMM-*`),
never from mtime, which copying would rewrite. Names without a valid
timestamp are never touched and never counted.

## The default-entry law

Ventoy's highlighted entry is whatever it last felt like unless
`VTOY_DEFAULT_IMAGE` pins it. The injector pins the third-latest
timestamped ISO (`default_rank = 3`). On a normal dev-day stick that is:
two builds from today above it, then the newest previous-day copy, which
is exactly the release you want highlighted when the lab machine
reboots and you are not looking.

## Config contract

Like the build, the injector runs on a gitignored local config:

- `ventoy.example.toml` is tracked and documents every key;
- `ventoy.local.toml` is untracked site reality (stick host, mountpoint,
  stick-side ISO directory, retention numbers, extra control entries).
  The script refuses to run without it;
- `ygg.local.toml` is the same contract for the build itself, which is
  where the hostname lives. A stick ISO booted as `localhost` means the
  build that produced it had no real `ygg.local.toml`.hostname.

Two keys deserve care on an existing stick:

- `iso_stick_dir`: the stick may keep its ISOs in a subdirectory (the
  lab stick uses `/yggdrasil`). `VTOY_DEFAULT_IMAGE` must carry that
  path, so never change the stick layout without changing the config.
- `ventoy_<key>` lines: each becomes an extra `VTOY_*` control entry in
  the rewritten ventoy.json. Read the stick's current ventoy.json before
  the first injector run and carry its live entries over (menu timeout,
  secondary menu), or a rewrite silently changes boot behaviour.

## Live-booted target hosts

When the stick's host is itself live-booted FROM that stick, the Ventoy
runtime holds the data partition behind device-mapper and a plain
`mount /dev/sdXN` fails with `Can't open blockdev`. Mount the dm
passthrough instead: read `dmsetup table`, `mknod` the missing
`/dev/mapper/<name>` node (major/minor from the table), mount that, then
run the injector with `skip_mount = true`. This is a boot-state quirk;
a host booted from disk mounts the partition normally.

## Operator loop

```
./mkconfig.sh --profile both        # builds both profiles, runs smoke
./scripts/ventoy-inject.sh --dry-run
./scripts/ventoy-inject.sh
```

The repo-root artifact pruning (`scripts/prune-isos.sh`) keeps the build
tree tidy; the injector owns the stick. They are separate policies for
separate surfaces and neither reads the other.
