# ZFS block-clone txg wedge — failure mode, mitigation, watchdog

## Failure mode

On zfs 2.4.x, block cloning (`copy_file_range(2)`, `cp --reflink`, and
some tools' automatic fast-copy paths) can park a transaction group in
the **quiesced-never-syncs** state: the txg reaches `Q` in

```
/proc/spl/kstat/zfs/<pool>/txgs
```

and never reaches `C`. From that moment every write on the pool stalls
in uninterruptible sleep (D state):

- load average climbs (D-state processes count), ssh eventually stops
  accepting logins while the host still answers ping
- reads degrade over minutes to well under 1 MB/s — ARC cannot evict
  dirty data that cannot sync
- `zpool sync <pool>` hangs unkillably; killing the writers does **not**
  drain the txg
- recovery observed: only a reboot (`reboot -f`; a clean `reboot` hangs
  on unmounts). After reboot the pool imports clean, no data errors.

Reproduced twice on zfs 2.4.2 with multi-GB image copies where the copy
itself completed but subsequent small writes into the cloned file sat in
the wedged txg. The trigger is the block-clone path: plain `cp` on a
2.4.x pool uses `copy_file_range`, which is a clone — **there is no safe
"plain cp" on an affected pool**.

## Mitigations shipped in this ISO

1. **Block cloning disabled at module level**
   (`/etc/modprobe.d/zfs-bclone.conf`):

   ```
   options zfs zfs_bclone_enabled=0
   ```

   `copy_file_range` falls back to a plain copy. Big-image copies should
   still use `dd` (deterministic, no hidden fast paths):

   ```
   dd if=src of=dst bs=4M status=none
   ```

   If a future zfs release fixes the quiesce path, drop the override and
   re-test with a large `cp` before trusting it.

2. **`ygg-txg-watchdog.service`/`.timer`** (1-minute cadence): tracks the
   highest committed txg per pool; if it stops advancing for 15 minutes
   while a pending (Q/S) txg exists, it logs one `kern.crit` line
   (`journalctl -t ygg-txg-watchdog`) and touches `/run/ygg-txg-stuck`.
   An idle pool never alarms (no pending txgs). Detection only — decide
   escalation separately.

## Operator playbook when the watchdog fires

1. Confirm: `tail /proc/spl/kstat/zfs/*/txgs` — a txg sitting in `Q`
   while the committed counter is frozen is the signature.
2. Stop new writers to the affected pool. Killing existing writers has
   NOT historically drained the txg.
3. Plan a reboot window (`reboot -f` from console; sshd may already be
   dead). Buffered guest/VM writes are lost; qcow metadata survives.
4. After reboot: check `zpool status` for errors, and leave
   `zfs_bclone_enabled=0` in place.

## Site wiring (not committed)

The watchdog and the module override ship in the ISO. Big-copy
discipline (`dd` for images) lives in your ops docs, not in code.
