# ZBM — the recovery superpower

When the main kernel update dies, ZFSBootMenu boots the previous kernel —
and with the wifi + ssh layers, you fix the host from your desk instead of
the desk the host sits on.

- `build.sh` — pinned-tag build on the Debian kernel (driver match).
- `modules.d/` — opt-in dracut layers. Wifi creds and ssh host keys are
  injected from the site layer at build; they never enter this repository.
- Law: the default ZBM look is sacred (see zbm-dev/zfsbootmenu#660 for what
  happens when a layer gets loud). Silent layers only.
