//! Line rendering: indicator + label + fill + result.

use super::caps::Capabilities;
use std::fmt::Write as _;
use std::io::Write;

/// Indicator state for a line.
#[derive(Debug, Clone, Copy)]
pub enum Indicator {
    Done,
    /// Partial success — primary action succeeded but a child had issues.
    Partial,
    InProgress,
    /// Spinning in-progress — cycles through ◐ ◓ ◑ ◒
    Spinning(u8),
    Pending,
    Failed,
}

/// Spinner frames: half-circle rotates.
const SPIN_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];

impl Indicator {
    fn glyph(self) -> &'static str {
        match self {
            Self::Done => "●",
            Self::Partial => "◒",
            Self::InProgress => "◐",
            Self::Spinning(frame) => SPIN_FRAMES[(frame as usize) % SPIN_FRAMES.len()],
            Self::Pending => "◌",
            Self::Failed => "✕",
        }
    }

    fn color_code(self) -> &'static str {
        match self {
            Self::Done => "\x1b[32m",
            Self::Partial => "\x1b[33m",
            Self::InProgress | Self::Spinning(_) => "\x1b[33m",
            Self::Pending => "\x1b[2m",
            Self::Failed => "\x1b[31m",
        }
    }

    /// Fill character: `─` for resolved states, `·` for in-progress/pending.
    fn fill_char(self) -> &'static str {
        match self {
            Self::Done | Self::Failed | Self::Partial => "─",
            _ => "·",
        }
    }

    /// How much of the available gap to fill.
    /// Resolved states fill everything. Spinning grows. Pending shows minimal.
    fn fill_count(self, available: usize) -> usize {
        match self {
            Self::Done | Self::Failed | Self::Partial => available,
            Self::Spinning(frame) => {
                // Grow fast: ~3 chars per tick (80ms), fills in ~2.5s on wide terminal
                ((frame as usize + 1) * 3).min(available)
            }
            Self::InProgress => 3.min(available),
            Self::Pending => 3.min(available),
        }
    }
}

/// Parameters for rendering a complete indicator line.
pub struct RenderLineParams<'a> {
    pub caps: &'a Capabilities,
    pub indicator: Indicator,
    pub label: &'a str,
    pub result: &'a str,
    pub timing: Option<&'a str>,
    pub indent: usize,
    pub full_width: bool,
}

/// Render a complete indicator line to a writer.
///
/// full_width=true:  ` ● label ─────── result (timing)` (aligned, for pipeline/group)
/// full_width=false: ` ● label ── result (timing)` (compact, for standalone steps)
/// Non-TTY:          `dtx: label: result (timing)`
pub fn render_line(w: &mut dyn Write, params: &RenderLineParams<'_>) {
    let RenderLineParams {
        caps,
        indicator,
        label,
        result,
        timing,
        indent,
        full_width,
    } = params;
    let mut full_result = result.to_string();
    if let Some(t) = timing {
        if caps.color {
            write!(full_result, " \x1b[2m({})\x1b[0m", t).ok();
        } else {
            write!(full_result, " ({})", t).ok();
        }
    }

    if caps.is_tty() {
        if *full_width {
            render_tty(w, caps, *indicator, label, &full_result, *indent);
        } else {
            render_tty_compact(w, caps, *indicator, label, &full_result, *indent);
        }
    } else {
        render_pipe(w, label, &full_result, *indent);
    }
}

/// Render a TTY line with indicator glyph and fill between label and result.
///
/// When `newline` is true, appends `\n`; when false, no trailing newline
/// (used by pipeline redraws).
fn render_tty_full(
    w: &mut dyn Write,
    caps: &Capabilities,
    indicator: Indicator,
    label: &str,
    result: &str,
    indent: usize,
    newline: bool,
) {
    let indent_str = " ".repeat(indent * 2);
    let glyph = indicator.glyph();
    let width = caps.width as usize;
    let result_visible_len = strip_ansi_len(result);
    let prefix_len = 1 + glyph_width(glyph) + 1 + label.len() + 1;
    let available = width.saturating_sub(indent * 2 + prefix_len + result_visible_len + 1);

    let fill_ch = indicator.fill_char();
    let fill_n = indicator.fill_count(available);
    let nl = if newline { "\n" } else { "" };

    if caps.color {
        let color = indicator.color_code();
        let reset = "\x1b[0m";

        if caps.width < 40 || available < 3 {
            let _ = write!(
                w,
                "{} {}{}{} {} {}{}",
                indent_str, color, glyph, reset, label, result, nl
            );
        } else {
            let fill = fill_ch.repeat(fill_n);
            let pad = " ".repeat(available.saturating_sub(fill_n));
            let _ = write!(
                w,
                "{} {}{}{} {} \x1b[2m{}\x1b[0m{} {}{}",
                indent_str, color, glyph, reset, label, fill, pad, result, nl
            );
        }
    } else if caps.width < 40 || available < 3 {
        let _ = write!(w, "{} {} {} {}{}", indent_str, glyph, label, result, nl);
    } else {
        let fill = fill_ch.repeat(fill_n);
        let pad = " ".repeat(available.saturating_sub(fill_n));
        let _ = write!(
            w,
            "{} {} {} {}{} {}{}",
            indent_str, glyph, label, fill, pad, result, nl
        );
    }
}

/// Render a TTY line with trailing newline.
fn render_tty(
    w: &mut dyn Write,
    caps: &Capabilities,
    indicator: Indicator,
    label: &str,
    result: &str,
    indent: usize,
) {
    render_tty_full(w, caps, indicator, label, result, indent, true);
}

/// Render a compact TTY line with short `──` connector (for standalone steps).
fn render_tty_compact(
    w: &mut dyn Write,
    caps: &Capabilities,
    indicator: Indicator,
    label: &str,
    result: &str,
    indent: usize,
) {
    let indent_str = " ".repeat(indent * 2);
    let glyph = indicator.glyph();

    if caps.color {
        let color = indicator.color_code();
        let reset = "\x1b[0m";
        let _ = writeln!(
            w,
            "{} {}{}{} {} \x1b[2m──\x1b[0m {}",
            indent_str, color, glyph, reset, label, result
        );
    } else {
        let _ = writeln!(w, "{} {} {} ── {}", indent_str, glyph, label, result);
    }
}

/// Render a non-TTY (pipe) line.
fn render_pipe(w: &mut dyn Write, label: &str, result: &str, indent: usize) {
    let indent_str = " ".repeat(indent * 2);
    let _ = writeln!(w, "dtx:{} {}: {}", indent_str, label, result);
}

/// Render an ephemeral line (in-place update, cursor-capable terminals only).
/// Uses \r and \x1b[K to overwrite the current line.
pub fn render_ephemeral(
    w: &mut dyn Write,
    caps: &Capabilities,
    indicator: Indicator,
    label: &str,
    status: &str,
    indent: usize,
) {
    if !caps.cursor {
        return;
    }

    let indent_str = " ".repeat(indent * 2);
    let glyph = indicator.glyph();
    let width = caps.width as usize;
    let status_visible_len = strip_ansi_len(status);
    let prefix_len = 1 + glyph_width(glyph) + 1 + label.len() + 1;
    let available = width.saturating_sub(indent * 2 + prefix_len + status_visible_len + 1);

    let fill_ch = indicator.fill_char();
    let fill_n = indicator.fill_count(available);

    if caps.color {
        let color = indicator.color_code();
        let reset = "\x1b[0m";
        if available >= 3 {
            let fill = fill_ch.repeat(fill_n);
            let pad = " ".repeat(available.saturating_sub(fill_n));
            let _ = write!(
                w,
                "\r\x1b[K{} {}{}{} {} \x1b[2m{}\x1b[0m{} {}",
                indent_str, color, glyph, reset, label, fill, pad, status
            );
        } else {
            let _ = write!(
                w,
                "\r\x1b[K{} {}{}{} {} {}",
                indent_str, color, glyph, reset, label, status
            );
        }
    } else if available >= 3 {
        let fill = fill_ch.repeat(fill_n);
        let pad = " ".repeat(available.saturating_sub(fill_n));
        let _ = write!(
            w,
            "\r\x1b[K{} {} {} {}{} {}",
            indent_str, glyph, label, fill, pad, status
        );
    } else {
        let _ = write!(w, "\r\x1b[K{} {} {} {}", indent_str, glyph, label, status);
    }
    let _ = w.flush();
}

/// Clear the current ephemeral line.
pub fn clear_ephemeral(w: &mut dyn Write, caps: &Capabilities) {
    if caps.cursor {
        let _ = write!(w, "\r\x1b[K");
        let _ = w.flush();
    }
}

/// Render a TTY line without trailing newline (for pipeline redraws).
pub fn render_tty_no_newline(
    w: &mut dyn Write,
    caps: &Capabilities,
    indicator: Indicator,
    label: &str,
    result: &str,
    indent: usize,
) {
    render_tty_full(w, caps, indicator, label, result, indent, false);
}

/// Render a separator line.
pub fn render_separator(w: &mut dyn Write, caps: &Capabilities, text: &str) {
    if caps.is_tty() {
        let width = caps.width as usize;
        let prefix = format!(" ─── {} ", text);
        let remaining = width.saturating_sub(prefix.len());
        let suffix = "─".repeat(remaining);
        if caps.color {
            let _ = writeln!(w, "\x1b[2m{}{}\x1b[0m", prefix, suffix);
        } else {
            let _ = writeln!(w, "{}{}", prefix, suffix);
        }
    } else {
        let _ = writeln!(w, "dtx: --- {} ---", text);
    }
}

/// Calculate the display width of a Unicode glyph.
fn glyph_width(s: &str) -> usize {
    s.chars().count()
}

/// Calculate visible length of a string, stripping ANSI escape codes.
fn strip_ansi_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_len() {
        assert_eq!(strip_ansi_len("hello"), 5);
        assert_eq!(strip_ansi_len("\x1b[32mhello\x1b[0m"), 5);
        assert_eq!(strip_ansi_len("\x1b[2m(1.2s)\x1b[0m"), 6);
    }

    #[test]
    fn test_render_pipe_format() {
        let mut buf = Vec::new();
        render_pipe(&mut buf, "nix", "42 vars", 0);
        assert_eq!(String::from_utf8(buf).unwrap(), "dtx: nix: 42 vars\n");
    }

    #[test]
    fn test_render_pipe_indented() {
        let mut buf = Vec::new();
        render_pipe(&mut buf, "postgres", ":5432 pid 4821", 1);
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "dtx:   postgres: :5432 pid 4821\n"
        );
    }

    #[test]
    fn test_render_tty_done_uses_line() {
        let caps = Capabilities {
            color: false,
            cursor: true,
            width: 60,
        };
        let mut buf = Vec::new();
        render_tty(&mut buf, &caps, Indicator::Done, "nix", "42 vars", 0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("●"));
        assert!(output.contains("─")); // solid line, not dots
        assert!(!output.contains("·"));
    }

    #[test]
    fn test_render_tty_pending_uses_dots() {
        let caps = Capabilities {
            color: false,
            cursor: true,
            width: 60,
        };
        let mut buf = Vec::new();
        render_tty(&mut buf, &caps, Indicator::Pending, "nix", "waiting", 0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("◌"));
        assert!(output.contains("·")); // dots, not line
        assert!(!output.contains("─"));
        // Only 3 dots for pending
        assert_eq!(output.matches('·').count(), 3);
    }

    #[test]
    fn test_render_tty_narrow() {
        let caps = Capabilities {
            color: false,
            cursor: true,
            width: 30,
        };
        let mut buf = Vec::new();
        render_tty(&mut buf, &caps, Indicator::Done, "nix", "42 vars", 0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("●"));
        assert!(output.contains("nix"));
        assert!(output.contains("42 vars"));
        assert!(!output.contains("─")); // No fill in narrow mode
    }

    #[test]
    fn test_spinning_dots_grow() {
        let caps = Capabilities {
            color: false,
            cursor: true,
            width: 60,
        };
        let mut buf1 = Vec::new();
        render_tty(
            &mut buf1,
            &caps,
            Indicator::Spinning(0),
            "nix",
            "loading",
            0,
        );
        let out1 = String::from_utf8(buf1).unwrap();

        let mut buf2 = Vec::new();
        render_tty(
            &mut buf2,
            &caps,
            Indicator::Spinning(10),
            "nix",
            "loading",
            0,
        );
        let out2 = String::from_utf8(buf2).unwrap();

        // More dots at frame 10 than frame 0
        assert!(out2.matches('·').count() > out1.matches('·').count());
    }
}
