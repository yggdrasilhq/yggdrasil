# ADR-0001 — `yggdrasil-maker` is a libyggterm app

- **Status:** accepted, 2026-08-01
- **Supersedes:** the shell-script config wizard, and the Rust/Dioxus desktop app
- **Scope:** the front door to building a Yggdrasil image

## The philosophy this inherits

The maker is a **front door, never a proprietary format trap, and never charged
for.** Everything it does, `./mkconfig.sh` also does from a plain shell. That is
not a limitation of v0 to be grown out of — it is the design. The maker's job is
to make the existing build legible and reachable, not to become the only way in.

Two obligations follow, and they bind every flow specced below:

1. **Every flow must name the shell command it stands for.** The plan flow
   exists precisely so a user can read what would run and then go run it
   themselves. A flow that cannot show its command line does not ship.
2. **No format only the maker can read.** Configuration stays `ygg*.toml`,
   consumed by `scripts/toml-to-env.sh`. The maker never writes a config the
   shell path cannot build from.

## Decision

`yggdrasil-maker` is a CLI that runs **inside a yggterm terminal** and takes
over the viewport by declaring a web surface over OSC 7717 on its own PTY,
serving its UI from loopback HTTP. Run outside yggterm it degrades to serving
the same UI and printing its URL.

### Why this shape

- **The transport is the terminal byte stream.** A local session and a session
  on the far side of `ssh` behave identically, with no port juggling, no daemon
  to install and no second channel to secure. A plain terminal ignores an
  unknown OSC, so the degradation story is built into the wire format.
- **State is host-resident.** The app's state lives under
  `~/.yggterm/yggdrasil-maker/` on the host the CLI runs on — which is the host
  holding the checkout being built. yggterm is a pure renderer and persists none
  of it. This is what makes "open the maker for the checkout on that machine"
  mean something.
- **Egress follows the invoking host.** yggterm fetches a remote session's
  surface through that session's own tunnel, so `127.0.0.1` resolves on the
  machine running the build. Binding loopback is correct rather than limiting.
- **No path dependency on the yggterm source tree.** The entire contract is a
  byte format plus two environment variables. This is the single most important
  correction to the predecessor (see below).

### Surface type: viewport plus a mode toggle

One viewport surface with a segmented Home / Plan / Run switch, rather than
three surfaces or a sidebar. The three flows are one task at three depths, and
yggterm already owns the one control we must not duplicate — the ⌨ Terminal
toggle that puts the PTY back in front. Heartbeat re-declares never fight it.

### The alternative that was considered and not taken

yggterm also offers a **document surface**, where the app declares a widget
schema and the GUI renders it as ordinary shell DOM — no child webview, cheaper,
and faithful to `app screenshot` by construction. The web surface was chosen for
v0 because the run flow streams a log and the plan flow renders command lines,
both of which want real layout, and because the loopback UI is the half that
also works in a plain browser under degradation.

**The cost is real and should be recorded:** a web surface is invisible to
`server app screenshot` unless `--backend os` is passed, and costs two web
processes. If the flows below settle into forms and lists, the document surface
becomes the better substrate and this ADR should be revisited.

## Why the predecessors were dropped

- **The shell config wizard** (`scripts/mkconfig-tui.sh`, `docs/tui.md`,
  `scripts/mkconfig-legacy.sh`) — a whiptail/dialog questionnaire that wrote a
  `YGG_*` env file. It was already deleted from the tree before this work; it
  duplicated `mkconfig.sh`'s flag surface in a second place that could drift,
  and it could show a user nothing about what their answers would actually run.
- **The Rust/Dioxus desktop app** (`yggdrasil-maker/`, ~59 commits) — it took
  four of its dependencies, and a `[patch.crates-io]`, from a **sibling checkout
  of the yggterm repo by relative path**. A fresh clone of yggdrasil could not
  build it at all, which is a fatal property for the front door of a public
  build tool. It also carried its own window chrome, theming, icon rendering and
  five-platform release matrix — an entire desktop application's maintenance
  surface, to put a form in front of one shell script.

Both were purged from history rather than deleted at HEAD, because neither has
any successor value and the repository is young enough that the rewrite is
cheap.

## The target control-flow map

Everything the user asked for "in our viewport". v0 status against each.

### 1. Discover — **v0, implemented**

Home lists what the checkout actually contains: the `--profile` values parsed
out of `mkconfig.sh`'s own usage block, every `ygg*.toml` with its header
comment and knob count, the package lists, the hook count, built ISOs in
`./artifacts`, and whether `lb` is on PATH.

Nothing here has a hard-coded fallback. When the profile list cannot be parsed
the UI says so and shows the parse error, rather than presenting a remembered
`server|kde|both` that might no longer be true.

### 2. Plan — **v0, implemented**

Renders exactly what a build would run, having run none of it: the ordered
steps with their real argv, each tagged with what it really costs (seconds and
rootless / needs root / root and tens of minutes); the config delta against
`ygg.example.toml`, key by key; and the fully resolved environment, produced by
**really executing `scripts/toml-to-env.sh`** rather than by modelling it.

Profile resolution is faithful to the shell, including the two things that
surprise people: an omitted `--profile` falls back to the config's
`build_profile`, and the smoke stage receives the profile *unexpanded*, so
`both` is passed through as `both` rather than run twice.

### 3. Run — **v0, partially implemented; the cheap stage is real**

Executes `./mkconfig.sh --config … --profile … --dry-run --skip-smoke` and
streams stdout and stderr into the viewport with an honest
idle / running / done / failed state and the real exit code.

That command is a genuine execution of the repository's entry point: it resolves
the config, converts the TOML, sources the result, validates the profile and
prints the resolved build command lines. A bad profile really fails with exit 1.

**What it does not do is unpack a chroot.** The image stages need root and take
tens of minutes; they are shown on the plan as staged steps with their cost, and
are not run. There is no simulated progress anywhere in this app — the
completion percentage of a stage that has not started is not knowable, so it is
not drawn.

**Specced next:** run the real image stages with a privilege story (a `pkexec`
or `sudo` handoff, or a build container), per-stage progress parsed from
live-build's own output, and cancellation.

### 4. Profile and config editing — **specced, not in v0**

Home and Plan are read-only today. The target: edit knobs in the viewport with
the section banners and `# hint` enums already parsed out of the TOML driving
the widget types, writing back to `ygg.local.toml` in a form the shell path
still builds from. The parse layer this needs is already implemented and tested;
what is missing is the writer and its round-trip guarantee.

### 5. Flash and export — **specced, not in v0**

Write a built ISO to a USB device, and export a build's config plus manifest as
a reproducible bundle. Both need a device-enumeration and confirmation story
that is not worth guessing at before the run flow is finished; flashing the
wrong block device is unrecoverable, so this flow ships last and with the most
friction, not the least.

### 6. Artifact history — **specced, not in v0**

`./artifacts` is listed today. The target is a per-build record — config
snapshot, resolved environment, log, smoke result — kept host-resident under
`~/.yggterm/yggdrasil-maker/builds/`, so "what exactly went into this ISO" is
answerable after the fact.

## Consequences

- The app builds from a fresh clone with `cargo build`, no sibling checkout.
- Its dependency list is six crates and stays that way; if a flow needs a
  framework, that is a signal the flow belongs on the document surface instead.
- Discovery parses the shell scripts rather than restating them. When
  `mkconfig.sh` gains a profile, the app shows it with no edit here. The cost is
  that the parse can break, which is why it reports its error instead of
  falling back.
- The surface handshake is verified by capturing the emitted bytes; the
  remaining live check is that yggterm's GUI ingests the declare and paints the
  page, which needs a running desktop host.
