# DESIGN.md

## Purpose

This file is the reusable visual and interaction source of truth for Yggdrasil applications.

Use it in two layers:

1. `Core System`: reusable design rules that should transfer cleanly across projects.
2. `Project Overlay`: product-specific vocabulary, workflows, and UI emphasis.

When this file is copied into another repo, the default move is:

- keep `Core System`
- replace or trim `Project Overlay`

Do not bury project-only nouns in the reusable sections.

## Core System

### Brand intent

Yggdrasil apps should feel:

- calm
- modern
- lightly premium
- youthful without being toy-like
- crisp rather than ornamental
- soft around the edges, but not soft-headed

They should not feel like:

- a Linux utility panel
- a web admin dashboard
- a noisy IDE clone
- a skeuomorphic toy
- a stack of nested cards inside more cards

The target impression is:

- one clear main workspace
- supportive chrome around it
- low-friction controls
- light, breathable, polished surfaces

### Visual structure

#### Main workspace

The main workspace is the focus.

- It should read like a calm sheet, canvas, or stage.
- In light mode it should generally be white or near-white.
- It may have a soft shadow and mild radius.
- It should feel like it is floating slightly above the surrounding chrome rather than being boxed into it.
- It should not be crowded by decorative headers, nested boxes, or redundant toolbars.
- Whatever the app’s core artifact is, it should feel native to the main canvas rather than pasted inside a widget frame.

#### Supporting chrome

The surrounding chrome should feel supportive, not dominant.

- Side rails should be lighter and quieter than the main canvas.
- A faint blue-to-green fresh tint over a muted neutral base is desirable.
- A light gradient plus blur system is preferred when the platform supports it.
- Rails should avoid heavy borders.
- The shell should feel visually unified rather than partitioned into harsh boxes.
- Titlebar, side rails, and utility surfaces should feel like one seamless scaffold around the floating main canvas.

#### Shape language

- Rounded corners are welcome, but should stay restrained and OS-friendly.
- Outer shell rounding should feel closer to modern KDE/Windows than to exaggerated mobile UI.
- In maximized state, outer window corner radius should collapse to zero.
- Inner radii should be smaller than outer shell radii.

### Color direction

Light mode is the primary reference unless a project explicitly says otherwise.

- Prefer white and pale blue-grey foundations.
- Accent color can lean clean blue.
- Background tint may gently lean sky-blue to green.
- Use contrast carefully; avoid washed-out unreadable controls.
- Keep the main canvas and supporting chrome visually coherent.

Avoid:

- muddy greys
- purple-heavy defaults
- overly opaque frosted layers that bury hierarchy
- gratuitous gradients inside the main content region

### Theming system

Yggdrasil shells should support a reusable visual theme editor.

- Theme editing should be centered on a small floating modal, not a full settings page takeover.
- The editor should feel Arc-like or Zen-like: compact, visual, tactile.
- The core interaction model is:
  - a preview pad
  - draggable color stops
  - a lightweight color library
  - a brightness control
  - a single grain dial control
- Double-clicking the preview pad should be able to add a color stop.
- The preview pad should use a visible grid, not a blank field, so stop placement feels intentional.
- Dragging color stops should live-preview the shell background.
- Light and dark shell modes should remain selectable independently of the custom gradient.
- Saving should persist the theme; cancel should revert live preview.
- Reset should always return to the project’s base shell theme, not an empty placeholder state.
- The active portable theme should be stored in `~/.yggterm/settings.json` under the `theme` object.
- If no custom colors exist, the shell should fall back to the system gradient cleanly.

#### Theme surfaces

- The outer shell background should be theme-driven.
- Supporting chrome should inherit the shell gradient subtly through transparency and blur.
- The main workspace should remain calmer and more neutral than the shell chrome.
- Theme accent can be derived from the dominant gradient stop for lightweight emphasis.
- The theme modal itself should not blur the background. The surrounding UI should remain clearly visible, with a calm blue active-state halo around the modal to signal focused editing.

### Typography

#### Interface font

- Linux: `Inter Variable`
- macOS/Windows: default platform system UI font

#### General text guidance

- small text must still feel antialiased and intentional
- avoid overly thin utility-rail typography
- headings should feel clean and editorial, not shouty
- labels should be concise and legible

Project overlays can define additional content fonts, such as terminal, code, map, or data fonts.

#### Preferred monospace font

- `JetBrains Mono` is the preferred monospace across all platforms unless a project explicitly overrides it.

### Control language

#### Segmented controls

Segmented pills are preferred for compact mode switches.

They should:

- clearly show the active segment
- have a clean outer shell
- avoid muddy selected states
- feel stable and precise

#### Primary buttons

Primary actions should look unmistakably clickable.

- blue background is acceptable for the main affirmative action
- white text
- clear contrast
- enough padding to feel intentional

If a user says “this does not look like a button”, that is a design failure.

#### Inputs

- Prefer clean rectangular or softly rounded input boxes.
- Avoid pill-shaped text fields unless there is a strong reason.
- Inputs must remain visible against the supporting chrome.

#### Search in chrome

- If the product has a global or sidebar search, the default preference is a centered search field in the titlebar.
- The search field should feel like part of the shell, not a floating badge.
- Search should generally be the visual anchor of the center titlebar slot.
- When an app has an active primary artifact such as a session, terminal, paper, or preview, its title should live in the titlebar to the left of the search field rather than consuming a duplicate header inside the main canvas.
- Hovering the title control should expose the summary via tooltip, and clicking it may open a compact dropdown with the fuller summary and related actions.
- Avoid showing both a titlebar title and a second in-canvas title card for the same artifact unless the inner canvas is itself an editor that must edit the title as content.

#### Context menus

Context menus should feel closer to modern Microsoft app menus than generic web popovers.

That means:

- open at the cursor
- modest radius
- clean light surface
- subtle shadow
- compact but breathable row sizing
- strong label clarity

Avoid:

- giant floating glass blobs
- top-left fallback placement
- labels that invent confusing product language

### Motion and interaction

Motion should be functional, not decorative.

- side panels can ease in and out
- notifications should stack and reflow smoothly
- drag-and-drop should show clear make-way affordances
- state changes should feel crisp, not rubbery

### Notifications

Notifications are reusable shell components, not one-off project afterthoughts.

- In-app toast notifications should be supported by default.
- Toasts should have clear tone coloring.
- Toast stacks should animate upward when items leave.
- Notification history panels are acceptable when the product benefits from persistent event history.
- Clear-one and clear-all actions should be supported when a notification panel exists.
- In-app toasts should usually sit horizontally centered near the top of the app, not pinned to a screen edge.
- Long-running work such as generation, caching, indexing, sync, or remote bootstrap should use reusable job notifications with a visible progress bar.
- Background jobs should not be silent; if the work may take more than a moment, the shell should make that work legible.
- Job notifications should coalesce by task identity instead of stacking duplicate progress cards.

### Update system

Update UX is a reusable shell concern, not project-specific glue.

- Direct-install update flows should reuse the notification and chrome systems.
- Installing an update must not immediately tear down a running productive workspace.
- Preferred behavior is:
  - install in the background
  - notify that the update is ready
  - expose an explicit restart affordance
- Update state should be readable from shell chrome without feeling alarmist.
- If a restart is required, the app should say so plainly instead of silently relaunching itself.

### Debug telemetry

Debug-only telemetry is a design-support component, not just an engineering detail.

- Instrumentation should help explain interaction failures such as drag, selection, layout, or context-menu issues.
- Debug telemetry should be local-first and easy to inspect.
- It should be safe to remove or gate behind debug builds without affecting the product UI.
- If a complex interaction is likely to be reused, the telemetry strategy should be reusable too.

### Drag and drop

If a project has drag-and-drop tree or list reordering:

- explicit `before / inside / after` snap zones are preferred
- a floating drag card is preferred over invisible drags
- hover affordances should show where the item will land
- adjacent snap boundaries must behave predictably
- multi-select drag can use stacked-card visuals
- the final placement must match the visible snap indicator exactly

### Preview surfaces

If a project has a conversation preview surface:

- preview reading mode and runtime/live mode should share one header system
- generated title and summary should be treated as refreshable navigational aids
- preview content should render like content, not raw log lines
- headings, bullets, task items, quotes, and code fences should each have distinct treatment
- overview/graph mode should feel structural, not like the same chat list in a second skin
- overview mode should highlight summary, counts, and message progression before full transcript detail

### Reusable shell guidance

If a project has:

- a main canvas
- left or right rails
- titlebar actions
- reorderable tree/list structures

then the shell should be designed as reusable primitives rather than one-off page markup.

Preferred reusable boundaries:

- drag/reorder engine
- drag ghost / drop-zone visuals
- titlebar primitives
- window control primitives
- rail/panel primitives
- menu and toast primitives
- update-state primitives
- telemetry hooks for interaction-heavy components

### Window chrome specifics

If a project owns its own titlebar/chrome:

- the main viewport should sit visually above a seamless titlebar + rail scaffold
- the preferred top-right control order is:
  - always-on-top
  - minimize
  - maximize / restore
  - close
- these controls should use crisp simple line icons
- minimize/maximize/always-on-top should stay neutral by default
- close should gain a red background with a white `X` on hover
- outer radii should disappear in maximized state

## Project Overlay Interface

Each project should define the following explicitly.

### 1. Main artifact

What is the main canvas actually for?

Examples:

- terminal
- map
- graph
- document
- dashboard

### 2. Navigation model

What lives in the left rail?

Examples:

- sessions
- folders
- machines
- topology nodes
- boards

### 3. Right rail modes

What modes can the right rail switch between?

Examples:

- metadata
- settings
- notifications
- inspector
- filters

### 4. Vocabulary

Define the user-facing nouns here, not in the reusable sections.

Examples:

- session
- terminal
- paper
- folder
- separator

### 5. Domain-specific control rules

Document:

- quick action labels
- context menu labels
- titlebar actions
- view toggles

### 6. Domain content typography

If the main artifact needs a special font, define it here.

Examples:

- terminal font
- map label font
- monospace editor font

## Project Overlay: Yggdrasil Maker

This section is intentionally project-specific.

### Main artifact

- a guided build studio for shaping, validating, and exporting Debian live systems

### Navigation model

- a left rail of saved setups first, with a secondary recent-artifacts section beneath them

### Preferred user-facing terms

- `Setup`
- `Build`
- `Artifact`
- `Profile`
- `Preset`

Avoid by default:

- `Workspace`
- `Project`
- `Session`
- `Pipeline`
- `Job`

### Navigation behavior

- the primary navigation object is the saved setup, not a file tree
- the top of the rail should prioritize active and recent setups
- the lower part of the rail may show recent artifacts, but those should remain secondary to setup creation and editing
- the rail should not read like a filesystem browser, logs viewer, or package manager
- a saved setup row should make the journey stage legible at a glance
- artifact rows should feel like outputs from setups, not peer objects with equal weight
- the `Recent Artifacts` section should be collapsible in v1
- `Recent Artifacts` may auto-expand when there is a fresh successful output, but should otherwise stay visually secondary
- on compact windows, collapse the right utility surface before collapsing the left rail
- the left rail should remain visible longer because it is the user’s orientation system

### Creation language

- primary quick actions should stay concrete and literal:
  - `New Setup`
  - `Build / Export`
  - `Open Artifact`
  - `Reveal Artifact`

### Shared YggUI Portability Rules

`yggdrasil-maker` is intentionally a simpler app than `yggterm`, but it should still act as a portability harness for the shared Ygg UI stack.

- do not fork the shell language just because the app is simpler
- prefer shared `yggui` primitives over app-local one-off replacements whenever a primitive exists or is being stabilized
- treat `yggdrasil-maker` as a proving ground for the reusable Ygg shell, not a separate visual system
- when a shell feature feels too heavy for maker, simplify the behavior, not the design language

### Shell Theme And Background

The app should inherit the Ygg shell treatment rather than inventing a flatter substitute.

- keep the shared interface font and monospace defaults from the core system
- support the `yggui` custom window background system, including the calm tinted shell background, blur, and sleek surface treatment
- prefer an Arc-like shell-mode or theme selector with a sane default rather than a raw settings form
- defaults should always look finished, calm, and premium before any user theme customization
- the main workspace should stay calmer and more neutral than the surrounding shell chrome
- maker may ship with a narrower theming surface than yggterm, but the underlying shell language should remain compatible
- in v1, keep the full shared theming system under the hood but expose only a minimal selector surface

### Custom Titlebar

The app should use the shared Ygg custom titlebar direction, not generic native chrome plus app content underneath.

- use the Ygg custom titlebar as the default reference
- titlebar content should stay concise and operational:
  - app identity
  - active setup name
  - current journey stage or build status
  - the primary shell-truth toggle or equivalent utility affordance
- the titlebar should feel like part of the shell scaffold, not a separate toolbar
- if shared `yggui` titlebar primitives exist, prefer them over maker-local markup

### Header behavior

- the main header system should be shared between the titlebar and the main studio canvas
- the setup name in chrome is the primary re-entry anchor
- the journey stage should stay visible without duplicating loud status cards
- shell-truth access should always be nearby, but it should not permanently crush the main canvas on compact windows
- a compact shell should prefer a toggleable utility overlay or drawer rather than a permanently docked right rail
- header copy should remain operational and brief, never aspirational or marketing-heavy

### Alt / Meta-Key Type System

`yggdrasil-maker` should preserve the Ygg keyboard-discovery language even if the action set is smaller than in `yggterm`.

- `Alt` should remain the entry point into visible command-hint mode
- hint chips should appear on the live controls they target
- maker can use a smaller command vocabulary, but it should not invent a different meta-key grammar
- overlays should stay lightweight, reversible with `Esc`, and compatible with the broader Ygg shell expectations

V1 command vocabulary:

- `New Setup`
- `Build / Export`
- `Shell Truth`
- `Focus Studio`

### Observability And App Control

The app should keep the Ygg local observability posture because this repo is also testing the portable shell stack.

- retain the dtrace-like local observability and interaction-debugging mindset from the Ygg ecosystem
- maker should stay compatible with the `yggui-app-control` style of inspection where practical, such as app state, focus, and screenshot-oriented debugging
- debug instrumentation should explain layout, overlay, and interaction failures without contaminating the product UI
- local observability is part of the design system because it helps stabilize reusable shell behavior across apps

### Main workspace behavior

- the main workspace should feel like a guided studio, not a dashboard mosaic
- the main task is shaping a setup toward a truthful build/export outcome
- the canvas should privilege sequence, clarity, and confidence over dense operational detail
- the workspace should feel compatible with Ygg shell primitives while still being quieter and simpler than yggterm
- the guided flow should remain explicit:
  - `Outcome`
  - `Profile`
  - `Personalize`
  - `Review`
  - `Build`
  - `Boot`
- each stage should feel like progress toward a real machine, not like a tabbed settings taxonomy
- the main canvas should avoid nested card stacks wherever a simpler sectional layout will do

### Artifact and export surfaces

- artifact surfaces should feel like outputs of setups, not like detached files
- exported artifacts should remain legible, revealable, and easy to inspect from the shell
- build truth, artifact manifests, and output paths should use the shared utility-surface logic rather than ad hoc panels
- successful completion should advance into a dedicated post-build success step, not collapse into a toast or raw manifest dump
- the success step should feel like “artifact ready” rather than “job complete”
- non-Linux and export-only cases should still feel honest and complete, not like degraded failure states
- raw logs should remain available, but should never be the first thing a normal user sees after success

### Right rail modes

The canonical utility-surface modes for maker are:

- `Config`
- `Plan`
- `Build`

Rules:

- avoid a vague `Inspector` label in the product UI
- avoid splitting `Artifacts` into a peer top-level mode unless the artifact surface grows beyond what `Build` can hold cleanly
- `Build` may contain artifact-manifest, output-path, and reveal/open affordances as part of one truthful output surface
- on compact windows, these modes should move into a toggleable overlay or drawer rather than stay permanently docked

### Success moment

The canonical success moment for maker is a dedicated screen or journey step after `Build`.

It should:

- clearly announce that the artifact is ready
- make the primary next actions obvious
- summarize proof and output truth without dumping raw logs first
- feel calm, confident, and slightly celebratory

It should not:

- rely on a toast as the main success communication
- force the user to parse raw manifest text before they can act
- look like a generic CI job completion panel

V1 primary actions:

- `Reveal Artifact`
- `Open Build Details`
- `Start Another Setup`

V1 success content should include:

- artifact name and output type
- profile used
- smoke-test result or equivalent proof summary
- output path
- the three primary actions above

### Build details surface

The `Build` utility mode should show:

- current build status
- the truthful next action when idle, running, failed, or complete
- artifact manifest or export summary when available
- raw event stream below the summary layer, not above it

The order should be:

1. status
2. proof / outcome summary
3. action affordances
4. manifest or output details
5. raw stream

### Domain content typography

Maker should stay conservative and crisp.

- use the shared interface font for all standard UI copy
- use the shared monospace font for config, paths, manifests, and build/event text
- do not introduce a third decorative display face in v1
- success and stage headlines should rely on scale, spacing, and copy quality rather than novelty typography
