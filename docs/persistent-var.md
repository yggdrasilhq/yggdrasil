# A /var that survives the reboot

## The problem with a disposable root

A live ISO gives you a root filesystem that exists only in memory. That
is the point. You boot a machine, the machine works, you cut power, and
the next boot starts from exactly the same image with exactly the same
state. Nothing rots. Nothing accumulates. Nothing surprises you.

The price is that everything the running system writes to its root
disappears with it. On a machine with a ZFS pool this is painful in one
specific way: the pool is persistent, the root is not, and the system
keeps writing things to the root that you actually wanted to keep.
Configuration you hand edited. A script you dropped into
`/usr/local/sbin`. The journal you now need for the incident you are
debugging. On a live system these things evaporate at the next reboot,
and you rebuild them by hand. Twice is a coincidence. Three times is a
design flaw.

## The SmartOS answer

SmartOS runs the operating system from a ramdisk and treats the ZFS
pool as the only real storage on the machine. Its trick for the grey
zone in between is simple: `/var` is a dataset on the pool, mounted
over the ramdisk's `/var` early in boot. The OS stays disposable. The
grey zone becomes persistent. Nothing else changes.

Yggdrasil borrows that trick. When the `var_persist_enable` knob is set
in the site config, the image ships a small service that runs before
journald and before `sysinit.target`. It imports the pool if that has
not happened yet, takes the dataset named by `var_persist_dataset`
(`zroot/var` by default) and mounts it over `/var`.

## First boot

On the first boot after you enable the knob, the dataset is empty.
Mounting an empty dataset over a working `/var` would leave the system
without a package database, logs, or spool files. So the service
populates the dataset first: it copies the current live `/var` into the
dataset, and only then mounts it over `/var`. From that moment the
machine writes its grey-zone state straight into the pool.

## Every boot after that

One directory must never persist across an image change:
`/var/lib/dpkg`. It is the package database of the squashfs you booted.
If a dataset copied from an older ISO shadowed the package database of
a newer ISO, every package query would answer with the past. So the
service snapshots the image's copy of `/var/lib/dpkg` before the mount
and realigns it into the dataset after the mount, on every boot. The
package database always matches the running image. Everything beside
it, which is the part you care about, persists.

## What belongs in the persistent /var

The short list, learned from a real outage:

- Site configuration that boot services read, for example the exporter
  config under `/etc/yggdrasil`. Keep a copy under the dataset and
  restore it with the boot hook, or move readers to
  `/var/ygg/`.
- Custom scripts. `/usr/local/sbin` is on the disposable root; a copy
  in the dataset plus one restore line in the boot hook is cheaper than
  re-writing the script from memory at 3 a.m.
- Journal logs. After the first persistent boot, `journalctl --boot=-1`
  works across reboots, which is exactly what you want on the morning
  after an incident.
- Spools and queues. The recovery-stick export spool belongs here, so
  the stick builder and the pool do not depend on each other's boot
  state.

What does not belong: anything the image must own (package database,
udev state, and the like; the service handles the one dangerous case),
and any data that is already a ZFS dataset of its own. Datasets that
exist should stay datasets. The persistent `/var` is for the state that
has no dataset of its own.

## Enabling it

Site values live in `ygg.local.toml`, never committed:

```toml
var_persist_enable = true
var_persist_dataset = "zroot/var"
```

Create the dataset once on the host:

```
zfs create -o mountpoint=none zroot/var
```

The image generates `/etc/ygg/var-persist.conf` from these values and a
`ygg-var-persist.service` unit that runs before journald. On the next
boot the service populates the dataset and mounts it. Every boot after
that, your journal, your scripts, and your site configuration are
already there when the rest of the system wakes up.

## The limits

A persistent `/var` is not a backup. It lives on the same pool as
everything else, and the same block-clone wedge, disk loss, or fat
fingered `zfs destroy` takes it down with the rest. It removes the
small daily losses: the config you forgot to copy, the script you did
not commit, the journal that would have explained the crash. For the
big loss you still have the two backup legs, and you should still have
the two backup legs.
