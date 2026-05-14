//! Entry point + terminal lifecycle for `rustytree-cli`.
//!
//! Owns the alternate-screen / raw-mode dance and the main event loop;
//! all state and rendering live in [`app`] and [`ui`].

mod app;
mod ui;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

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

/// Main loop: poll for input, drain scan events, redraw. Runs until the
/// app reports [`Action::Quit`] or the terminal returns an error.
fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut RustyTreeApp) -> Result<()> {
    // Initial draw before we block on input so the user sees the welcome
    // screen immediately.
    terminal.draw(|f| ui::render(f, app))?;

    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    loop {
        // Pull whatever is on the scan channel before polling input so any
        // progress events show up on the next redraw.
        app.poll_scan();

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_default();

        if event::poll(timeout).context("event::poll")? {
            let ev = event::read().context("event::read")?;
            if let Event::Key(key) = ev
                && key.kind == event::KeyEventKind::Press
            {
                match app.on_key(key) {
                    Action::Quit => return Ok(()),
                    Action::None => {}
                }
            }
            // Mouse events ignored for MVP.
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        terminal.draw(|f| ui::render(f, app))?;
    }
}
