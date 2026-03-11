//! Log-phase output: service logs and lifecycle events.

use super::caps::Capabilities;
use std::io::Write;

/// Render a service log line.
///
/// TTY:      `[service] log line content`
/// Non-TTY:  `dtx: [service] log line content`
pub fn render_log(
    w: &mut dyn Write,
    caps: &Capabilities,
    service: &str,
    line: &str,
    is_stderr: bool,
) {
    if caps.is_tty() {
        if caps.color {
            let content_color = if is_stderr { "\x1b[31m" } else { "" };
            let reset = if is_stderr { "\x1b[0m" } else { "" };
            let _ = writeln!(w, "\x1b[36m[{}]\x1b[0m {}{}{}", service, content_color, line, reset);
        } else {
            let _ = writeln!(w, "[{}] {}", service, line);
        }
    } else {
        let _ = writeln!(w, "dtx: [{}] {}", service, line);
    }
}

/// Render a lifecycle event in the log stream.
///
/// TTY:      `── message ──`
/// Non-TTY:  `dtx: -- message --`
pub fn render_lifecycle(w: &mut dyn Write, caps: &Capabilities, message: &str) {
    if caps.is_tty() {
        if caps.color {
            let _ = writeln!(w, "\x1b[2m── {} ──\x1b[0m", message);
        } else {
            let _ = writeln!(w, "── {} ──", message);
        }
    } else {
        let _ = writeln!(w, "dtx: -- {} --", message);
    }
}
