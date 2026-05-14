# AGENTS.md

Cross-tool guidance for AI coding agents working in this repository.

## Quick start (new agents)

**Do these in order. Do NOT skip to reading source code.**

1. Read this file fully — project context, repo layout, conventions.
2. **Go to Vikunja first.** Use the Vikunja MCP tools (`mcp__vikunja__*`) to
   read the `rustyTree` project (`vikunja_projects.get`, project id `66`),
   list all tasks (`vikunja_tasks.list`), and understand current status.
   This is the authoritative source for what's been done, what's in
   progress, and what's planned. Understand the full project state from
   Vikunja before touching the codebase.
3. Check `.cursor/rules/` if scoped rule files exist.
4. Check `docs/` for technical specs (architecture, threading model,
   platform shims).
5. **Only then** read source code as needed for your specific task.

## What is rustyTree?

rustyTree is a cross-platform **disk-usage analyzer** written in Rust,
modeled after [JAM Software TreeSize](https://www.jam-software.com/treesize).
You point it at a folder, it scans recursively, and shows you a sortable tree
of folders and files ordered by size with extra columns (file count,
last-modified, allocated vs logical bytes, owner) plus search/filter.

It ships as **two front-ends** sharing one scan engine:

- `rustytree-gui` — desktop window via
  [`eframe`](https://crates.io/crates/eframe) /
  [`egui`](https://crates.io/crates/egui). Pure Rust, no system
  GTK/Qt dependency.
- `rustytree-cli` — interactive terminal UI via
  [`ratatui`](https://crates.io/crates/ratatui) +
  [`crossterm`](https://crates.io/crates/crossterm). Pure Rust, no
  system curses dependency.

Both are thin clients on top of `rustytree-core`, the headless library
that owns scanning, aggregation, sorting, and search-filter logic.

- **Primary platform:** Linux. **Compiles on:** Linux, macOS, Windows.
- **Filesystem traversal:** [`jwalk`](https://crates.io/crates/jwalk)
  (parallel) on a worker thread, results streamed to the front-end
  through a channel.
- **Status:** GUI is MVP-complete. CLI is under active development; see
  Vikunja project 66 for the open task list.

### MVP scope

In: directory scan, sortable size-tree with expand/collapse, percent bars,
cancellable scans, extra columns (count, mtime, allocated, owner),
search/filter.

Out (deferred, not part of MVP): treemap visualization, sunburst chart,
snapshot/compare, export (CSV/JSON), scheduled rescans.

## Systems of record

| What | Where |
|------|-------|
| Vision, roadmap, project status | Vikunja project "rustyTree" (id 66) |
| Research findings and analysis | Vikunja task descriptions and comments |
| Task status and ownership | Vikunja tasks |
| Architecture decisions | `docs/` directory |
| Coding rules and conventions | This file + `.cursor/rules/` |
| User-facing build/run instructions | `README.md` |

Technical documentation that doesn't fit in Vikunja (specs, diagrams,
benchmarks, design docs) goes in `docs/`. Use Vikunja task descriptions and
comments for research findings, status reports, and narrative context. Use
`docs/` for anything an agent or developer needs while reading or writing
code.

## Repo layout

This is a Cargo **workspace** with one library crate and one binary crate
per front-end. The split is the contract: anything that touches the
filesystem, aggregates a tree, sorts, or filters lives in
`rustytree-core`. The front-ends only do display + input handling.

```text
rustyTree/
├── AGENTS.md                          # This file
├── README.md                          # User-facing pitch + build/run
├── Cargo.toml                         # Virtual workspace
├── Cargo.lock                         # Workspace-wide lockfile
├── docs/                              # Technical specs (architecture,
│                                      # threading, platform shims)
├── crates/
│   ├── rustytree-core/                # Headless lib. NO GUI/TUI deps.
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs                 # `pub mod scan; pub mod format;`
│   │   │   ├── format.rs              # bytes / mtime / percent / elapsed
│   │   │   └── scan/
│   │   │       ├── mod.rs
│   │   │       ├── walker.rs          # jwalk-based parallel walker
│   │   │       ├── tree.rs            # In-memory size tree (Vec arena)
│   │   │       ├── events.rs          # ScanEvent / ScanHandle / cancel
│   │   │       └── platform.rs        # cfg(unix)/cfg(windows) shims
│   │   └── tests/
│   │       └── scan_integration.rs    # tempfile-based end-to-end tests
│   ├── rustytree-gui/                 # eframe/egui binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs                # NativeOptions, run_native
│   │       ├── app.rs                 # RustyTreeApp, scan wiring
│   │       └── ui/
│   │           ├── mod.rs
│   │           ├── tree_view.rs       # virtualized hierarchical table
│   │           ├── toolbar.rs         # path + Browse + Scan/Cancel + search
│   │           └── status.rs          # bottom bar
│   └── rustytree-cli/                 # ratatui/crossterm binary
│       ├── Cargo.toml
│       ├── README.md                  # CLI keybindings + manual checklist
│       └── src/{main,app,ui}.rs
├── .git-hooks/
│   └── pre-commit                     # opt-in: exec scripts/cursor-review.sh
├── scripts/
│   └── cursor-review.sh               # Cursor `agent` CLI staged-diff review
└── .github/workflows/ci.yml           # workspace-wide fmt/clippy/test/build
```

Anything not yet in this tree is a future task — don't be surprised by gaps.

### What goes where

| Concern | Crate |
|--------|-------|
| Filesystem scan, jwalk wrapper, cancellation | `rustytree-core::scan::walker`/`events` |
| Tree storage, aggregation, sort/percent helpers | `rustytree-core::scan::tree` |
| Cross-platform metadata (allocated bytes, owner, mtime) | `rustytree-core::scan::platform` |
| Byte / date / duration formatting | `rustytree-core::format` |
| Hierarchy flatten / search-match closure / chevron picker | `rustytree-core` (extracted before the CLI lands) |
| eframe rendering, egui widgets, file picker | `rustytree-gui` |
| ratatui rendering, crossterm event loop | `rustytree-cli` |

If you find yourself wanting to copy a function from one front-end into
the other, lift it to `rustytree-core` instead. Two-front-end consistency
is the whole point of the split.

## Prerequisites

| Dependency | Required by | Install |
|-----------|-------------|---------|
| Rust toolchain pinned in `rust-toolchain.toml` | Building rustyTree | https://rustup.rs |
| OpenGL / system windowing libs | Running the GUI | see below |

### Linux (Debian/Ubuntu) system packages for `eframe`/`winit`/`glow`:

```sh
sudo apt install build-essential libxkbcommon-dev libgl1-mesa-dev \
    libwayland-dev libxkbcommon-x11-0
```

(Most desktop installs already have these.)

### macOS / Windows

Stock toolchain installs are sufficient. No extra system libs.

## Coding conventions

- **Language:** Rust, edition `2024` (set in `Cargo.toml`).
- **Formatting:** `cargo fmt` (rustfmt defaults). CI enforces `cargo fmt
  --check`.
- **Lints:** `cargo clippy --all-targets -- -D warnings` must pass. Don't
  silence lints without a comment explaining why.
- **Errors:**
  - Library code (`src/scan/*`, `src/lib.rs`): use `thiserror` to define
    typed errors. No `unwrap()`/`expect()` in production paths.
  - Application/UI code (`src/main.rs`, `src/app.rs`): bubbling up
    `eframe::Result` is fine; anything else can use `anyhow` if it
    simplifies code, but keep typed errors at module boundaries.
- **Naming:** snake_case modules and functions, PascalCase types, SCREAMING
  for consts. Public items get rustdoc; one-line summary minimum.
- **Cross-platform code:** isolate OS-specific code behind `cfg(unix)` /
  `cfg(windows)` in `src/scan/platform.rs` (and similar shims). The rest of
  the codebase imports a uniform API from there. No `cfg`-checks scattered
  through UI code.
- **Threading:** scanning runs on a worker thread; UI polls a channel each
  frame. Never block the UI thread on filesystem I/O. The cancellation
  contract (an `Arc<AtomicBool>`) is documented in `docs/architecture.md`
  once that task lands.
- **Comments:** explain *why*, not *what*. Don't narrate trivial code.
- **Dependencies:** prefer pure-Rust crates that work on Linux/macOS/Windows.
  Adding a non-trivial dep is worth a Vikunja comment justifying it.

### Naming for files / modules / types

- Crate / binary name: `rustytree` (lowercase, matches `Cargo.toml`).
- Product / window title: `rustyTree` (camelCase).
- Public type prefix `Scan*` for scan-engine-facing types
  (`ScanEvent`, `ScanHandle`, `ScanError`).

## Testing

**Test-driven development is required.** When building new functionality, write
tests as part of the same change — not as a follow-up task. Specifically:

- **New features**: add tests covering success paths, error paths, and edge
  cases.
- **Bug fixes**: add a regression test that would have caught the bug before
  applying the fix.
- **Run the test suite** before considering a change complete. All tests must
  pass.

A PR or change that adds functionality without corresponding tests is
incomplete.

### Test layers

- **Unit tests** live in `#[cfg(test)] mod tests` blocks at the bottom of
  each source file. Use them for pure logic (tree aggregation math,
  formatting, sort comparators).
- **Integration tests** live under `crates/rustytree-core/tests/` and
  exercise the public API of `rustytree-core` as a library
  (`use rustytree_core::scan::*`). Use
  [`tempfile`](https://crates.io/crates/tempfile) to build fixture trees on
  the fly — never commit binary fixtures.
- **UI tests** are not required for MVP. Focus on testing the headless
  scan engine and pure helpers; treat egui rendering as visually verified.

### Running tests

Always run against the whole workspace, not a single crate:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

CI runs the same on `ubuntu-latest`, `macos-latest`, `windows-latest`.

To exercise just one front-end:

```sh
cargo run -p rustytree-gui
cargo run -p rustytree-cli
```

## Vikunja project management

Keep Vikunja accurate **as you work**, not after being asked.

- **Before starting work**: check if a relevant task exists (project id 66).
  If not, create one and mark it in progress (`vikunja_tasks.update` with a
  comment, or use a label to indicate in-progress state).
- **While working**: if scope changes or you discover sub-tasks, update the
  task description or create related tasks.
- **After completing work**: mark the task done (`vikunja_tasks.update` with
  `done: true`). If the work produced decisions, trade-offs, or research
  worth preserving, add a comment to the task.
- **If you create new files or components**: make sure the task description
  reflects what was actually built, not just what was planned.
- **Never backfill** a batch of tasks after the fact. Each piece of work
  should have a task created before or at the start of that work.

The user should be able to open Vikunja at any time and see an accurate
picture of what's done, what's in progress, and what's next.

### Vikunja MCP server quirks

The Vikunja MCP server
([democratize-technology/vikunja-mcp](https://github.com/democratize-technology/vikunja-mcp))
has a few sharp edges around partial updates. Treat
`vikunja_projects.update` and `vikunja_tasks.bulk-update` as **full replace,
not partial patch**.

- **`vikunja_projects.update` requires `title`.** Calling `update` with only
  `id` + `description` (or any other subset that omits `title`) returns
  `Invalid Data`. Always include the existing `title` even when you don't
  intend to change it.
- **`vikunja_projects.update` resets `parent_project_id` to `0` if you don't
  pass `parentProjectId`.** A child project will silently become a top-level
  project. Always pass `parentProjectId` when updating any field on a child
  project — fetch the current parent first if you don't already know it.
- **`vikunja_tasks.bulk-update` wipes other fields.** Calling
  `bulk-update` with `field: "done"`, `value: true` clears `description` and
  `priority` on every targeted task. Either:
  - Re-apply lost fields with per-task single `update` calls afterward
    (single `update` correctly preserves omitted fields, including `done`),
    or
  - Avoid `bulk-update` entirely and use parallel single `update` calls.

All three are the same root cause: the server sends a partial PATCH that
Vikunja treats as a full object replace, so omitted fields get cleared.
Upstream tracking:

- [#44](https://github.com/democratize-technology/vikunja-mcp/issues/44) —
  `vikunja_projects.update` requires `title`
- [#45](https://github.com/democratize-technology/vikunja-mcp/issues/45) —
  `vikunja_projects.update` resets `parent_project_id`
- [#46](https://github.com/democratize-technology/vikunja-mcp/issues/46) —
  `vikunja_tasks.bulk-update` wipes other fields
- [#37](https://github.com/democratize-technology/vikunja-mcp/issues/37) —
  same family on the **task-update** path (silent project moves; `labels`
  ignored on `create`)

## Cursor tools and workarounds

### Pre-commit AI review

The repo ships an opt-in Cursor-CLI-powered pre-commit hook in
`.git-hooks/pre-commit` (a one-line shim) plus `scripts/cursor-review.sh`
(the actual review logic). When enabled, every `git commit` runs the
staged diff through `agent --mode=ask` against the rules in this file
plus anything under `.cursor/rules/`, and prints findings grouped as
**Blockers / Warnings / Nits**.

Activation is per-clone (the hook config is intentionally not
auto-applied):

```sh
git config core.hooksPath .git-hooks
```

Default behaviour is **warn-only**: failing reviews print but don't
block the commit. Knobs:

- `CURSOR_REVIEW_BLOCK=1` — make a `FAIL` verdict actually fail the commit.
- `CURSOR_REVIEW_SKIP=1` — bypass for this commit (WIP / squash prep).
- `CURSOR_REVIEW_MAX_BYTES=N` — skip review for diffs over `N` bytes (default 200 000).
- `CURSOR_REVIEW_MODEL=<slug>` — override which model `agent` uses.

The hook **silently no-ops if the `agent` CLI isn't installed**, so a
collaborator who hasn't installed the Cursor CLI is never blocked from
committing. The script also bails gracefully when `python3` isn't
available (it just prints the raw model output without the deterministic
verdict gate).

If you're an agent making changes here: the review prompt is in
`scripts/cursor-review.sh` and is designed to emit one
`[BLOCKER]/[WARNING]/[NIT]` line per issue and *nothing else* — keep
that contract intact so the python3 grouping/verdict logic works.

### Recovering chat history after moving a workspace folder

Cursor binds each chat (composer) to a specific workspace folder path via an
embedded `workspaceIdentifier` stored inside
`~/.config/Cursor/User/globalStorage/state.vscdb`. If the workspace folder is
moved on disk (e.g. `~/work/foo` → `~/work/team/foo`), Cursor treats the new
path as a brand-new workspace and the previous chats vanish from the sidebar.
The chats are **not deleted** — they're just orphaned to the old path.

Use the `migrate-cursor-chat` helper to rewrite the binding so the chats
reappear under the new workspace:

https://github.com/beardfaceguy/agentic_tools_misc/tree/main/migrate-cursor-chat

Workflow when this happens:

1. Open the new workspace path in Cursor once so it gets registered under
   `~/.config/Cursor/User/workspaceStorage/`, then **fully quit Cursor**
   (the script refuses to run if Cursor still has the SQLite DB open).
2. Dry-run to see what would be migrated:

   ```bash
   python3 migrate-cursor-chat.py --dry-run \
       <old-workspace-path> <new-workspace-path>
   ```

3. Apply for real (drop `--dry-run`). The script:
   - Makes a timestamped backup of `state.vscdb`.
   - Updates `composer.composerHeaders` (the sidebar list) and
     `composerData:<chat-id>` (per-chat record) for every chat tied to the
     old path.
   - Copies the per-chat folders under
     `~/.cursor/projects/<old-encoded-path>/agent-transcripts/` to the new
     project dir so past chats remain citeable from new agent sessions.
4. Reopen Cursor at the new path. The chats appear in the sidebar.

The helper is Linux-focused (paths under `~/.config/Cursor/`); see the repo
README for macOS/Windows path adjustments and recovery instructions.

When an agent encounters a user reporting "my chat history disappeared after
I moved my project folder" or similar, point them at this script rather than
trying to reconstruct sessions by hand.

## Review checklist

When reviewing or producing a diff:

- **Cross-platform:** OS-specific code lives behind `cfg(unix)` /
  `cfg(windows)` in `src/scan/platform.rs` (or equivalent shim). UI and
  scan-engine code call a uniform API.
- **Threading:** no blocking filesystem I/O on the UI thread. Long-running
  work happens on a worker thread; UI polls a channel.
- **Cancellation:** any new long-running operation respects the
  `Arc<AtomicBool>` cancellation flag.
- **Error handling:** no `unwrap()` or `expect()` in production paths.
  Library boundaries return typed errors via `thiserror`.
- **Tests:** new functionality lands with tests in the same change. Run
  `cargo test` before considering the work done.
- **Lints/format:** `cargo fmt --check` and `cargo clippy --all-targets
  -- -D warnings` clean.
- **Dependencies:** new third-party crates flagged in a Vikunja comment with
  rationale (size, maintenance, license, alternatives considered).
- **Vikunja sync:** the relevant task is updated/closed; new sub-tasks
  created if scope grew.
