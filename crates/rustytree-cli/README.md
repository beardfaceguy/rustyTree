# rustytree-cli

Interactive terminal UI for rustyTree, built on
[`ratatui`](https://crates.io/crates/ratatui) and
[`crossterm`](https://crates.io/crates/crossterm). Same scan engine as
`rustytree-gui`, just rendered in your terminal so it works over SSH or
inside tmux without forwarding X.

## Run

```sh
cargo run -p rustytree-cli                  # scan $PWD
cargo run -p rustytree-cli -- /var/log      # scan a specific path
```

In release mode:

```sh
cargo build --release -p rustytree-cli
./target/release/rustytree-cli ~/projects
```

## Keys

```text
q               quit
Ctrl+C          force quit (works in any mode)

s, r            start / restart scan on the current path
Esc             cancel a running scan

Up,    j, k     move selection (k = up, j = down)
Down
PgUp / PgDn     page up / down
Home, g         jump to first row
End,  G         jump to last row

Right, l, Enter expand selection (or step into first child)
Left,  h        collapse selection (or step out to parent)

1               sort by Name
2               sort by Size
3               sort by Allocated
4               sort by Files
5               sort by Dirs
6               sort by Modified
7               sort by Owner
                pressing the same key again flips ascending/descending

/               enter search mode (case-insensitive substring filter)
                Enter applies, Esc aborts and clears
c               clear active search

?               toggle help overlay
```

## Status indicators

The header shows the active path and a status line that mirrors the GUI:

- `ready` — no scan has run yet
- `scanning... 12345 entries, 1.2 GiB so far (...)` — live progress
- `done in 3.4s | 1.2 GiB | 4321 files, 56 dirs`
- `cancelled` — Esc during a scan
- `error: ...` — typically a non-existent or unreadable directory

The `%` column is colour-coded: red for >50% of root, yellow for >10%,
default otherwise.

## Manual test checklist

A quick exercise after non-trivial changes to either the CLI or the
shared `rustytree-core` view module:

1. **Cold start.** `cargo run -p rustytree-cli` opens to the welcome
   screen with the current working directory shown in the header.
2. **Small scan.** Press `s`. Status flips through `scanning` to
   `done in ...`. Tree appears with the root expanded.
3. **Cancel a scan.** Point at a large directory, press `s`, then
   immediately `Esc`. Status reads `cancelled` within ~50 ms.
4. **Navigation.** `j`/`k` and Up/Down move the selection without
   scrolling the viewport unnecessarily; PgDn jumps a screen.
5. **Expand / collapse.** `l` on a directory opens it; `l` again drills
   into the first child. `h` collapses or steps out.
6. **Sorting.** Press `2`, `2` — order flips between Size desc and
   Size asc. The active column header turns yellow with `^`/`v`.
7. **Search.** `/`, type `target`, Enter. Only matching subtrees show;
   ancestors auto-expand. `c` clears the filter.
8. **Help overlay.** `?` shows the keybindings, `?` again hides them.
9. **Resize.** Resize the terminal mid-scan; the layout reflows on the
   next tick (~50 ms).
10. **Clean exit.** `q` returns you to a clean shell prompt with no
    leftover escape sequences.
