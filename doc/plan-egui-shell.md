# Plan: building `ui_egui/`

The implementation plan for [`decision-egui-shell.md`](decision-egui-shell.md). Read that first —
it is where the *why* lives, including why this shell is deliberately not native and why "portable"
is the right word rather than "cross-compiled".

Everything below was checked against egui **0.36.1** on 2026-08-23. The API changed materially in
that release; see the traps.

## What is being built

```
ui_egui/
  Cargo.toml            editor-egui, binary edit-egui
  src/main.rs           argv, the editor, the window, the observer      ~90 lines
  src/app.rs            one frame: render from the viewport, route input ~250 lines
  src/keymap.rs         egui::Event → UiAction, widget-free, unit-tested ~180 lines
  src/layout.rs         points ↔ line/column arithmetic, unit-tested     ~140 lines
  tests/behaviour.rs    egui_kittest driving the real UI, headless       ~180 lines
.github/workflows/egui.yml
```

Roughly the size of `ui_web/`, and structured the same way on purpose: the repetition across shells
is what makes the seventh one a transcription instead of a design problem.

## Stage 1 — the crate, and the versions

**Workspace** (`Cargo.toml`): add `ui_egui` to `members`, and to `[workspace.dependencies]`:

```toml
# The portable GUI shell. egui is reached through `eframe::egui`, never as a direct
# dependency, so the two versions cannot drift — the same rule as `ratatui::crossterm`
# and `libadwaita::gtk`.
#
# Not the default features: those select the wgpu backend and a large graphics stack
# this editor has no use for. glow runs on software GL, which is what a VM or a remote
# session actually offers. accesskit is put back by hand — it is dropped along with the
# defaults, and it is what makes the shell reachable by a screen reader (and what the
# test harness queries).
eframe = { version = "0.36", default-features = false, features = [
  "accesskit", "default_fonts", "glow", "wayland", "x11",
] }
# egui, eframe and egui_kittest are released in lockstep and must be on the same minor.
# eframe re-exports egui so that pairing polices itself; this one cannot be re-exported,
# so it is the version to check by hand when eframe moves.
egui_kittest = "0.36"
```

**`ui_egui/Cargo.toml`**: `editor-core.workspace = true`, `eframe.workspace = true`,
`[dev-dependencies] egui_kittest.workspace = true`, one `[[bin]] name = "edit-egui"`.

**Acceptance**

```sh
cargo build -p editor-egui          # no apt-get, on any of the three OSes
cargo clippy --workspace --all-targets -- -D warnings
```

That exact manifest — this workspace, plus a stub `ui_egui` with the feature list above — was
resolved and compiled before this plan was written: 2m19s from cold, exit 0. So stage 1 should be
uneventful, and if it is not, the dependency set is the thing that changed.

Then diff `Cargo.lock` and read what moved. Feature unification across a workspace is silent: if
adding eframe bumps a version something else depends on, this is the moment to see it. On a trial
resolve of exactly the manifest above, the expected shape is:

- 295 → 543 entries (283 → 507 distinct crate names), so **+224 dependencies**
- **no existing version changes**; a dozen crates gain a second, older copy beside the current one
  (`rustix` 0.38 with 1.1, `windows-sys` 0.52 with 0.59 and 0.61, `rustc-hash` 1.1 with 2.1)
- `wasm-bindgen` stays on 0.2.127, so `ui_web/`'s CLI pin is untouched

Anything beyond that is a surprise worth understanding before going further — particularly a
`wasm-bindgen` bump, which would mean reinstalling the CLI that `ui_web/build.sh` insists on.

## Stage 2 — window, editor, observer

`main.rs` follows `edit-tui` and `edit-gtk` exactly: one path argument, `Editor::open` (which fails
on a missing file rather than inventing a buffer — `edit new` is how files are created), usage text
to stderr and exit 2 when it is missing.

The observer bridge is the simplest of the six, and worth noticing: `egui::Context` is `Clone`,
`Send` and `Sync` (verified with a compile-time assertion and a cross-thread call), so the notifier
can hold one directly. No channel as in GTK, no `AtomicBool` as in the TUI and wasm shells —
`request_repaint()` already coalesces, because it sets a flag that the next frame clears.

```rust
struct Notifier(egui::Context);

impl EditorObserver for Notifier {
    fn state_changed(&self) {
        self.0.request_repaint();
    }
}
```

Wire it inside eframe's creation closure, where the context first exists:

```rust
eframe::run_native(
    "Editor",
    eframe::NativeOptions::default(),
    Box::new(move |cc| {
        editor.set_observer(Arc::new(Notifier(cc.egui_ctx.clone())));
        Ok(Box::new(App::new(editor)))
    }),
)
```

**Do not** ask for continuous repaints (`request_repaint_after` on a timer, or a
`viewport.with_continuous`-style setting). An immediate-mode shell that redraws unconditionally is
polling, and rule 4 says shells never poll. It should idle at zero frames per second with the
document untouched.

**Acceptance:** the window opens on a Linux desktop and shows an empty frame; `cargo test -p
editor-egui` still passes; the process is idle in `top` when nothing is happening.

## Stage 3 — rendering from the viewport

One `egui::CentralPanel` for the text and one `TopBottomPanel` for the status line. The text is
*painted*, not put in a widget:

```rust
let font = egui::FontId::monospace(14.0);
let (char_width, line_height) =
    ui.fonts(|f| (f.glyph_width(&font, ' '), f.row_height(&font)));
```

Both exist on `epaint::Fonts` in 0.36 and are measured **every frame**, for the same reason the web
shell re-measures its probe: egui's zoom factor changes them and nothing announces it.

Then the same shape as every other shell:

1. `visible_lines` from the panel's height and `line_height` (floored at 1, or the first frame — laid
   out before any size is known — asks the core for an empty range).
2. `get_viewport(scroll_offset, scroll_offset + visible_lines)`.
3. One `painter.text(...)` per returned line.
4. The caret as `painter.rect_filled(...)` at `layout::caret_offset(line, column)`, drawn only when
   the cursor is inside the returned window — when it is scrolled away there is nowhere honest to put
   it.
5. The status line: name, a dirty marker, `Ln`/`Col` (+1 for display — the core is 0-based and stays
   that way), total lines, char count, and undo/redo shown as disabled when `can_undo`/`can_redo` say
   so.

**Painting the text costs it its accessibility node, and that has to be paid back.** Verified while
building stage 3: a `Painter::text` call produces no widget, so a screen reader — and
`egui_kittest`, which reads the same AccessKit tree — finds an empty window. Claim the text area with
`ui.allocate_rect(rect, Sense::click())` and hand the response a
`WidgetInfo::labeled(WidgetType::Label, true, visible_lines.join("\n"))`. That makes the document
both announceable and queryable, and stage 4 needs the response anyway to locate a click. Without it
stage 5 cannot assert on anything the user can see, which would take most of this shell's
justification with it.

Two widgets are **banned in this shell**, both for rule 1:

- **`egui::TextEdit`** owns a `String` and keeps a `TextEditState` in egui's memory. It is this
  toolkit's `GtkTextView`, `TextBox` and `contenteditable`, and using it would give the project two
  documents that disagree.
- **`egui::ScrollArea`** owns a scroll position. The core owns `scroll_offset`; a second one would
  make this the only shell that scrolls by a different rule. Draw a scrollbar by hand later if one
  is wanted (it is a non-goal for v1), and keep the offset in the core.

**Acceptance:** open a long file, and the visible text matches
`edit view FILE --start N --lines M` for the same range — remembering that the CLI is 1-based and the
core is not.

## Stage 4 — input

`keymap.rs` takes `egui::Event` and returns `Option<UiAction>` — widget-free, and testable with no
window, because `egui::Event` is a plain data type. `UiAction`/`Request` mirror `ui_web/src/keymap.rs`
(`Insert`, `Backspace`, `Move`, `Save`, `Undo`, `Redo`, `Quit`).

Three things about egui's input model decide the shape:

- **Text and keys arrive separately.** Printable input is `Event::Text(String)`; named keys and
  chords are `Event::Key { key, modifiers, pressed, repeat }`. That split does most of the work the
  web shell's "one non-control character is text" heuristic had to do by hand.
- **`Modifiers::command` is already the Ctrl/⌘ split** — ⌘ on macOS, Ctrl elsewhere. It is exactly
  `ui_web`'s `primary`, resolved by the toolkit instead of by the shell.
- **Act on `pressed: true` only.** The same trap as the TUI's `KeyEventKind::Press`, arriving by a
  different route; handling both edges types everything twice.

Then the rules that are already written down elsewhere in this repo:

- **A `command`-modified key must never also insert text.** Drop `Event::Text` when
  `modifiers.command` is set, even if the platform is believed not to deliver it — this is the bug
  that has appeared in three shells already.
- **`follow_cursor` is called from input handling only, never while drawing.** In immediate mode
  everything happens in one function, so this needs deliberate ordering: handle events, call
  `follow_cursor(visible_lines)`, *then* read `scroll_offset` and paint. Calling it during the paint
  makes a wheel scroll away from the caret snap straight back.
- **Wheel scrolling goes to `set_scroll_offset`**, converted from `input.smooth_scroll_delta` by
  `layout::wheel_lines`, so this shell scrolls by the core's rule like every other one.
- **Clicks place the caret** through `layout::position_at` and `editor.set_cursor` — free, because
  the core already has the API.
- **Ctrl/⌘+Q** warns once on a dirty document and quits on a second press, matching the TUI, and
  closing the window is intercepted the same way (`ctx.input(|i| i.viewport().close_requested())` plus
  `ViewportCommand::CancelClose`). The "warned once" flag is presentation state and may live in the
  shell — the same exception the TUI's quit prompt already takes.

`layout.rs` is `ui_web/src/layout.rs` in points instead of pixels: `caret_offset`, `position_at`,
`visible_lines`, `wheel_lines`, `scrolled_by`, with a non-zero floor on both metrics so the first
frame cannot divide by zero.

**Acceptance:** `cargo test -p editor-egui` covers both modules with no display; typing in the real
window matches the TUI.

## Stage 5 — the part that justifies the shell

`tests/behaviour.rs`, using `egui_kittest::Harness` to drive the real UI headlessly. This is the
first behavioural GUI test in the project, so it is worth writing properly rather than as a token:

- type text, and assert on `editor.text()` *and* on what was rendered
- Enter, Backspace, arrows in all four directions
- undo and redo, including that redo is gone after a fresh edit
- scrolling a document taller than the viewport, and that the caret follows
- the status line's line/column, which is where the 0-based/1-based mistake would show up
- a `command`-modified key that must not type its letter

The harness runs the full egui pass — layout and text shaping with the same bundled fonts the app
uses — and skips rasterisation, so it proves behaviour and layout, and says nothing about pixels.
Write that limit into the test module's doc comment, the way `ui_web/smoke.js`'s zero-geometry caveat
is written down; the next person needs to know what a green run does not cover.

Verified already, so the risk here is low: a probe harness clicked a button by accessibility label
with `DISPLAY` and `WAYLAND_DISPLAY` unset, in 0.02s, both on this machine and in a bare
`docker.io/library/rust:1` container.

**Acceptance:** `env -u DISPLAY -u WAYLAND_DISPLAY cargo test -p editor-egui` passes, and breaking a
keymap entry on purpose makes it fail.

## Stage 6 — CI

`.github/workflows/egui.yml`, modelled on `tui.yml`, because this shell has the same property the
TUI does — pure Rust, every platform, no cross-compilation:

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
steps:
  - test:   cargo test -p editor-egui --all-targets
  - build:  cargo build --release -p editor-egui
  - upload: target/release/edit-egui{,.exe}
```

No system-package step anywhere, which is the point — verified in a container with none installed.
The three-OS matrix is for artifacts and for platform-specific surprises; the *verification* is
complete on the Ubuntu job alone.

Two workflow-level notes:

- `core-cli.yml`'s test job names its crates, so it needs no change. Its **lint job does say
  `--workspace`**, so clippy will now compile eframe on every push — the cost lands there, and
  `Swatinem/rust-cache` is already in place to absorb it.
- Nothing about `ui_web/` changes, but watch the first run of `web.yml` after the lock file moves.

## Stage 7 — documentation

The change is not finished until the repo describes itself correctly:

- **`README.md`** — a badge, a row in the layout table, a build section, and the honest sentence in
  "Status and limits": this shell is portable and *not* native, chosen as a compromise, and it is the
  one whose behaviour CI actually tests. The "Why" argument must not be softened to accommodate it;
  if anything it gets a sharper edge, because now there is something to compare against.
- **`CLAUDE.md`** — a `## The portable shell (ui_egui/, binary edit-egui)` section next to the
  others, carrying the invariants: no `TextEdit`, no `ScrollArea`, no continuous repaint,
  `follow_cursor` from input only, metrics re-measured per frame, and the version lockstep.
- **`doc/shared-core-native-shell.md`** — three small edits, all of which are things the guide
  currently gets slightly wrong now that this shell exists:
  1. §2 rule 1's list of document-owning widgets gains `egui::TextEdit`.
  2. §5's verification table gains a row: an immediate-mode shell's behaviour is testable headlessly,
     on any host, which is the one gap that table currently reports as unfixable.
  3. §9's "never cross-compile" gains a sentence saying that a portable toolkit is not an exception
     to it — it centralises *verification*, not *distribution*.
- Update the shell count in both documents ("six front-ends" → seven).

## Non-goals for the first version

Written down rather than implied, per the checklist:

- **No egui-on-wasm build.** `ui_web/` is the browser shell; a canvas would be a worse one. See the
  decision document.
- **No snapshot (pixel) tests.** `egui_kittest`'s `snapshot` + `wgpu` features render real frames and
  diff PNGs, but need a GPU or a software Vulkan stack in CI and produce diffs that move with driver
  versions. Revisit once the behavioural tests have proved themselves.
- **No file dialog.** The path comes from `argv`, exactly as in `edit-tui` and `edit-gtk`, so this
  shell needs no new core capability and therefore no new CLI subcommand. `rfd` with its
  `xdg-portal` feature is the option if this ever changes — not its GTK3 backend, which would put a
  system dependency back.
- **No scrollbar**, no selection, no clipboard, no IME, no horizontal scrolling. Same gaps as the
  other shells; the prototype's scope is unchanged by this addition.
- **No theming work** beyond following the system light/dark preference, which eframe offers for
  nearly nothing.

## Traps, all of them found before writing a line of the shell

- **The 0.36 API is not the API you remember.** `App::update(&mut self, ctx: &Context, frame)` is now
  `App::ui(&mut self, ui: &mut Ui, frame)`, and `CentralPanel::default().show(ctx, …)` now takes
  `&mut Ui`. Every tutorial and almost any generated code will be pre-0.36 and will fail with
  `E0407: method 'update' is not a member of trait 'App'`. Read
  `~/.cargo/registry/src/*/eframe-0.36.1/src/epi.rs` rather than trusting recall — that is how this
  was found. Two more of the same kind, each found by the compiler rather than by reading:
  **`TopBottomPanel` and `SidePanel` are gone**, replaced by one `Panel` type
  (`egui::Panel::bottom("status")`), and font metrics need **`Context::fonts_mut`**, because
  `glyph_width` and `row_height` take `&mut self` while `Context::fonts` hands out a shared
  reference.
- **Painted text is invisible to AccessKit** — see stage 3. Costs the screen reader and the whole
  test harness if not paid back with a `WidgetInfo`.
- **eframe's default backend is wgpu**, not glow; `glow` is opt-in and the defaults must be turned
  off to avoid the wgpu stack. Turning them off also drops `accesskit`, which the shell wants — put
  it back explicitly.
- **`default_fonts` is not optional in practice.** Without it egui has no font, text has no size, and
  the layout assertions in the harness measure nothing.
- **Three crates in lockstep.** `eframe`, `egui` and `egui_kittest` must share a minor version;
  `eframe::egui` polices two of them, and `egui_kittest` is the one to check by hand.
- **`Event::Text` under a `command` modifier** must be dropped, whether or not the platform sends it.
- **`repeat`/`pressed`**: act on the press edge only.
- **`request_repaint` is the whole observer**, and asking for repaints on a timer instead turns the
  shell into a poller.

## Checklist (from `doc/shared-core-native-shell.md` §7)

| Requirement | How it is met |
|---|---|
| Holds no application state | only the quit-warning flag, as in the TUI |
| Renders from the windowed read API; the widget cannot edit itself | painted from `get_viewport`; `TextEdit` banned |
| Key mapping in a widget-free, unit-tested module | `keymap.rs` over `egui::Event` |
| Repaints from the observer, never a poll | `Notifier(egui::Context)` → `request_repaint()` |
| `follow_cursor` from input handling only | ordered explicitly inside the frame |
| Platform conventions from that platform's guidelines | **deliberately not met** — this shell has no platform, and the decision record says so |
| New capability landed in core + CLI in the same change | none needed |
| A UI-free test of its boundary, before the app build | `tests/behaviour.rs`, and it tests more than a boundary |
| Its own workflow, its own runners, no cross-compilation | `egui.yml`, three runners |
| Known gaps written down | the non-goals above |
