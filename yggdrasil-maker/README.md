# yggdrasil-maker

The front door to building a Yggdrasil image, in the yggterm viewport.

Run it inside a yggterm terminal and it contributes its panes to the GUI. Run it
anywhere else and it serves the same UI over loopback and prints the URL.

## It ships no UI code inside yggterm

The maker used to declare a **web surface** — a child web engine, with its own
HTML, CSS and JS, to draw a form and three lists. That is Tier B in libyggterm's
app-architecture spec, and the spec is blunt about what Tier B is: **a cost,
never a choice.** It buys a foreign engine and pays for it forever — no faithful
screenshot into the surface, no `dom-eval`, no inherited theme, its own
compositing and lifecycle, two web processes per surface.

Nothing this app draws needs an engine. A checkout survey, a plan and a build log
are lists, labels and a couple of buttons. So it declares a **widget schema**
instead and yggterm paints it as ordinary shell DOM: themed with the terminal,
screenshot-faithful by construction, and drivable by the same automation as
everything else. Every widget kind it uses already existed — nothing was
invented for it, and there is no context menu anywhere in it.

The loopback web UI is unchanged and still serves the standalone path, because
the app is a normal local web app that happens to know how to ask for a surface.

```sh
cd yggdrasil-maker
cargo build --release
./target/release/yggdrasil-maker            # from anywhere inside the checkout
```

| Flag | Meaning |
| --- | --- |
| `--repo PATH` | Checkout to drive. Default: search upward from the current directory. |
| `--port PORT` | Fixed loopback port. Default: let the kernel choose. |
| `--print-url` | Print the URL and exit without asking for a viewport. |

## What it does

**Home** — what this checkout actually contains: the `--profile` values parsed
from `mkconfig.sh`'s own usage block, every `ygg*.toml` with its knobs, package
lists, hook count, built ISOs, and whether `lb` is installed.

**Plan** — exactly what a build would run, having run none of it: the ordered
command lines with what each really costs, the config delta against
`ygg.example.toml`, and the resolved environment produced by really executing
`scripts/toml-to-env.sh`.

**Run** — executes `./mkconfig.sh --dry-run --skip-smoke` for real and streams
the log with an honest running / done / failed state and the real exit code. It
does not unpack a chroot: the image stages need root and tens of minutes, and
are shown on the Plan tab as staged steps. There is no simulated progress
anywhere in this app.

## The promise

The maker is a front door, never a proprietary format trap, and never charged
for. Everything it does, `./mkconfig.sh` also does from a shell — which is why
the Plan tab shows you the command lines rather than hiding them.

Design notes and the full control-flow map, including the flows not yet built:
[`docs/adr-0001-libyggterm-app.md`](docs/adr-0001-libyggterm-app.md).

## Licence

GPL-3.0-or-later, with the rest of the repository.
