# rustyTree

A cross-platform disk-usage analyzer in Rust, modeled after
[JAM Software TreeSize](https://www.jam-software.com/treesize). Scan a
directory, sort folders by size, drill in, find what's eating your disk —
either in a desktop window (`rustytree-gui`) or right in your terminal
(`rustytree-cli`).

> Status: GUI is feature-complete for MVP scope (a)+(c) — sortable,
> searchable, virtualized size tree with extra columns. CLI is in active
> development. See the Vikunja project "rustyTree" for current backlog.

## Workspace layout

```text
rustyTree/
├── Cargo.toml                # virtual workspace
├── crates/
│   ├── rustytree-core/       # headless scan engine + format helpers (no GUI/TUI deps)
│   ├── rustytree-gui/        # eframe / egui desktop binary
│   └── rustytree-cli/        # ratatui + crossterm terminal binary
└── docs/architecture.md      # threading model, scan pipeline, extension points
```

The two front-ends share **all** scan and aggregation logic via
`rustytree-core`. The split between front-ends is purely how data is
displayed.

## Goals

- **Primary platform:** Linux. **Compiles on:** Linux, macOS, Windows.
- Pure-Rust GUI ([`eframe`](https://crates.io/crates/eframe) /
  [`egui`](https://crates.io/crates/egui)) — no system GTK/Qt/WebView
  dependency.
- Pure-Rust TUI ([`ratatui`](https://crates.io/crates/ratatui) /
  [`crossterm`](https://crates.io/crates/crossterm)) — no system curses
  dependency.
- Parallel filesystem traversal via
  [`jwalk`](https://crates.io/crates/jwalk).
- Cancellable scans with live progress updates.
- Sortable, virtualized tree of folders/files with extra columns
  (file count, last-modified, allocated vs logical size, owner) and
  search/filter.

## MVP scope

In: directory scan, sortable size-tree with expand/collapse, percent bars,
cancellation, extra columns, search/filter — across both front-ends.

Out (deferred): treemap, sunburst, snapshot/compare, export, scheduled
rescans.

## Build & run

Requires a stable Rust toolchain. The exact version is pinned in
`rust-toolchain.toml` (currently `1.95.0`); rustup will auto-install it
on first build.

### Desktop GUI

```sh
cargo run -p rustytree-gui
```

### Terminal CLI

```sh
cargo run -p rustytree-cli
```

### Release builds

```sh
cargo build --release --workspace
./target/release/rustytree-gui
./target/release/rustytree-cli
```

### Test the whole workspace

```sh
cargo test --workspace
```

### Linux build deps

`eframe` (used only by `rustytree-gui`) needs the standard
OpenGL / Wayland / X libraries:

```sh
sudo apt install build-essential libxkbcommon-dev libgl1-mesa-dev \
    libwayland-dev libxkbcommon-x11-0
```

The CLI has no such requirement — it only needs a working terminal.

## Pre-commit AI review (optional)

The repo ships a Cursor-CLI-powered pre-commit hook that reviews your
staged diff against the project rules in `AGENTS.md` (and any
`.cursor/rules/` files), groups findings into Blockers / Warnings /
Nits, and prints a verdict. It's **opt-in per clone** — nothing
happens until you point `core.hooksPath` at the tracked hook
directory:

```sh
git config core.hooksPath .git-hooks
```

The hook is **warn-only by default**: failing reviews print but don't
block the commit. Behaviour is controlled by environment variables:

| Variable | Effect |
|----------|--------|
| `CURSOR_REVIEW_BLOCK=1` | Make a `FAIL` verdict (any `[BLOCKER]`) actually fail the commit. |
| `CURSOR_REVIEW_SKIP=1`  | Bypass entirely for this commit (useful for WIP). |
| `CURSOR_REVIEW_MAX_BYTES=N` | Skip review for diffs larger than `N` bytes (default 200 000). |
| `CURSOR_REVIEW_MODEL=<slug>` | Override which model `agent` runs the review with. |

If the [Cursor `agent` CLI](https://cursor.com/install) isn't
installed, the hook prints brief install instructions and exits 0 — so
collaborators without the CLI are never blocked.

Source lives in `scripts/cursor-review.sh` and `.git-hooks/pre-commit`.

## License

TBD.
