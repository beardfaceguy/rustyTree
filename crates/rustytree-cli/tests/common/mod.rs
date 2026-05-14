//! Shared helpers for the CLI integration tests.
//!
//! The CLI is a TUI built on ratatui + crossterm, but ratatui ships a
//! `TestBackend` that renders into an in-memory `Buffer` we can inspect.
//! Combined with the fact that `RustyTreeApp` exposes
//! `dispatch(Command)` / `on_key(KeyEvent)` / `poll_scan` as public,
//! we can drive the entire CLI end-to-end without a real terminal:
//!
//!   1. Build a fixture directory tree on disk.
//!   2. Construct a `RustyTreeApp` pointing at it.
//!   3. `dispatch(Command::StartScan)` and pump `poll_scan` until
//!      `Status::Done`.
//!   4. Send keys via `dispatch` (clearer than synthesising `KeyEvent`s
//!      for each action) or `on_key` (when we specifically want to
//!      cover the keymap).
//!   5. Render once into a `TestBackend` and inspect the buffer.

#![allow(dead_code)] // Each integration test binary uses a different subset of these helpers.

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rustytree_cli::app::{Action, Command, RustyTreeApp};
use rustytree_cli::ui;
use rustytree_core::view::Status;
use tempfile::TempDir;

/// Wall-clock cap on `run_to_done`. Fixtures used by these tests are
/// tiny (a handful of files); a real filesystem walk completes in a
/// few milliseconds. Anything north of `RUN_TIMEOUT` is a bug —
/// either the worker disconnected without sending `Done`, or the
/// fixture grew unexpectedly. Failing loud here is much better than
/// hanging CI.
const RUN_TIMEOUT: Duration = Duration::from_secs(5);

/// Build a known-shape directory tree we can assert against:
///
/// ```text
/// <tmp>/
/// ├── alpha.txt    (100 bytes — a's)
/// ├── beta/
/// │   ├── b1.txt   (200 bytes — b's)
/// │   └── b2.txt   (300 bytes — c's)
/// └── gamma/
///     └── g1.txt   (400 bytes — d's)
/// ```
///
/// The size totals work out to 100 + 200 + 300 + 400 = 1000 logical
/// bytes; tests can use that as a load-bearing constant.
pub fn make_fixture_tree() -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir for fixture");
    let root = dir.path();

    write_file(&root.join("alpha.txt"), &b"a".repeat(100));

    let beta = root.join("beta");
    std::fs::create_dir(&beta).expect("mkdir beta");
    write_file(&beta.join("b1.txt"), &b"b".repeat(200));
    write_file(&beta.join("b2.txt"), &b"c".repeat(300));

    let gamma = root.join("gamma");
    std::fs::create_dir(&gamma).expect("mkdir gamma");
    write_file(&gamma.join("g1.txt"), &b"d".repeat(400));

    dir
}

fn write_file(path: &Path, contents: &[u8]) {
    std::fs::write(path, contents).unwrap_or_else(|e| panic!("write fixture file {path:?}: {e}"));
}

/// Run the scan to completion (Status::Done / Cancelled / Error),
/// pumping `poll_scan` in a tight loop with a small sleep between
/// iterations so we don't burn 100% CPU on the channel try-recv.
///
/// Panics if the scan doesn't terminate within [`RUN_TIMEOUT`] —
/// that indicates the worker never sent a final event, which is a
/// real bug worth surfacing as a test failure.
pub fn run_to_done(app: &mut RustyTreeApp) {
    let start = Instant::now();
    loop {
        app.poll_scan();
        if matches!(
            app.status,
            Status::Done { .. } | Status::Cancelled | Status::Error(_)
        ) {
            return;
        }
        if start.elapsed() > RUN_TIMEOUT {
            panic!(
                "run_to_done: scan did not terminate within {:?}; status = {:?}",
                RUN_TIMEOUT, app.status
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Build a `RustyTreeApp` pointed at `path`, kick off a scan, and
/// pump events until it finishes. Returns the app ready for further
/// command dispatch.
pub fn scan_fixture(path: &Path) -> RustyTreeApp {
    let mut app = RustyTreeApp::new(path.to_path_buf());
    app.dispatch(Command::StartScan);
    run_to_done(&mut app);
    assert!(
        matches!(app.status, Status::Done { .. }),
        "expected Status::Done after fixture scan, got {:?}",
        app.status
    );
    app
}

/// Render the app once into a `TestBackend` of the given size and
/// return the buffer's contents as one `String` per row, with
/// trailing whitespace trimmed off each line so substring assertions
/// don't have to know about column padding.
pub fn render_lines(app: &mut RustyTreeApp, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("Terminal::new");
    terminal
        .draw(|f| ui::render(f, app))
        .expect("terminal.draw");
    let buf = terminal.backend().buffer();
    let mut lines = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut line = String::with_capacity(width as usize);
        for x in 0..width {
            line.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        // Trim trailing spaces; ratatui pads cells with " " to the
        // right edge and tests only care about the visible content.
        let trimmed: String = line.trim_end().to_string();
        lines.push(trimmed);
    }
    lines
}

/// Render and assert the rendered text contains `needle` somewhere.
/// Better failure messages than open-coding the same loop in every
/// test.
pub fn assert_render_contains(app: &mut RustyTreeApp, needle: &str) {
    let lines = render_lines(app, 120, 30);
    let joined = lines.join("\n");
    assert!(
        joined.contains(needle),
        "render did not contain {needle:?}; full render:\n{joined}"
    );
}

/// Convenience: dispatch a sequence of commands on the app. Lets a
/// test read like a script (`run_commands(&mut app, &[Move(1),
/// Move(1), Expand]);`).
pub fn run_commands(app: &mut RustyTreeApp, cmds: &[Command]) {
    for cmd in cmds {
        app.dispatch(cmd.clone());
    }
}

/// Build a `KeyEvent` with no modifiers — saves repeating
/// `KeyEvent::new(_, KeyModifiers::empty())` everywhere.
pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

/// Drive the app via the same path the real terminal loop uses
/// (`on_key`), and return the resulting `Action` so tests can assert
/// on Quit / Redraw / Ignore.
pub fn press(app: &mut RustyTreeApp, code: KeyCode) -> Action {
    app.on_key(key(code))
}
