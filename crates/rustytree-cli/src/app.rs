//! CLI app state, scan plumbing, and key-to-action dispatch.
//!
//! Mirrors `rustytree-gui::app::RustyTreeApp` in spirit, but without any
//! eframe types. Key handling is split into a pure `key_to_command`
//! function (easy to unit-test) plus a `dispatch_command` that mutates
//! state.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustytree_core::scan::{ScanEvent, ScanHandle, Tree, start_scan};
use rustytree_core::view::{
    SortDir, SortKey, Status, UiState, rebuild_visible_rows, toggle_expand,
};

/// The top-level CLI app. Holds everything between frames.
pub struct RustyTreeApp {
    pub path: PathBuf,
    pub scan: Option<ScanHandle>,
    pub tree: Option<Tree>,
    pub status: Status,
    pub ui: UiState,
    pub mode: Mode,
    pub help_open: bool,
    /// 0-based index of the top-most row currently rendered; updated by
    /// the renderer so the app can keep the selected row in view.
    pub scroll_offset: usize,
}

/// Modal input state. Most of the time we're in `Normal`. `/` switches
/// into `Search` until the user hits Enter or Esc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
}

/// Result of dispatching a key. The event loop uses this to decide
/// whether to keep running and whether the next frame needs to be
/// redrawn. `Ignore` is returned for keys that don't map to any
/// command (e.g. unbound letters in normal mode); the loop uses it
/// to skip the redraw, which is the whole point of the dirty-flag
/// scheme — most idle keystrokes shouldn't repaint the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Key was recognised; state may have changed; redraw.
    Redraw,
    /// Key wasn't bound; no state change; skip redraw.
    Ignore,
    /// Quit the loop.
    Quit,
}

/// One discrete command the user can issue. Extracting this from the
/// raw key event makes the dispatch logic straightforward to unit-test
/// without spinning up a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Quit the application.
    Quit,
    /// Begin scanning the configured path.
    StartScan,
    /// Cancel a running scan.
    CancelScan,
    /// Move the selection by the given delta (negative = up).
    Move(i32),
    /// Move selection to the first / last visible row.
    MoveFirst,
    MoveLast,
    /// Expand the selected row (or step into its first child).
    Expand,
    /// Collapse the selected row (or step out to its parent).
    Collapse,
    /// Set the sort key. If the same key is already active, flip direction.
    SetSort(SortKey),
    /// Toggle the help overlay.
    ToggleHelp,
    /// Enter search mode (modal input at the bottom).
    EnterSearch,
    /// Append a character to the search query (only meaningful in
    /// [`Mode::Search`]).
    SearchPush(char),
    /// Pop the last char from the search query.
    SearchBackspace,
    /// Apply the current search query and exit search mode.
    SearchApply,
    /// Clear the search query.
    SearchClear,
    /// Exit search mode without changing the query.
    SearchAbort,
}

impl RustyTreeApp {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            scan: None,
            tree: None,
            status: Status::Idle,
            ui: UiState::default(),
            mode: Mode::Normal,
            help_open: false,
            scroll_offset: 0,
        }
    }

    /// Drain pending scan events. Returns `true` if anything observable
    /// changed (progress tick, scan completion, cancellation, or
    /// disconnect), so the event loop only redraws when there's something
    /// new to show. A no-op call (no scan in flight, or no events
    /// queued) returns `false`.
    pub fn poll_scan(&mut self) -> bool {
        let mut dirty = false;
        loop {
            let recv = match self.scan.as_ref() {
                Some(h) => h.try_recv(),
                None => return dirty,
            };
            match recv {
                Ok(ScanEvent::Progress(p)) => {
                    self.ui.last_progress = Some(p);
                    dirty = true;
                }
                Ok(ScanEvent::Done { tree, elapsed }) => {
                    let root = tree.root();
                    let (total_bytes, file_count, dir_count) = root
                        .and_then(|r| tree.get(r))
                        .map(|n| (n.size_total, n.file_count, n.dir_count))
                        .unwrap_or((0, 0, 0));
                    self.status = Status::Done {
                        elapsed,
                        total_bytes,
                        file_count,
                        dir_count,
                    };
                    if let Some(r) = root {
                        self.ui.expanded.clear();
                        self.ui.expanded.insert(r);
                        self.ui.selected = Some(r);
                    }
                    self.tree = Some(tree);
                    self.ui.rows_dirty = true;
                    self.scroll_offset = 0;
                    self.scan = None;
                    dirty = true;
                }
                Ok(ScanEvent::Cancelled) => {
                    self.status = Status::Cancelled;
                    self.scan = None;
                    dirty = true;
                }
                Ok(ScanEvent::Error(e)) => {
                    self.status = Status::Error(format!("{e}"));
                    self.scan = None;
                    dirty = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if matches!(self.status, Status::Scanning) {
                        self.status = Status::Error(
                            "scan worker disconnected before reporting completion".into(),
                        );
                        dirty = true;
                    }
                    self.scan = None;
                    break;
                }
            }
        }

        if self.ui.rows_dirty
            && let Some(tree) = self.tree.as_ref()
        {
            rebuild_visible_rows(tree, &mut self.ui);
            self.ui.rows_dirty = false;
            self.clamp_selection();
            dirty = true;
        }
        dirty
    }

    /// Top-level key handler. Returns the resulting [`Action`] for the
    /// event loop. Unbound keys yield [`Action::Ignore`] so the main
    /// loop can skip its redraw — random keystrokes shouldn't cause
    /// the terminal to repaint.
    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        if let Some(cmd) = key_to_command(key, self.mode) {
            self.dispatch(cmd)
        } else {
            Action::Ignore
        }
    }

    /// Apply a command to state. Returns whether the loop should keep
    /// running.
    pub fn dispatch(&mut self, cmd: Command) -> Action {
        match cmd {
            Command::Quit => return Action::Quit,
            Command::StartScan => self.start_scan(),
            Command::CancelScan => {
                if let Some(h) = self.scan.as_ref() {
                    h.cancel();
                }
            }
            Command::Move(delta) => self.move_selection(delta),
            Command::MoveFirst => self.set_selection_to_index(0),
            Command::MoveLast => {
                let last = self.ui.visible_rows.len().saturating_sub(1);
                self.set_selection_to_index(last);
            }
            Command::Expand => self.expand_selected(),
            Command::Collapse => self.collapse_selected(),
            Command::SetSort(key) => {
                if self.ui.sort_key == key {
                    self.ui.sort_dir = match self.ui.sort_dir {
                        SortDir::Asc => SortDir::Desc,
                        SortDir::Desc => SortDir::Asc,
                    };
                } else {
                    self.ui.sort_key = key;
                    self.ui.sort_dir = match key {
                        SortKey::Name | SortKey::Owner => SortDir::Asc,
                        _ => SortDir::Desc,
                    };
                }
                self.ui.rows_dirty = true;
            }
            Command::ToggleHelp => self.help_open = !self.help_open,
            Command::EnterSearch => {
                self.mode = Mode::Search;
            }
            Command::SearchPush(c) => {
                self.ui.search.push(c);
                self.ui.rows_dirty = true;
            }
            Command::SearchBackspace => {
                if self.ui.search.pop().is_some() {
                    self.ui.rows_dirty = true;
                }
            }
            Command::SearchApply => {
                self.mode = Mode::Normal;
                self.ui.rows_dirty = true;
            }
            Command::SearchClear => {
                self.ui.search.clear();
                self.ui.rows_dirty = true;
            }
            Command::SearchAbort => {
                self.ui.search.clear();
                self.mode = Mode::Normal;
                self.ui.rows_dirty = true;
            }
        }
        // Re-flatten now if anything dirtied the rows so the next render
        // sees a consistent state.
        if self.ui.rows_dirty
            && let Some(tree) = self.tree.as_ref()
        {
            rebuild_visible_rows(tree, &mut self.ui);
            self.ui.rows_dirty = false;
            self.clamp_selection();
        }
        Action::Redraw
    }

    fn start_scan(&mut self) {
        if self.path.as_os_str().is_empty() {
            self.status =
                Status::Error("no path configured — pass a directory as the first argument".into());
            return;
        }
        self.tree = None;
        self.ui.reset_for_new_scan();
        self.scroll_offset = 0;
        match start_scan(self.path.clone()) {
            Ok(handle) => {
                self.status = Status::Scanning;
                self.scan = Some(handle);
            }
            Err(e) => {
                self.status = Status::Error(e.to_string());
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.ui.visible_rows.is_empty() {
            return;
        }
        let cur = self.selected_index().unwrap_or(0) as i32;
        let max = self.ui.visible_rows.len() as i32 - 1;
        let new = (cur + delta).clamp(0, max) as usize;
        self.set_selection_to_index(new);
    }

    fn set_selection_to_index(&mut self, idx: usize) {
        if let Some(row) = self.ui.visible_rows.get(idx) {
            self.ui.selected = Some(row.id);
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let id = self.ui.selected?;
        self.ui.visible_rows.iter().position(|r| r.id == id)
    }

    fn expand_selected(&mut self) {
        let Some(id) = self.ui.selected else { return };
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        let Some(node) = tree.get(id) else { return };
        if node.children.is_empty() {
            return;
        }
        if self.ui.expanded.contains(&id) {
            // Already expanded: step into the first child as a "drill in".
            if let Some(first_child) = node.children.first().copied() {
                self.ui.selected = Some(first_child);
                self.ui.expanded.insert(first_child);
            }
        } else {
            self.ui.expanded.insert(id);
        }
        self.ui.rows_dirty = true;
    }

    fn collapse_selected(&mut self) {
        let Some(id) = self.ui.selected else { return };
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        if self.ui.expanded.contains(&id) {
            toggle_expand(&mut self.ui.expanded, id);
        } else if let Some(node) = tree.get(id)
            && let Some(parent) = node.parent
        {
            self.ui.selected = Some(parent);
        }
        self.ui.rows_dirty = true;
    }

    /// If the selection somehow points at a node that's no longer visible
    /// (e.g. its parent was collapsed), snap to the nearest visible row.
    fn clamp_selection(&mut self) {
        if self.ui.visible_rows.is_empty() {
            self.ui.selected = None;
            return;
        }
        if let Some(id) = self.ui.selected
            && self.ui.visible_rows.iter().any(|r| r.id == id)
        {
            return;
        }
        // Default to the root.
        self.ui.selected = self.ui.visible_rows.first().map(|r| r.id);
    }
}

/// Pure mapping from key event + current mode to a command. Keeping this
/// pure makes it easy to unit-test all the keybindings without bringing
/// up a terminal.
pub fn key_to_command(key: KeyEvent, mode: Mode) -> Option<Command> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Ctrl+C is universal: always quit.
    if ctrl && matches!(key.code, KeyCode::Char('c')) {
        return Some(Command::Quit);
    }

    match mode {
        Mode::Search => match key.code {
            KeyCode::Esc => Some(Command::SearchAbort),
            KeyCode::Enter => Some(Command::SearchApply),
            KeyCode::Backspace => Some(Command::SearchBackspace),
            KeyCode::Char(c) => Some(Command::SearchPush(c)),
            _ => None,
        },
        Mode::Normal => match key.code {
            KeyCode::Char('q') => Some(Command::Quit),
            KeyCode::Char('s') | KeyCode::Char('r') => Some(Command::StartScan),
            KeyCode::Esc => Some(Command::CancelScan),

            KeyCode::Up | KeyCode::Char('k') => Some(Command::Move(-1)),
            KeyCode::Down | KeyCode::Char('j') => Some(Command::Move(1)),
            KeyCode::PageUp => Some(Command::Move(-15)),
            KeyCode::PageDown => Some(Command::Move(15)),
            KeyCode::Home | KeyCode::Char('g') => Some(Command::MoveFirst),
            KeyCode::End | KeyCode::Char('G') => Some(Command::MoveLast),

            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => Some(Command::Expand),
            KeyCode::Left | KeyCode::Char('h') => Some(Command::Collapse),

            KeyCode::Char('1') => Some(Command::SetSort(SortKey::Name)),
            KeyCode::Char('2') => Some(Command::SetSort(SortKey::Size)),
            KeyCode::Char('3') => Some(Command::SetSort(SortKey::Allocated)),
            KeyCode::Char('4') => Some(Command::SetSort(SortKey::FileCount)),
            KeyCode::Char('5') => Some(Command::SetSort(SortKey::DirCount)),
            KeyCode::Char('6') => Some(Command::SetSort(SortKey::Mtime)),
            KeyCode::Char('7') => Some(Command::SetSort(SortKey::Owner)),

            KeyCode::Char('/') => Some(Command::EnterSearch),
            KeyCode::Char('c') => Some(Command::SearchClear),

            KeyCode::Char('?') => Some(Command::ToggleHelp),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }
    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_c_quits_in_any_mode() {
        assert_eq!(
            key_to_command(ctrl(KeyCode::Char('c')), Mode::Normal),
            Some(Command::Quit)
        );
        assert_eq!(
            key_to_command(ctrl(KeyCode::Char('c')), Mode::Search),
            Some(Command::Quit)
        );
    }

    #[test]
    fn q_quits_in_normal_but_is_search_input_in_search() {
        assert_eq!(
            key_to_command(key(KeyCode::Char('q')), Mode::Normal),
            Some(Command::Quit)
        );
        assert_eq!(
            key_to_command(key(KeyCode::Char('q')), Mode::Search),
            Some(Command::SearchPush('q'))
        );
    }

    #[test]
    fn arrow_keys_move_selection_in_normal_mode() {
        assert_eq!(
            key_to_command(key(KeyCode::Up), Mode::Normal),
            Some(Command::Move(-1))
        );
        assert_eq!(
            key_to_command(key(KeyCode::Down), Mode::Normal),
            Some(Command::Move(1))
        );
        assert_eq!(
            key_to_command(key(KeyCode::PageDown), Mode::Normal),
            Some(Command::Move(15))
        );
    }

    #[test]
    fn vim_navigation_works_in_normal_mode() {
        assert_eq!(
            key_to_command(key(KeyCode::Char('j')), Mode::Normal),
            Some(Command::Move(1))
        );
        assert_eq!(
            key_to_command(key(KeyCode::Char('k')), Mode::Normal),
            Some(Command::Move(-1))
        );
        assert_eq!(
            key_to_command(key(KeyCode::Char('h')), Mode::Normal),
            Some(Command::Collapse)
        );
        assert_eq!(
            key_to_command(key(KeyCode::Char('l')), Mode::Normal),
            Some(Command::Expand)
        );
    }

    #[test]
    fn digit_keys_set_sort() {
        assert_eq!(
            key_to_command(key(KeyCode::Char('1')), Mode::Normal),
            Some(Command::SetSort(SortKey::Name))
        );
        assert_eq!(
            key_to_command(key(KeyCode::Char('2')), Mode::Normal),
            Some(Command::SetSort(SortKey::Size))
        );
        assert_eq!(
            key_to_command(key(KeyCode::Char('7')), Mode::Normal),
            Some(Command::SetSort(SortKey::Owner))
        );
    }

    #[test]
    fn slash_enters_search_and_esc_aborts() {
        assert_eq!(
            key_to_command(key(KeyCode::Char('/')), Mode::Normal),
            Some(Command::EnterSearch)
        );
        assert_eq!(
            key_to_command(key(KeyCode::Esc), Mode::Search),
            Some(Command::SearchAbort)
        );
        assert_eq!(
            key_to_command(key(KeyCode::Enter), Mode::Search),
            Some(Command::SearchApply)
        );
    }

    #[test]
    fn search_typing_pushes_chars() {
        assert_eq!(
            key_to_command(key(KeyCode::Char('a')), Mode::Search),
            Some(Command::SearchPush('a'))
        );
        assert_eq!(
            key_to_command(key(KeyCode::Backspace), Mode::Search),
            Some(Command::SearchBackspace)
        );
    }

    #[test]
    fn esc_in_normal_mode_cancels_scan() {
        assert_eq!(
            key_to_command(key(KeyCode::Esc), Mode::Normal),
            Some(Command::CancelScan)
        );
    }

    #[test]
    fn dispatch_set_sort_flips_direction_on_repeat() {
        let mut app = RustyTreeApp::new(PathBuf::from("/tmp"));
        let initial_dir = app.ui.sort_dir;
        app.dispatch(Command::SetSort(SortKey::Size));
        // First press on Size while default key is also Size flips Desc <-> Asc.
        assert_ne!(app.ui.sort_dir, initial_dir);
        let after_first = app.ui.sort_dir;
        app.dispatch(Command::SetSort(SortKey::Size));
        assert_ne!(app.ui.sort_dir, after_first);
    }

    #[test]
    fn dispatch_quit_returns_quit_action() {
        let mut app = RustyTreeApp::new(PathBuf::from("/tmp"));
        // Quit short-circuits before reaching Action::Redraw at the
        // bottom of dispatch, so the test stays correct under the new
        // Action enum.
        assert_eq!(app.dispatch(Command::Quit), Action::Quit);
    }

    #[test]
    fn unbound_key_returns_ignore_so_loop_skips_redraw() {
        // The whole point of the dirty-flag scheme is that random
        // keystrokes don't trigger a redraw. Pick a key that the
        // normal-mode keymap doesn't handle (PageDown maps to a
        // Move command — use F1 instead, which is unmapped) and
        // assert it lands as Ignore.
        let mut app = RustyTreeApp::new(PathBuf::from("/tmp"));
        let f1 = KeyEvent::new(KeyCode::F(1), KeyModifiers::empty());
        assert_eq!(app.on_key(f1), Action::Ignore);
    }

    #[test]
    fn bound_key_returns_redraw() {
        // A key that DOES map to a command should yield Redraw, so
        // the loop knows the next frame is worth painting. ToggleHelp
        // is a low-side-effect choice.
        let mut app = RustyTreeApp::new(PathBuf::from("/tmp"));
        let q_mark = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::empty());
        assert_eq!(app.on_key(q_mark), Action::Redraw);
    }

    #[test]
    fn poll_scan_returns_false_when_no_scan_in_flight() {
        // No scan started yet → nothing to drain → loop should be
        // free to skip the redraw.
        let mut app = RustyTreeApp::new(PathBuf::from("/tmp"));
        assert!(!app.poll_scan());
    }
}
