# yggdrasil

**One config file. A bootable, self-healing ZFS+LXC Debian spine.**

yggdrasil builds a live ISO that turns a bare machine into your
infrastructure: ZFS root, LXC containers, your services, your recovery.
If the host dies, the stick rebuilds it knowing everything the live
machine knew — because everything the live machine knew lives here.

- **Recovery-first**: ZFSBootMenu with optional wifi + ssh in the boot
  image. The old kernel boots when the new one dies, and you fix the host
  from your desk.
- **Site-private, repo-public**: one `ygg.local.toml` carries your proxy,
  keys, pools, and network. The repo carries the machinery and never your
  values.
- **Agent-native**: an agent with shell access can build, test, and ship
  the ISO end to end — see `.agents/skills/yggdrasil-iso/`.

## Quickstart

```bash
git clone https://github.com/yggdrasilhq/yggdrasil && cd yggdrasil
cp ygg.example.toml ygg.local.toml   # fill in your site values
sudo ./mkconfig.sh --profile both    # builds server + kde ISOs
./tests/smoke/run.sh                 # gates the ship
```

## Docs

- [docs/build-system.md](docs/build-system.md) — the generator, the laws,
  and where the bodies are buried.
- [docs/site/](docs/site/) — the full story.
- ZBM recovery image: [zbm/](zbm/).

GPL-3.0 — see [LICENSE](LICENSE).
