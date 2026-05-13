# rustyTree

A cross-platform GUI disk-usage analyzer in Rust, modeled after
[JAM Software TreeSize](https://www.jam-software.com/treesize). Scan a
directory, sort folders by size, drill in, find what's eating your disk.

> Status: early scaffold. Window opens; scan engine and tree view are work in
> progress. See the Vikunja project "rustyTree" for current backlog and
> in-progress tasks.

## Goals

- **Primary platform:** Linux. **Compiles on:** Linux, macOS, Windows.
- Pure-Rust GUI ([`eframe`](https://crates.io/crates/eframe) /
  [`egui`](https://crates.io/crates/egui)) — no system GTK/Qt/WebView
  dependency.
- Parallel filesystem traversal (planned: [`jwalk`](https://crates.io/crates/jwalk)).
- Cancellable scans with live progress.
- Sortable, virtualized tree of folders/files with extra columns
  (file count, last-modified, allocated vs logical size, owner) and
  search/filter.

## MVP scope

In: directory scan, sortable size-tree with expand/collapse, percent bars,
cancellation, extra columns, search/filter.

Out (deferred): treemap, sunburst, snapshot/compare, export, scheduled
rescans.

## Build

Requires a stable Rust toolchain (1.80+ recommended).

```sh
cargo run
```

This opens an empty `rustyTree` window.

To produce a release binary:

```sh
cargo build --release
./target/release/rustytree
```

### Linux build deps

`eframe` uses `glow` (OpenGL) by default and `winit` for windowing. On a fresh
Debian/Ubuntu system you typically need:

```sh
sudo apt install build-essential libxkbcommon-dev libgl1-mesa-dev \
    libwayland-dev libxkbcommon-x11-0
```

(Most desktop installs already have these.)

## License

TBD.
