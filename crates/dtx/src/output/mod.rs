//! Unified CLI output system.
//!
//! Provides a transferential output model where output structure mirrors work structure.
//! See `docs/guides/cli-output-design.md` for the full design system.

mod caps;
mod line;
mod stream;
mod table;

pub use caps::Capabilities;
pub use table::{Cell, TableBuilder};

use line::Indicator;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Shared output handle. Clone-friendly (Arc-based), Send + Sync.
///
/// All CLI output flows through this. Detects terminal capabilities once
/// and renders appropriately for TTY vs pipe.
#[derive(Clone)]
pub struct Output {
    inner: Arc<OutputInner>,
}

struct OutputInner {
    writer: Mutex<Box<dyn Write + Send>>,
    err_writer: Mutex<Box<dyn Write + Send>>,
    caps: Capabilities,
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

impl Output {
    /// Create a new Output that writes to stdout/stderr with auto-detected capabilities.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OutputInner {
                writer: Mutex::new(Box::new(io::stdout())),
                err_writer: Mutex::new(Box::new(io::stderr())),
                caps: Capabilities::detect(),
            }),
        }
    }

    /// Create with explicit capabilities and writer (for testing).
    #[cfg(test)]
    pub fn with_test_writer(caps: Capabilities, writer: Box<dyn Write + Send>) -> Self {
        Self {
            inner: Arc::new(OutputInner {
                writer: Mutex::new(writer),
                err_writer: Mutex::new(Box::new(Vec::new())),
                caps,
            }),
        }
    }

    /// Get terminal capabilities.
    pub fn caps(&self) -> &Capabilities {
        &self.inner.caps
    }

    // === Steps ===

    /// Create a standalone step (compact: short `──` connector).
    pub fn step(&self, label: &str) -> Step {
        Step {
            output: self.clone(),
            label: label.to_string(),
            indent: 0,
            start: Instant::now(),
            animation: None,
            full_width: false,
        }
    }

    /// Create a child step (full-width fill, aligns with siblings).
    pub fn step_child(&self, label: &str) -> Step {
        Step {
            output: self.clone(),
            label: label.to_string(),
            indent: 1,
            start: Instant::now(),
            animation: None,
            full_width: true,
        }
    }

    // === Pipeline ===

    /// Create a pipeline that shows all steps upfront as pending placeholders.
    ///
    /// On TTY: prints all labels as `◌ label ··· pending`, then updates in-place.
    /// On non-TTY: returns a pipeline that prints each step sequentially when done.
    pub fn pipeline(&self, labels: &[&str]) -> Pipeline {
        let now = Instant::now();
        let total_lines = labels.len();
        let string_labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
        let starts: Vec<Instant> = labels.iter().map(|_| now).collect();

        if self.caps().cursor {
            // Print all steps as pending placeholders
            for label in labels {
                self.write_line(Indicator::Pending, label, "waiting", None, 0, true);
            }
        }
        // On non-TTY: don't print anything upfront — each step prints when done

        let states = vec![PipelineStepState::Pending; total_lines];

        Pipeline {
            output: self.clone(),
            labels: string_labels,
            starts,
            states,
            total_lines,
            first_update: true,
            animation: None,
        }
    }

    // === Groups ===

    /// Create a new group (parent with child steps).
    pub fn group(&self, label: &str) -> Group {
        Group {
            output: self.clone(),
            label: label.to_string(),
            children: Vec::new(),
            start: Instant::now(),
        }
    }

    // === Phase separator ===

    /// Print a separator line between phases (e.g., bootstrap → streaming).
    pub fn separator(&self, text: &str) {
        let mut w = self.inner.writer.lock().unwrap();
        let _ = writeln!(w);
        line::render_separator(&mut *w, &self.inner.caps, text);
        let _ = writeln!(w);
    }

    // === Tables ===

    /// Create a table builder.
    pub fn table(&self) -> TableBuilder {
        TableBuilder::new(self.inner.caps)
    }

    /// Render a table to this output's writer.
    pub fn print_table(&self, table: TableBuilder) {
        let mut w = self.inner.writer.lock().unwrap();
        table.render(&mut *w);
    }

    // === Log stream ===

    /// Print a service log line (for streaming phase).
    pub fn log(&self, service: &str, line: &str, is_stderr: bool) {
        let mut w = self.inner.writer.lock().unwrap();
        stream::render_log(&mut *w, &self.inner.caps, service, line, is_stderr);
    }

    /// Print a lifecycle event in the log stream.
    pub fn lifecycle(&self, message: &str) {
        let mut w = self.inner.writer.lock().unwrap();
        stream::render_lifecycle(&mut *w, &self.inner.caps, message);
    }

    // === Errors (to stderr) ===

    /// Print an error message to stderr.
    pub fn error(&self, message: &str) {
        let mut w = self.inner.err_writer.lock().unwrap();
        let caps = &self.inner.caps;
        if caps.color {
            let _ = writeln!(w, "\x1b[31merror:\x1b[0m {}", message);
        } else if caps.is_tty() {
            let _ = writeln!(w, "error: {}", message);
        } else {
            let _ = writeln!(w, "dtx: error: {}", message);
        }
    }

    /// Print an error with structured details and optional hint.
    pub fn error_detail(&self, msg: &str, details: &[(&str, &str)], hint: Option<&str>) {
        let mut w = self.inner.err_writer.lock().unwrap();
        let caps = &self.inner.caps;

        let _ = writeln!(w);
        if caps.color {
            let _ = writeln!(w, "   \x1b[31merror:\x1b[0m {}", msg);
        } else if caps.is_tty() {
            let _ = writeln!(w, "   error: {}", msg);
        } else {
            let _ = writeln!(w, "dtx:   error: {}", msg);
        }

        for (key, value) in details {
            if caps.is_tty() {
                let _ = writeln!(w, "     {}: {}", key, value);
            } else {
                let _ = writeln!(w, "dtx:     {}: {}", key, value);
            }
        }

        if let Some(h) = hint {
            if caps.color {
                let _ = writeln!(w, "     \x1b[33mhint:\x1b[0m {}", h);
            } else if caps.is_tty() {
                let _ = writeln!(w, "     hint: {}", h);
            } else {
                let _ = writeln!(w, "dtx:     hint: {}", h);
            }
        }
    }

    /// Print a warning message.
    pub fn warning(&self, message: &str) {
        let mut w = self.inner.err_writer.lock().unwrap();
        let caps = &self.inner.caps;
        if caps.color {
            let _ = writeln!(w, "\x1b[33mwarn:\x1b[0m {}", message);
        } else if caps.is_tty() {
            let _ = writeln!(w, "warn: {}", message);
        } else {
            let _ = writeln!(w, "dtx: warn: {}", message);
        }
    }

    // === Pass-through ===

    /// Print raw text (no formatting, no prefix).
    pub fn raw(&self, text: &str) {
        let mut w = self.inner.writer.lock().unwrap();
        let _ = write!(w, "{}", text);
    }

    /// Print a blank line.
    pub fn blank(&self) {
        let mut w = self.inner.writer.lock().unwrap();
        let _ = writeln!(w);
    }

    /// Write directly to the underlying writer.
    fn write_line(
        &self,
        indicator: Indicator,
        label: &str,
        result: &str,
        timing: Option<&str>,
        indent: usize,
        full_width: bool,
    ) {
        let mut w = self.inner.writer.lock().unwrap();
        line::render_line(
            &mut *w,
            &line::RenderLineParams {
                caps: &self.inner.caps,
                indicator,
                label,
                result,
                timing,
                indent,
                full_width,
            },
        );
    }

    fn write_ephemeral(&self, indicator: Indicator, label: &str, status: &str, indent: usize) {
        let mut w = self.inner.writer.lock().unwrap();
        line::render_ephemeral(&mut *w, &self.inner.caps, indicator, label, status, indent);
    }

    fn clear_ephemeral(&self) {
        let mut w = self.inner.writer.lock().unwrap();
        line::clear_ephemeral(&mut *w, &self.inner.caps);
    }
}

/// A single unit of work with timing.
///
/// Created via `Output::step()` or `Group::step()`.
/// Must be resolved by calling `done()` or `fail()`, which consume `self`.
pub struct Step {
    output: Output,
    label: String,
    indent: usize,
    start: Instant,
    animation: Option<AnimationHandle>,
    full_width: bool,
}

/// Handle for a background animation thread.
/// Dropped automatically when Step is consumed by done()/fail().
struct AnimationHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AnimationHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Step {
    /// Show pending state (ephemeral on TTY).
    pub fn pending(&mut self, status: &str) {
        self.output
            .write_ephemeral(Indicator::Pending, &self.label, status, self.indent);
    }

    /// Show in-progress state (overwrites ephemeral on TTY).
    pub fn progress(&mut self, status: &str) {
        self.output
            .write_ephemeral(Indicator::InProgress, &self.label, status, self.indent);
    }

    /// Start a live progress animation with elapsed timer.
    ///
    /// Updates the ephemeral line every 200ms with the current elapsed time:
    /// ```text
    ///  ◐ nix ··· loading (1.4s)
    /// ```
    /// The animation stops when `done()` or `fail()` is called.
    /// On non-TTY or no cursor support, prints a single static line instead.
    pub fn animate(&mut self, status: &str) {
        // Stop any existing animation
        self.animation = None;

        if !self.output.caps().cursor {
            // Non-TTY: just show a static line, no animation
            return;
        }

        // Show initial state immediately
        let elapsed = format_duration(self.start.elapsed());
        self.output.write_ephemeral(
            Indicator::Spinning(0),
            &self.label,
            &format!("{} ({})", status, elapsed),
            self.indent,
        );

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let output = self.output.clone();
        let label = self.label.clone();
        let indent = self.indent;
        let start = self.start;
        let status = status.to_string();

        let thread = std::thread::spawn(move || {
            let mut frame: u8 = 0;
            while !stop_clone.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(80));
                if stop_clone.load(Ordering::SeqCst) {
                    break;
                }
                frame = frame.wrapping_add(1);
                let elapsed = format_duration(start.elapsed());
                output.write_ephemeral(
                    Indicator::Spinning(frame),
                    &label,
                    &format!("{} ({})", status, elapsed),
                    indent,
                );
            }
        });

        self.animation = Some(AnimationHandle {
            stop,
            thread: Some(thread),
        });
    }

    /// Mark as done. Consumes self (stops animation).
    pub fn done(self, result: &str) {
        drop(self.animation);
        self.output.clear_ephemeral();
        let elapsed = self.start.elapsed();
        let timing = format_duration(elapsed);
        self.output.write_line(
            Indicator::Done,
            &self.label,
            result,
            Some(&timing),
            self.indent,
            self.full_width,
        );
    }

    /// Mark as done without timing. Consumes self (stops animation).
    pub fn done_untimed(self, result: &str) {
        drop(self.animation);
        self.output.clear_ephemeral();
        self.output.write_line(
            Indicator::Done,
            &self.label,
            result,
            None,
            self.indent,
            self.full_width,
        );
    }

    /// Mark as failed. Consumes self (stops animation).
    pub fn fail(self, result: &str) {
        drop(self.animation);
        self.output.clear_ephemeral();
        let elapsed = self.start.elapsed();
        let timing = format_duration(elapsed);
        self.output.write_line(
            Indicator::Failed,
            &self.label,
            result,
            Some(&timing),
            self.indent,
            self.full_width,
        );
    }

    /// Mark as failed without timing. Consumes self (stops animation).
    pub fn fail_untimed(self, result: &str) {
        drop(self.animation);
        self.output.clear_ephemeral();
        self.output.write_line(
            Indicator::Failed,
            &self.label,
            result,
            None,
            self.indent,
            self.full_width,
        );
    }
}

/// A recorded child result within a group.
struct ChildLine {
    label: String,
    result: String,
    failed: bool,
}

/// A group of related steps.
///
/// Children are buffered and flushed on `.done()`.
/// On TTY with cursor: children show as live ephemeral lines during execution.
/// On non-TTY: everything is buffered and printed at once.
pub struct Group {
    output: Output,
    label: String,
    children: Vec<ChildLine>,
    start: Instant,
}

impl Group {
    /// Create a child step that shows ephemeral progress on TTY.
    pub fn step(&self, label: &str) -> Step {
        Step {
            output: self.output.clone(),
            label: label.to_string(),
            indent: 1,
            start: Instant::now(),
            animation: None,
            full_width: true,
        }
    }

    /// Record a completed child (buffered, printed on `.done()`).
    pub fn child_done(&mut self, label: &str, result: &str) {
        self.children.push(ChildLine {
            label: label.to_string(),
            result: result.to_string(),
            failed: false,
        });
    }

    /// Record a failed child (buffered, printed on `.done()`).
    pub fn child_fail(&mut self, label: &str, result: &str) {
        self.children.push(ChildLine {
            label: label.to_string(),
            result: result.to_string(),
            failed: true,
        });
    }

    /// Flush: print header + all children. Consumes self.
    pub fn done(self) {
        let total = self.children.len();
        let failed = self.children.iter().filter(|c| c.failed).count();
        let elapsed = self.start.elapsed();
        let timing = format_duration(elapsed);

        let (indicator, summary) = if failed > 0 {
            (Indicator::Failed, format!("{}/{} failed", failed, total))
        } else {
            (Indicator::Done, format!("{} done", total))
        };

        self.output
            .write_line(indicator, &self.label, &summary, Some(&timing), 0, true);

        for child in &self.children {
            let ind = if child.failed {
                Indicator::Failed
            } else {
                Indicator::Done
            };
            self.output
                .write_line(ind, &child.label, &child.result, None, 1, true);
        }
    }

    /// Flush with a custom summary. Consumes self.
    ///
    /// Uses `◒` (partial) if any child failed, `●` (done) if all succeeded.
    /// Child failures show as `✕` on their own lines.
    pub fn done_with_summary(self, summary: &str) {
        let has_failure = self.children.iter().any(|c| c.failed);
        let elapsed = self.start.elapsed();
        let timing = format_duration(elapsed);

        let indicator = if has_failure {
            Indicator::Partial
        } else {
            Indicator::Done
        };
        self.output
            .write_line(indicator, &self.label, summary, Some(&timing), 0, true);

        for child in &self.children {
            let ind = if child.failed {
                Indicator::Failed
            } else {
                Indicator::Done
            };
            self.output
                .write_line(ind, &child.label, &child.result, None, 1, true);
        }
    }
}

// === Pipeline ===

/// State of a single pipeline step.
#[derive(Clone)]
enum PipelineStepState {
    Pending,
    InProgress {
        status: String,
        frame: u8,
    },
    /// `display` is the pre-formatted result+timing string, computed once at transition.
    Done {
        timing: Option<String>,
        display: String,
    },
    Failed {
        timing: Option<String>,
        display: String,
    },
}

/// A pipeline shows all steps upfront as pending placeholders,
/// then redraws the entire block when any step changes state.
///
/// On non-TTY: falls back to sequential output (prints each step when done).
///
/// Uses full-redraw approach: move cursor to top of block, clear and reprint
/// all lines. This is reliable across all terminals.
pub struct Pipeline {
    output: Output,
    labels: Vec<String>,
    starts: Vec<Instant>,
    states: Vec<PipelineStepState>,
    total_lines: usize,
    /// True before the first redraw (cursor is 1 line below block).
    /// After first redraw, cursor is on the last line of the block.
    first_update: bool,
    /// Tracks which step has an active animation
    animation: Option<(usize, AnimationHandle)>,
}

/// Build a display string from result + optional timing.
/// Pre-computed once at state transition, avoiding per-tick allocation.
fn build_display(result: &str, timing: Option<&str>, color: bool) -> String {
    let mut s = result.to_string();
    if let Some(t) = timing {
        if color {
            write!(s, " \x1b[2m({})\x1b[0m", t).ok();
        } else {
            write!(s, " ({})", t).ok();
        }
    }
    s
}

/// Redraw all pipeline lines to a writer. Shared by both `Pipeline::redraw()`
/// and the animation thread.
fn redraw_pipeline(
    w: &mut dyn io::Write,
    caps: &Capabilities,
    labels: &[String],
    states: &[PipelineStepState],
    starts: &[Instant],
) {
    for (i, state) in states.iter().enumerate() {
        let label = &labels[i];
        let _ = write!(w, "\r\x1b[K");
        match state {
            PipelineStepState::Pending => {
                line::render_tty_no_newline(w, caps, Indicator::Pending, label, "waiting", 0);
            }
            PipelineStepState::InProgress { status, frame } => {
                let elapsed = format_duration(starts[i].elapsed());
                let result = format!("{} ({})", status, elapsed);
                line::render_tty_no_newline(
                    w,
                    caps,
                    Indicator::Spinning(*frame),
                    label,
                    &result,
                    0,
                );
            }
            PipelineStepState::Done { display, .. } => {
                line::render_tty_no_newline(w, caps, Indicator::Done, label, display, 0);
            }
            PipelineStepState::Failed { display, .. } => {
                line::render_tty_no_newline(w, caps, Indicator::Failed, label, display, 0);
            }
        }
        if i < states.len() - 1 {
            let _ = writeln!(w);
        }
    }
    let _ = w.flush();
}

impl Pipeline {
    /// Redraw all pipeline lines from current cursor position.
    fn redraw(&self, w: &mut dyn io::Write) {
        redraw_pipeline(
            w,
            &self.output.inner.caps,
            &self.labels,
            &self.states,
            &self.starts,
        );
    }

    /// Move cursor to the top of the pipeline block and redraw everything.
    ///
    /// After N writeln! calls, cursor is on the line after the last pipeline line.
    /// After a redraw (N-1 newlines between N lines), cursor is on the last pipeline line.
    /// We track this to know how far to move up.
    fn update(&mut self) {
        let mut w = self.output.inner.writer.lock().unwrap();
        // First call: cursor is 1 line below the block (after writeln! calls)
        // Subsequent calls: cursor is on the last line of the block (after redraw)
        let up = if self.first_update {
            self.first_update = false;
            self.total_lines // from line below block to first line
        } else {
            self.total_lines - 1 // from last line to first line
        };
        if up > 0 {
            let _ = write!(w, "\x1b[{}F", up);
        } else {
            let _ = write!(w, "\r");
        }
        self.redraw(&mut *w);
    }

    /// Start a live animation on the given step.
    /// Also resets the step's start time for accurate elapsed measurement.
    pub fn animate(&mut self, index: usize, status: &str) {
        // Stop any existing animation first
        self.animation = None;

        // Reset start time
        self.starts[index] = Instant::now();

        // Update state
        self.states[index] = PipelineStepState::InProgress {
            status: status.to_string(),
            frame: 0,
        };

        if !self.output.caps().cursor {
            return;
        }

        self.update();

        // Spawn animation thread that redraws periodically
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let output = self.output.clone();
        let labels = self.labels.clone();
        let starts = self.starts.clone();
        let states = self.states.clone();
        let total_lines = self.total_lines;
        let anim_index = index;
        let status = status.to_string();

        let thread = std::thread::spawn(move || {
            let mut frame: u8 = 0;
            let mut local_states = states;
            while !stop_clone.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(80));
                if stop_clone.load(Ordering::SeqCst) {
                    break;
                }
                frame = frame.wrapping_add(1);
                local_states[anim_index] = PipelineStepState::InProgress {
                    status: status.clone(),
                    frame,
                };
                let caps = &output.inner.caps;
                let mut w = output.inner.writer.lock().unwrap();
                if total_lines > 1 {
                    let _ = write!(w, "\x1b[{}F", total_lines - 1);
                } else {
                    let _ = write!(w, "\r");
                }
                redraw_pipeline(&mut *w, caps, &labels, &local_states, &starts);
            }
        });

        self.animation = Some((
            index,
            AnimationHandle {
                stop,
                thread: Some(thread),
            },
        ));
    }

    /// Build a Done/Failed state with pre-computed display string.
    fn make_done(&self, result: &str, timing: Option<String>) -> PipelineStepState {
        let display = build_display(result, timing.as_deref(), self.output.caps().color);
        PipelineStepState::Done { timing, display }
    }

    fn make_failed(&self, result: &str, timing: Option<String>) -> PipelineStepState {
        let display = build_display(result, timing.as_deref(), self.output.caps().color);
        PipelineStepState::Failed { timing, display }
    }

    /// Mark a step as done.
    pub fn done(&mut self, index: usize, result: &str) {
        if self.animation.as_ref().map(|(i, _)| *i) == Some(index) {
            self.animation = None;
        }

        let timing = format_duration(self.starts[index].elapsed());
        self.states[index] = self.make_done(result, Some(timing));

        if self.output.caps().cursor {
            self.update();
        } else {
            self.output.write_line(
                Indicator::Done,
                &self.labels[index],
                result,
                self.timing_str(index).as_deref(),
                0,
                true,
            );
        }
    }

    /// Mark a step as done without timing.
    pub fn done_untimed(&mut self, index: usize, result: &str) {
        if self.animation.as_ref().map(|(i, _)| *i) == Some(index) {
            self.animation = None;
        }

        self.states[index] = self.make_done(result, None);

        if self.output.caps().cursor {
            self.update();
        } else {
            self.output
                .write_line(Indicator::Done, &self.labels[index], result, None, 0, true);
        }
    }

    /// Mark a step as failed.
    pub fn fail(&mut self, index: usize, result: &str) {
        if self.animation.as_ref().map(|(i, _)| *i) == Some(index) {
            self.animation = None;
        }

        let timing = format_duration(self.starts[index].elapsed());
        self.states[index] = self.make_failed(result, Some(timing));

        if self.output.caps().cursor {
            self.update();
        } else {
            self.output.write_line(
                Indicator::Failed,
                &self.labels[index],
                result,
                self.timing_str(index).as_deref(),
                0,
                true,
            );
        }
    }

    /// Mark a step as failed without timing.
    pub fn fail_untimed(&mut self, index: usize, result: &str) {
        if self.animation.as_ref().map(|(i, _)| *i) == Some(index) {
            self.animation = None;
        }

        self.states[index] = self.make_failed(result, None);

        if self.output.caps().cursor {
            self.update();
        } else {
            self.output.write_line(
                Indicator::Failed,
                &self.labels[index],
                result,
                None,
                0,
                true,
            );
        }
    }

    /// Helper: get timing string from state (for non-TTY fallback).
    fn timing_str(&self, index: usize) -> Option<String> {
        match &self.states[index] {
            PipelineStepState::Done { timing, .. } | PipelineStepState::Failed { timing, .. } => {
                timing.clone()
            }
            _ => None,
        }
    }

    /// Finish the pipeline. Stops any animation and moves cursor below pipeline.
    /// Consumes self.
    pub fn finish(mut self) {
        self.animation = None;
        if self.output.caps().cursor {
            // Final redraw then newline to move past the block
            self.update();
            let mut w = self.output.inner.writer.lock().unwrap();
            let _ = writeln!(w);
            let _ = w.flush();
        }
    }
}

/// Format a duration as human-readable: "0.1s", "1.2s", "1m 5s".
fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else {
        let mins = secs as u64 / 60;
        let remaining = secs as u64 % 60;
        format!("{}m {}s", mins, remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_output(caps: Capabilities) -> (Output, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = WriterWrapper(buf.clone());
        let output = Output::with_test_writer(caps, Box::new(writer));
        (output, buf)
    }

    #[derive(Clone)]
    struct WriterWrapper(Arc<Mutex<Vec<u8>>>);

    impl Write for WriterWrapper {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn pipe_caps() -> Capabilities {
        Capabilities {
            color: false,
            cursor: false,
            width: 80,
        }
    }

    fn tty_caps() -> Capabilities {
        Capabilities {
            color: true,
            cursor: true,
            width: 80,
        }
    }

    #[test]
    fn test_step_pipe_format() {
        let (out, buf) = test_output(pipe_caps());
        out.step("nix").done_untimed("42 vars");
        let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "dtx: nix: 42 vars\n");
    }

    #[test]
    fn test_step_tty_compact() {
        let (out, buf) = test_output(tty_caps());
        out.step("nix").done_untimed("42 vars");
        let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(output.contains("●"));
        assert!(output.contains("nix"));
        assert!(output.contains("──")); // compact connector
        assert!(output.contains("42 vars"));
    }

    #[test]
    fn test_group_pipe_format() {
        let (out, buf) = test_output(pipe_caps());
        let mut grp = out.group("services");
        grp.child_done("postgres", ":5432");
        grp.child_done("api", ":3000");
        grp.done();
        let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(output.contains("dtx: services:"));
        assert!(output.contains("dtx:   postgres: :5432"));
        assert!(output.contains("dtx:   api: :3000"));
    }

    #[test]
    fn test_error_pipe_format() {
        let (out, _) = test_output(pipe_caps());
        // Error goes to err_writer which is a black hole in test mode
        out.error("something broke");
    }

    #[test]
    fn test_log_tty() {
        let (out, buf) = test_output(tty_caps());
        out.log("api", "listening on :3000", false);
        let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(output.contains("[api]"));
        assert!(output.contains("listening on :3000"));
    }

    #[test]
    fn test_log_pipe() {
        let (out, buf) = test_output(pipe_caps());
        out.log("api", "listening on :3000", false);
        let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "dtx: [api] listening on :3000\n");
    }

    #[test]
    fn test_separator_tty() {
        let (out, buf) = test_output(tty_caps());
        out.separator("logs (ctrl+c to stop)");
        let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(output.contains("logs (ctrl+c to stop)"));
        assert!(output.contains("───"));
    }

    #[test]
    fn test_table_pipe() {
        let (out, buf) = test_output(pipe_caps());
        out.table()
            .headers(vec!["NAME", "PORT"])
            .row(vec![Cell::new("api"), Cell::new("3000")])
            .render(&mut buf.lock().unwrap().as_mut_slice());
        // Table renders to a writer, not to the Output's writer
        // So let's test the table builder directly
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(
            format_duration(std::time::Duration::from_millis(100)),
            "0.1s"
        );
        assert_eq!(
            format_duration(std::time::Duration::from_millis(1234)),
            "1.2s"
        );
        assert_eq!(format_duration(std::time::Duration::from_secs(65)), "1m 5s");
    }
}
