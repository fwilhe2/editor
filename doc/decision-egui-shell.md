# Decision: a portable GUI shell, on egui

**Status:** proposed, 2026-08-23. The first decision record in this repository; if a second one is
ever needed, this is the shape to copy.

**Summary:** add `ui_egui/` — an [egui](https://github.com/emilk/egui)/`eframe` shell that is
deliberately **not** native to any platform, in exchange for building everywhere with no system
dependencies and being the first GUI in this project whose *behaviour* is verified in CI.

## Context

Six shells exist. Five of them are GUIs in some sense, and every one of them has the same hole in
it: **CI proves that they compile, and nothing else.** The workflows build XAML, SwiftUI, GTK and a
wasm page, and the only assertions anywhere near a GUI are the three UI-free smoke tests over the
*boundaries* (`ffi/csharp-smoke`, `FfiSmoke`, `ui_web/smoke.js`) plus the keymap unit tests. The
guide is blunt about it — "how any GUI *looks*: only on that platform" — and that honesty has a
practical cost:

- **No GUI regression can be caught automatically.** Wire a key to the wrong action in `ui_linux/`
  and every job stays green.
- **Every GUI needs its own operating system.** No contributor can run more than three of them
  without three machines, and no CI job runs any of them at all. `ui_windows/` and `ui_mac/` cannot
  even be built here.
- **The GTK shell needs system packages** (`libgtk-4-dev`, `libadwaita-1-dev`), which is why the
  shared jobs cannot say `--workspace` and why the lint job carries an `apt-get` step.

None of that is a flaw in the architecture; it is the price of the architecture's central claim,
which is that shells should be native. But it means the project currently has no answer to "does
this GUI still work?" other than a human opening it.

Separately, there is a gap in the matrix: someone on FreeBSD, on a Wayland-only system without
libadwaita, or who wants one binary that behaves the same everywhere, has the TUI and nothing else.

## The thesis this appears to contradict

The README argues that cross-platform toolkits make a real compromise, that native UIs are worth
paying for, and that agents have made paying for them affordable. Adding egui — the most
unapologetically non-native toolkit available — looks like conceding the argument.

It is not, and the distinction matters enough to write down:

**The thesis is about what an application should ship. This repository is about what the
architecture can drive.** The deliverable here is breadth: one core, front-ends that are as
different from each other as possible. An immediate-mode renderer that repaints from scratch every
frame and owns not one pixel of platform convention is the *furthest* point from `NSTextView`, and
therefore the strongest available test of the core's contract. If `get_viewport`, the observer trait
and `follow_cursor` survive GTK, WinUI, SwiftUI, the DOM *and* immediate mode, the contract is real
rather than a coincidence of retained-mode toolkits resembling each other.

The argument also gets a control group. Right now the README asserts that a native shell feels
better than a portable one, and the repository contains no portable one to compare against. After
this, it does — built from the same core, in the same style, by the same means — and a reader can
judge the claim instead of taking it. If egui turns out to feel fine, that is worth knowing too.

What must not happen is quiet reframing: this shell is a compromise, it is *chosen* as one, and the
README should say so where it lists the shells. A portable shell that gets described as "also
native" would cost the project its argument.

## Decision

Add `ui_egui/` (crate `editor-egui`, binary `edit-egui`), a Rust-direct shell depending on
`editor-core` as an ordinary Cargo dependency and reaching its platform through `eframe`.

It introduces no new class of shell — it is Rust-direct like the CLI, TUI and GTK app — but it does
add a second axis to the taxonomy, which the guide's one-dimensional table cannot express:

|  | Native to a platform | Portable by design |
|---|---|---|
| **Rust-direct** | `cli/`, `ui_tui/`, `ui_linux/`, planned `ui_qt/` | **`ui_egui/`** |
| **wasm-bindgen** | `ui_web/` | (egui on canvas — declined, see below) |
| **UniFFI foreign** | `ui_windows/`, `ui_mac/` | — |

Everything else about it is ordinary: it holds no editor state, renders from `get_viewport`, repaints
from the observer, and adds no capability the CLI does not already have.

### What "easy to cross-compile" actually means here, and what it does not

The motivation for this shell was stated as cross-compilation. That framing needs sharpening,
because taken literally it would break rule 9 of the guide for no benefit:

- **It does not mean building macOS or Windows artifacts on a Linux runner.** That remains
  unsupported and unwanted. macOS needs the Apple SDK and frameworks; Windows via
  `x86_64-pc-windows-gnu` would technically link for a pure-Rust winit app, but it is not the ABI
  anyone should ship. **Rule 9 stands, and `ui_egui/` is not an exception to it** — its Linux job is
  a native Linux build, and a `.exe` or an `.app`, if wanted, comes from its own runner exactly like
  `edit-tui` does.
- **It does mean one crate, no system dependencies, and identical source on every target** — so
  every runner can build it without an `apt-get` step, and, far more valuably, **one Linux runner can
  assert on its behaviour.** That is stronger than what was asked for: not "it compiles on Linux" but
  "it is tested on Linux, headlessly, in hundredths of a second."

The accurate word is *portable*, not *cross-compiled*, and the payoff is verification rather than
distribution.

### The thing that tips the decision

`egui_kittest` drives a real egui UI with no display server and no GPU, querying and clicking
widgets through AccessKit. Verified before writing this document, in a bare `docker.io/library/rust:1`
container with **zero apt packages installed** and with `DISPLAY` and `WAYLAND_DISPLAY` unset:

```
=== no apt packages installed ===
   Compiling eframe v0.36.1
    Finished `dev` profile in 1m 38s
=== headless kittest ===
test headless_egui_runs_with_no_display ... ok
test result: ok. 1 passed; 0 failed; ... finished in 0.02s
```

That test clicked a button by its accessibility label and asserted on what was rendered. No other
GUI in this repository can be driven that way on this machine, or in CI, at all.

It is worth being precise about what this buys, because it is easy to oversell: the harness runs the
full egui pass — including layout and text shaping with the same bundled fonts the real app uses —
and skips rasterisation. So it proves *behaviour and layout*, not pixels. Pixels are available too,
through the harness's snapshot feature, at the cost of a GPU or a software Vulkan stack in CI; that
is deliberately out of scope for the first version (see the plan's non-goals).

This is also the first GUI in the project an **agent** can verify end to end without a human looking
at a screen — which is on-thesis rather than beside it. The README's whole argument is that per-
platform labour got cheap while judgement stayed expensive; "open it and look" is the last big
chunk of unautomatable labour left in this repo, and this narrows it by one shell.

## Options considered

| Option | Bare-Linux build | Headless behaviour tests | Feel | Verdict |
|---|---|---|---|---|
| **egui / eframe** | yes, no system packages | yes, `egui_kittest` | its own, on every platform | **chosen** |
| Slint | yes | yes, testing backend | its own, or Qt-backed "native" style | declined |
| Iced | yes | no equivalent harness | its own | declined |
| Tauri | no — WebKitGTK dev packages | via a webdriver, heavily | the web's | declined |
| Qt (`ui_qt/`, already planned) | no — Qt dev packages | limited | close to native on KDE | keep, different purpose |
| Do nothing | — | — | — | rejected |

**Slint** was the closest call. It builds cleanly, it has a testing backend, and it would fit the
architecture fine. It was declined on two counts: its licensing (GPL, or a royalty-free licence with
conditions, or commercial) is a decision an MIT prototype should not have to make in passing, and it
introduces a `.slint` DSL plus a build step — a second language and a code generator for a shell
whose entire justification is that it is cheap to build and run everywhere. Worth revisiting if the
egui shell's *look* turns out to be the thing that grates.

**Iced** was declined on architectural grain rather than quality. Its Elm-style
`Message`/`update`/`view` loop wants the application to own its state; keeping a shell stateless
against that current is possible but is a fight, whereas immediate mode's "derive every frame from
whatever you have" *is* the rule this project already imposes on shells. It also has no harness
comparable to `egui_kittest`, which is the entire point of the exercise. (That last claim is a
judgement from documentation, not something built and measured here.)

**Tauri** would duplicate `ui_web/` — the renderer would be the same DOM, rewritten — and needs
WebKitGTK development packages on Linux, which is precisely the cost this decision is trying to
avoid.

**Qt is not replaced by this and remains planned.** The two have opposite purposes: `ui_qt/` exists
to reach a *second* set of native conventions (KDE/Plasma), `ui_egui/` exists to reach none of them.
If `ui_qt/` ever gets built, the taxonomy table above is where it goes — top-left, with GTK.

**egui compiled to wasm was considered and declined for now.** eframe supports it, and it would be a
few lines. It would also render a canvas with no DOM semantics, no text selection, weak IME and a
multi-megabyte download, and would sit next to a shell that does the web properly. Declining it is
the same judgement `ui_web/` was built on. The option stays open as a demonstration of portability;
it must never become the recommended way to run this editor in a browser.

## Consequences

**Good**

- The first GUI regression test in the project, running on every push, on the cheapest runner.
- A GUI that any contributor, on any OS, in any container, can build and test — including the
  machine this repository is developed on.
- One more data point for the core's contract, from the least retained-mode direction available.
- A fallback binary for platforms with no shell of their own.
- An honest comparison for the README's central claim.

**Bad, and accepted**

- **It will not feel native anywhere.** No menu bar on macOS, no Mica on Windows, no libadwaita, no
  system file dialog, no platform text-selection behaviour, no system fonts. This is the compromise
  the README describes, chosen on purpose, and it must be labelled as such wherever the shells are
  listed.
- **Accessibility is whatever AccessKit provides**, which is real but is not what any of the three
  native toolkits give for free.
- **+224 dependencies.** Measured, not guessed: adding the crate to a copy of this workspace takes
  the lock file from 295 entries to 543 (283 distinct crate names to 507). Nothing already there
  changes version — a dozen crates simply gain a second, older copy alongside the current one
  (`rustix` 0.38 next to 1.1, `windows-sys` 0.52 next to 0.59 and 0.61). The cost is a longer cold
  build, including in the lint job, which does compile everything; `Swatinem/rust-cache` already
  absorbs most of it in CI, and the whole eframe tree built from nothing in 1m38s on this machine.
- **A seventh shell to keep in step** when the core changes, and a seventh workflow.
- **A third version-coupling to police.** `egui`, `eframe` and `egui_kittest` release in lockstep and
  must all be on the same minor — alongside `uniffi`/`uniffi-bindgen-cs` and `wasm-bindgen`/its CLI.
  Mitigated the way the repo already mitigates this class of problem: depend on `eframe` and reach
  egui through `eframe::egui`, exactly as `crossterm` is reached through `ratatui::crossterm` and
  `gtk` through `libadwaita::gtk`. The dev-dependency on `egui_kittest` is the one that must be
  matched by hand, so it gets a comment at the pin.
- **egui's API churns.** Verified while writing this: 0.36 replaced `App::update(&mut self, ctx)`
  with `App::ui(&mut self, ui)` and changed every panel's `show()` to take `&mut Ui` instead of
  `&Context`. Essentially every tutorial, and almost certainly any code an LLM produces from memory,
  is pre-0.36 and will not compile. Budget for reading the version's own source at each upgrade.

**Neutral**

- eframe 0.36 declares `rust-version = 1.95`; current stable here is 1.97.1. The project has no MSRV
  and tests on current stable, so this changes nothing — but it is the first dependency with a floor
  that high, and it is worth knowing that the floor now comes from eframe rather than from us.
- eframe wants `wasm-bindgen ^0.2.126` on wasm targets; the lock file is on 0.2.127 and stays there
  when eframe is added — checked against a real resolve, not assumed. If a future eframe bumps it,
  `ui_web/build.sh` will simply demand a newer CLI — loudly, by design — rather than producing
  mismatched glue.

## What this does not change

- **Feature parity.** This shell exposes exactly what exists: open a file from `argv`, edit, move,
  undo/redo, save. It needs no new core capability and therefore no new CLI subcommand. If it ever
  wants one, the core and the CLI get it in the same change, as always.
- **Rule 9.** Artifacts are still built per OS. Nothing here is cross-compiled.
- **`ui_web/` remains the browser shell**, and `ui_qt/` remains planned.
- **The core.** No API is added, removed or modified by this decision.

## When to revisit

- If the egui shell's behavioural tests never catch anything in a year, the verification argument was
  wrong and the shell is carrying its cost for the taxonomy alone.
- If keeping up with egui's API churn costs more per release than the tests save, prefer Slint or
  pin hard and upgrade rarely.
- If a real user ever wants to *use* this editor, the answer stays a native shell; this one is the
  fallback, not the recommendation.

## Evidence behind this document

Everything asserted above was checked on 2026-08-23 rather than recalled:

| Claim | How it was checked |
|---|---|
| egui/eframe/`egui_kittest` current at 0.36.1, released in lockstep | `index.crates.io` sparse index |
| eframe builds with no system packages | `cargo build` in `docker.io/library/rust:1`, no `apt-get`, exit 0 |
| the exact feature list the plan prescribes compiles in *this* workspace | a stub `ui_egui` added to a copy of the repo; 2m19s from cold, exit 0 |
| headless GUI test, no display, no GPU | `egui_kittest` harness with `DISPLAY`/`WAYLAND_DISPLAY` unset, on the host and in the container |
| `egui::Context` is `Send + Sync` and can be the observer bridge | compile-time assertion plus a cross-thread `request_repaint()` |
| the 0.36 `App::ui` / panel API change | compiler errors against the real crate, then the crate's own source |
| +224 dependencies, nothing existing moved, `wasm-bindgen` unchanged | a copy of this workspace with the crate added, resolved and diffed |
| eframe's default backend is wgpu, glow is opt-in | the index entry's feature table |

The plan for building it is in [`plan-egui-shell.md`](plan-egui-shell.md).
