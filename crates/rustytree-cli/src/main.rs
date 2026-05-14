//! Entry point + terminal lifecycle for `rustytree-cli`.
//!
//! Owns the alternate-screen / raw-mode dance and the main event loop;
//! all state and rendering live in [`app`] and [`ui`].

mod app;
mod ui;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{Action, RustyTreeApp};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let mut terminal = setup_terminal()?;
    // Restore terminal even if the app panics so we don't leave the user
    // staring at a corrupted shell session.
    let _guard = TerminalGuard;

    let mut app = RustyTreeApp::new(path);
    let res = run(&mut terminal, &mut app);

    drop(terminal);
    drop(_guard);
    res
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("Terminal::new")
}

/// Restore terminal state on drop. Implemented as a separate type so it
/// runs even when the function returns via `?` or panics.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = disable_raw_mode();
        let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
    }
}

/// Main loop: poll for input, drain scan events, redraw only when
/// something changed. Runs until the app reports [`Action::Quit`] or
/// the terminal returns an error.
///
/// The loop tracks a `dirty` flag fed by three sources:
/// 1. Scan-event drain (`poll_scan` returns `true` when it advanced
///    state).
/// 2. Key dispatch (`on_key` returns `Action::Redraw` for recognised
///    keys; `Action::Ignore` for unbound ones).
/// 3. Initial mount (first frame is always drawn).
///
/// The previous implementation redrew on every loop iteration at the
/// poll timeout (~20fps). That's wasteful: on an idle UI it produced
/// hundreds of identical frames per minute, and on slow terminals it
/// caused noticeable flicker. Drawing only when state changes is
/// cheaper and visually quieter.
fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut RustyTreeApp) -> Result<()> {
    // Initial draw before we block on input so the user sees the welcome
    // screen immediately.
    terminal.draw(|f| ui::render(f, app))?;

    // Poll cap. Acts as a backstop so `event::poll` doesn't sit forever
    // when no keys are being pressed, which would also leave queued
    // scan events undrained. 50ms ≈ 20Hz, which is plenty for live
    // progress without burning idle CPU.
    let poll_timeout = Duration::from_millis(50);

    loop {
        let scan_dirty = app.poll_scan();

        let mut redraw = scan_dirty;

        if event::poll(poll_timeout).context("event::poll")? {
            let ev = event::read().context("event::read")?;
            if let Event::Key(key) = ev
                && key.kind == event::KeyEventKind::Press
            {
                match app.on_key(key) {
                    Action::Quit => return Ok(()),
                    Action::Redraw => redraw = true,
                    Action::Ignore => {}
                }
            }
            // Mouse events ignored for MVP.
        }

        if redraw {
            terminal.draw(|f| ui::render(f, app))?;
        }
    }
}
