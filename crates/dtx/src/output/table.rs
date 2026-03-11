//! Column-aligned table output.

use super::caps::Capabilities;
use std::io::Write;

/// A cell in a table that may have ANSI styling.
pub struct Cell {
    pub content: String,
    /// Optional ANSI color code to wrap the content.
    pub color: Option<&'static str>,
}

impl Cell {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            color: None,
        }
    }

    pub fn colored(content: impl Into<String>, color: &'static str) -> Self {
        Self {
            content: content.into(),
            color: Some(color),
        }
    }

    fn visible_len(&self) -> usize {
        self.content.len()
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        Cell::new(s)
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Self {
        Cell::new(s)
    }
}

/// Builder for table output.
pub struct TableBuilder {
    caps: Capabilities,
    headers: Vec<String>,
    rows: Vec<Vec<Cell>>,
    col_widths: Vec<usize>,
}

impl TableBuilder {
    pub fn new(caps: Capabilities) -> Self {
        Self {
            caps,
            headers: Vec::new(),
            rows: Vec::new(),
            col_widths: Vec::new(),
        }
    }

    /// Set column headers.
    pub fn headers(mut self, headers: Vec<&str>) -> Self {
        self.col_widths = headers.iter().map(|h| h.len()).collect();
        self.headers = headers.into_iter().map(String::from).collect();
        self
    }

    /// Add a row of cells.
    pub fn row(mut self, cells: Vec<Cell>) -> Self {
        // Update column widths
        for (i, cell) in cells.iter().enumerate() {
            let len = cell.visible_len();
            if i < self.col_widths.len() {
                self.col_widths[i] = self.col_widths[i].max(len);
            } else {
                self.col_widths.push(len);
            }
        }
        self.rows.push(cells);
        self
    }

    /// Render the table to a writer.
    pub fn render(self, w: &mut dyn Write) {
        if self.caps.is_tty() {
            self.render_tty(w);
        } else {
            self.render_tsv(w);
        }
    }

    fn render_tty(self, w: &mut dyn Write) {
        // Pad columns by 2 extra spaces
        let padded_widths: Vec<usize> = self.col_widths.iter().map(|w| w + 2).collect();

        // Header
        if !self.headers.is_empty() {
            let mut line = String::new();
            for (i, header) in self.headers.iter().enumerate() {
                let width = padded_widths.get(i).copied().unwrap_or(header.len());
                if i == self.headers.len() - 1 {
                    line.push_str(header);
                } else {
                    line.push_str(&format!("{:<width$}", header, width = width));
                }
            }
            if self.caps.color {
                let _ = writeln!(w, "\x1b[1m{}\x1b[0m", line.trim_end());
            } else {
                let _ = writeln!(w, "{}", line.trim_end());
            }

            // Separator
            let total_width = padded_widths.iter().sum::<usize>().min(self.caps.width as usize);
            if self.caps.color {
                let _ = writeln!(w, "\x1b[2m{}\x1b[0m", "─".repeat(total_width));
            } else {
                let _ = writeln!(w, "{}", "─".repeat(total_width));
            }
        }

        // Rows
        for row in &self.rows {
            let mut line = String::new();
            for (i, cell) in row.iter().enumerate() {
                let width = padded_widths.get(i).copied().unwrap_or(cell.visible_len());
                let content = truncate(&cell.content, width.saturating_sub(2));

                if i == row.len() - 1 {
                    // Last column: no padding, but apply color
                    if self.caps.color {
                        if let Some(color) = cell.color {
                            line.push_str(&format!("{}{}\x1b[0m", color, content));
                        } else {
                            line.push_str(&content);
                        }
                    } else {
                        line.push_str(&content);
                    }
                } else if self.caps.color {
                    if let Some(color) = cell.color {
                        // Color the content but pad with spaces to maintain alignment
                        let padded = format!("{}{}\x1b[0m", color, content);
                        let visible = content.len();
                        let extra_padding = width.saturating_sub(visible);
                        line.push_str(&padded);
                        line.push_str(&" ".repeat(extra_padding));
                    } else {
                        line.push_str(&format!("{:<width$}", content, width = width));
                    }
                } else {
                    line.push_str(&format!("{:<width$}", content, width = width));
                }
            }
            let _ = writeln!(w, "{}", line.trim_end());
        }
    }

    fn render_tsv(self, w: &mut dyn Write) {
        // Headers as TSV
        if !self.headers.is_empty() {
            let _ = writeln!(w, "{}", self.headers.join("\t"));
        }

        // Rows as TSV
        for row in &self.rows {
            let cells: Vec<&str> = row.iter().map(|c| c.content.as_str()).collect();
            let _ = writeln!(w, "{}", cells.join("\t"));
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if max_len < 4 {
        return s.chars().take(max_len).collect();
    }
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tsv_output() {
        let caps = Capabilities {
            color: false,
            cursor: false,
            width: 80,
        };
        let mut buf = Vec::new();
        TableBuilder::new(caps)
            .headers(vec!["NAME", "PORT"])
            .row(vec![Cell::new("api"), Cell::new("3000")])
            .row(vec![Cell::new("db"), Cell::new("5432")])
            .render(&mut buf);

        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "NAME\tPORT\napi\t3000\ndb\t5432\n");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
    }
}
