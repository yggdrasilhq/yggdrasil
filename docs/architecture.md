# Architecture

## Release Targets

- `server`: minimal host profile.
- `kde`: desktop/laptop profile.

Both profiles are mandatory and follow identical release gates.

## Runtime Services to Validate

- `ygg-import-zpool-at-boot.service`
- `ygg-lxc-autostart.service`
- `ygg-infisical-ensure.service`
- `nvidia-firstboot.service` (when NVIDIA enabled)

## LXC Baseline

- `/etc/lxc/lxc.conf` must point to `/zroot/lxc`
- `/etc/lxc/default.conf` must contain macvlan template defaults
