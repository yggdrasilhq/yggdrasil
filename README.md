# yggdrasil

`yggdrasil` builds the host at the center of the Yggdrasil ecosystem.

It produces a Debian sid live ISO for the machine that becomes your storage spine, your LXC host, your recovery anchor, and often the quiet box in the corner that the rest of your setup eventually depends on.

This repository is for the server build pipeline itself.
It is not the only way into the ecosystem, and it is not meant to trap you inside a wrapper.
If you want a guided experience, use `yggcli`.
If you prefer direct control, stay here and drive `mkconfig.sh` yourself.

## The Ecosystem In One View

A simple mental model:

- `yggdrasil` builds the server ISO
- `yggclient` configures the machines you use every day
- `yggsync` moves data between them
- `yggcli` is the optional front door that writes the native config files for all of the above
- `yggdocs` is the manual, field guide, and operational memory

```text
                         +----------------------+
                         |       yggdocs        |
                         | quickstart, wiki, dev|
                         +----------+-----------+
                                    |
                                    v
 +-------------+           +--------+--------+           +-------------+
 |  yggclient  |<--------->|    yggsync      |<--------->|  yggclient  |
 |   laptop    |           | sync engine     |           |    phone    |
 +------+------+           +--------+--------+           +------+------+
        \                           |                           /
         \                          |                          /
          \                         v                         /
           +------------------------------------------------+
           |                  yggdrasil                     |
           | Debian sid ISO, ZFS, LXC, host runtime         |
           +-------------------------+----------------------+
                                     ^
                                     |
                               +-----+-----+
                               |  yggcli   |
                               | guided UX |
                               +-----------+
```

Mermaid version:

```mermaid
flowchart TD
  D[yggdocs<br/>quickstart, wiki, dev]
  C[yggcli<br/>guided config UX]
  S[yggdrasil<br/>server ISO + host runtime]
  Y[yggsync<br/>sync engine]
  L[yggclient laptop]
  P[yggclient phone]

  C --> S
  C --> L
  C --> P
  C --> Y
  L <--> Y
  P <--> Y
  Y <--> S
  D --> C
  D --> S
  D --> L
  D --> P
```

## What A Yggdrasil Server Is

A Yggdrasil server is not just a generic Debian install.
It is the machine you prepare to do the heavy, patient work:

- import and mount your ZFS pool correctly
- bring up LXC with the expected defaults
- autostart the containers that matter
- remain bootable and understandable from a USB image
- give you a reproducible host baseline instead of an improvised snowflake

For many operators, this becomes the box that eventually holds:

- storage
- containers
- backup targets
- sync destinations
- reverse proxies
- service front doors

That is why the ISO matters.
It is not wallpaper.
It is the first disciplined step in the rest of the system.

## Who This Repo Is For

Use this repository directly if:

- you are comfortable editing config files
- you want full control over build inputs
- you want to script builds without a TUI
- you want to understand the host composition plainly

Use `yggcli` if:

- you want sensible defaults first
- you want a guided configuration flow
- you are new to the ecosystem
- you want to generate config files before touching the raw build knobs

The important design rule is this:

- `yggcli` is optional
- the native config files stay real and editable
- the path from beginner to operator stays open

## Repository Boundaries

- `yggdrasil`: ISO composition, hooks, package lists, host runtime wiring
- `yggcli`: guided configuration and automation entrypoint
- `yggclient`: endpoint automation for laptops, desktops, and Android/Termux
- `yggsync`: sync engine and job runner
- `yggdocs`: quickstart, wiki, recipes, and developer references

## Local Config

Use a local untracked config file.

- tracked example: `ygg.example.toml`
- tracked template preserving the old infrastructure shape: `ygg.legacy-infra.example.toml`
- local file: `ygg.local.toml` (gitignored)

`mkconfig.sh` accepts `--config` with either:

- TOML (`*.toml`)
- env files containing `YGG_*` key/value pairs

That means a power user can stay here permanently without `yggcli`, while a new user can begin with `yggcli` and later continue by hand.

## Quick Start

### Guided path with `yggcli`

If you want the smoother path, let `yggcli` write the local config and then build from this repo:

```bash
yggcli --bootstrap --write-defaults
yggcli --workspace ~/gh --build-iso --profile server
```

### Direct path with `mkconfig.sh`

If you want to work here directly:

```bash
cp ygg.example.toml ygg.local.toml
./mkconfig.sh --config ./ygg.local.toml --profile server
```

To build both server and KDE variants:

```bash
./mkconfig.sh --config ./ygg.local.toml --profile both
```

To skip smoke tests during iteration:

```bash
./mkconfig.sh --config ./ygg.local.toml --profile server --skip-smoke
```

## First Server Guidance

For a first Yggdrasil server, the recommended path is conservative:

1. set the host basics first
2. keep `apt_proxy_mode = "off"`
3. build and boot the host
4. validate ZFS import, LXC defaults, and container behavior
5. add an apt-proxy container later if you actually need faster rebuilds
6. switch later builds to explicit proxy mode

That sequence is deliberate.
The first success should be legible.
Speed comes after trust.

Kernel policy:

- `with_lts = false` uses Debian unstable's current kernel line
- `with_lts = true` switches to the compatibility-pinned kernel path
- that compatibility path is useful when a driver or DKMS stack needs a steadier ABI

## Examples

### 1. First server with defaults

```bash
cp ygg.example.toml ygg.local.toml
./mkconfig.sh --config ./ygg.local.toml --profile server
```

Use this when you want to produce the first ISO before tuning every dial.

### 2. Automated server build with explicit overrides

```bash
yggcli --workspace ~/gh \
  --set yggdrasil.hostname=mewmew \
  --set yggdrasil.net_mode=dhcp \
  --set yggdrasil.static_dns="192.168.1.1 9.11.11.11" \
  --set yggdrasil.with_lts=false \
  --set yggdrasil.with_nvidia=false \
  --build-iso --profile server
```

Use this when a CI job, agent, or repeatable script is driving the build.

### 3. Direct build from a local TOML profile

```bash
./mkconfig.sh --config ./ygg.local.toml --profile server
./mkconfig.sh --config ./ygg.local.toml --profile kde
```

Use this when you want the server and desktop ISOs to stay separate and explicit.

## What The Build Produces

The normal output is a bootable live ISO that carries the host runtime choices baked into this repository:

- Debian sid userspace
- current Debian kernel line
- ZFS userspace and DKMS integration
- LXC defaults and autostart hooks
- optional KDE profile when requested
- optional SSH key embedding when configured

## Privacy And Public Hygiene

Do not commit:

- private hosts
- internal domains
- tokens
- secrets
- local-only infrastructure names

Use generalized examples in tracked files.
Keep your real values in `ygg.local.toml` and other gitignored local config files.

## Where To Read Next

- `yggdocs` for the real quickstart and recipes
- `yggcli` if you want the guided path
- `AGENTS.md` if you are working on build and ops automation in this repo

## License

Apache-2.0
