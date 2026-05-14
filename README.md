# rustyTree

A cross-platform disk-usage analyzer in Rust, modeled after
[JAM Software TreeSize](https://www.jam-software.com/treesize). Scan a
directory, sort folders by size, drill in, find what's eating your disk —
either in a desktop window (`rustytree-gui`) or right in your terminal
(`rustytree-cli`).

> Status: both front-ends are feature-complete for MVP scope —
> sortable, searchable, virtualized size tree with extra columns
> (file count, mtime, allocated vs logical size, owner) and
> cancellable scans. CI runs on Linux, macOS, and Windows; release
> binaries are auto-built for all three on tagged releases.

## Install

### Download a prebuilt binary

The simplest way to try `rustyTree` is to grab a release archive from
the [Releases page](https://github.com/beardfaceguy/rustyTree/releases).

| Platform | File |
|----------|------|
| Linux x86_64 | `rustytree-vX.Y.Z-linux-x86_64.tar.gz` |
| macOS (universal: Apple Silicon + Intel) | `rustytree-vX.Y.Z-macos-universal.tar.gz` |
| Windows x86_64 | `rustytree-vX.Y.Z-windows-x86_64.zip` |

Each archive contains both binaries (`rustytree-gui` and `rustytree-cli`)
plus the licence files. A `.sha256` sidecar lives next to every archive
on the release page so you can verify the download:

```sh
# Linux / macOS
shasum -a 256 -c rustytree-vX.Y.Z-linux-x86_64.tar.gz.sha256
```

After extraction, run the GUI or CLI directly — no install step:

```sh
./rustytree-gui          # desktop window
./rustytree-cli /some/path   # interactive terminal UI
```

On Linux you may need a few system libraries for the GUI; see the
[Linux build deps](#linux-build-deps) section. The CLI has no system
dependencies beyond a working terminal.

### Install with `cargo`

If you already have a Rust toolchain, you can install directly from
the repository:

```sh
cargo install --git https://github.com/beardfaceguy/rustyTree --bin rustytree-gui
cargo install --git https://github.com/beardfaceguy/rustyTree --bin rustytree-cli
```

This drops the binaries into `~/.cargo/bin/`, which should already be on
your `PATH` if you installed Rust via [rustup](https://rustup.rs).

### Build from source

```sh
git clone https://github.com/beardfaceguy/rustyTree
cd rustyTree
cargo build --release --workspace
./target/release/rustytree-gui
./target/release/rustytree-cli
```

`rust-toolchain.toml` pins the exact Rust version (currently `1.95.0`);
rustup auto-installs it on first build.

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

## Develop

### Run from source

```sh
cargo run -p rustytree-gui
cargo run -p rustytree-cli
```

### Test the whole workspace

```sh
cargo test --workspace
```

### Linux build deps

`eframe` (used only by `rustytree-gui`) needs the standard
OpenGL / Wayland / X libraries. On Debian/Ubuntu:

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

## Releasing

Releases are tag-driven. Pushing a tag matching `v*` triggers
`.github/workflows/release.yml`, which builds release binaries on
Linux, macOS (universal arm64+x86_64 via `lipo`), and Windows, then
uploads each archive plus a SHA256 sidecar to the matching GitHub
Release. To cut a new release:

```sh
git tag v0.1.1   # match Cargo.toml workspace.package.version
git push origin v0.1.1
```

The workflow takes ~5–10 minutes; once it's green, the assets appear
on https://github.com/beardfaceguy/rustyTree/releases.

## License

Dual-licensed under either of:

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT)

at your option. This is the [Rust ecosystem
default](https://rust-lang.github.io/api-guidelines/necessities.html#crate-and-its-dependencies-have-a-permissive-license-c-permissive)
and lets downstream users pick whichever fits their project.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
