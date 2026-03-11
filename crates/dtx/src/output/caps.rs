//! Terminal capabilities detection (3 independent axes).

/// Terminal capabilities — three independent booleans.
///
/// These are NOT a hierarchy. Each is detected independently:
/// - `color`: ANSI color codes (green, red, yellow, cyan, dim)
/// - `cursor`: In-place line updates (\r, \x1b[K]), ephemeral lines
/// - `width`: Terminal width for leader dot count, table column sizing
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub color: bool,
    pub cursor: bool,
    pub width: u16,
}

impl Capabilities {
    /// Detect capabilities from the current environment.
    ///
    /// - `color`: is_tty && !NO_COLOR && TERM != "dumb"
    /// - `cursor`: is_tty && TERM != "dumb"
    /// - `width`: crossterm::terminal::size() or 80
    pub fn detect() -> Self {
        let is_tty = std::io::stdout().is_terminal();
        let term = std::env::var("TERM").unwrap_or_default();
        let is_dumb = term == "dumb";
        let no_color = std::env::var("NO_COLOR").is_ok();

        let color = is_tty && !no_color && !is_dumb;
        let cursor = is_tty && !is_dumb;
        let width = crossterm::terminal::size()
            .map(|(w, _)| w)
            .unwrap_or(80);

        Self {
            color,
            cursor,
            width,
        }
    }

    /// Whether we're in TTY mode (has cursor control).
    pub fn is_tty(&self) -> bool {
        self.cursor
    }
}

use std::io::IsTerminal;
