//! File-backed log storage with in-memory recent buffer.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use super::app::DisplayLog;

struct ServiceLogFile {
    path: PathBuf,
    writer: BufWriter<File>,
    total_lines: usize,
}

/// File-backed log store with in-memory recent buffer for fast rendering.
pub struct LogStore {
    log_dir: Option<PathBuf>,
    files: HashMap<String, ServiceLogFile>,
    recent: VecDeque<DisplayLog>,
    max_recent: usize,
}

impl LogStore {
    /// Create a new LogStore. If log_dir creation fails, falls back to memory-only mode.
    pub fn new(log_dir: PathBuf, max_recent: usize) -> Self {
        let dir = match fs::create_dir_all(&log_dir) {
            Ok(()) => {
                tracing::debug!("Log directory: {}", log_dir.display());
                Some(log_dir)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to create log directory {}: {}. Using memory-only mode.",
                    log_dir.display(),
                    e
                );
                None
            }
        };

        Self {
            log_dir: dir,
            files: HashMap::new(),
            recent: VecDeque::with_capacity(max_recent),
            max_recent,
        }
    }

    /// Create a memory-only LogStore (no disk persistence).
    pub fn memory_only(max_recent: usize) -> Self {
        Self {
            log_dir: None,
            files: HashMap::new(),
            recent: VecDeque::with_capacity(max_recent),
            max_recent,
        }
    }

    /// Append a log entry.
    pub fn append(&mut self, log: DisplayLog) {
        // Write to disk if available
        if let Some(ref log_dir) = self.log_dir {
            let file = self.files.entry(log.service.clone()).or_insert_with(|| {
                let path = log_dir.join(format!("{}.log", log.service));
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .unwrap_or_else(|e| {
                        tracing::warn!("Failed to open log file {}: {}", path.display(), e);
                        File::create(std::env::temp_dir().join(format!("dtx-{}.log", log.service)))
                            .expect("Failed to create temp log file")
                    });
                let total_lines = count_lines(&path).unwrap_or(0);
                ServiceLogFile {
                    path,
                    writer: BufWriter::new(file),
                    total_lines,
                }
            });

            let prefix = if log.is_stderr { "2" } else { "1" };
            if writeln!(file.writer, "[{}] {}", prefix, log.content).is_ok() {
                file.total_lines += 1;
            }
        }

        // Always keep in recent buffer
        self.recent.push_back(log);
        while self.recent.len() > self.max_recent {
            self.recent.pop_front();
        }
    }

    /// Get visible logs for rendering (from recent buffer).
    pub fn get_visible(
        &self,
        service: Option<&str>,
        offset_from_bottom: usize,
        height: usize,
    ) -> Vec<&DisplayLog> {
        let filtered: Vec<&DisplayLog> = if let Some(name) = service {
            self.recent.iter().filter(|l| l.service == name).collect()
        } else {
            self.recent.iter().collect()
        };

        let total = filtered.len();
        let end = total.saturating_sub(offset_from_bottom);
        let start = end.saturating_sub(height);
        filtered[start..end].to_vec()
    }

    /// Flush all file writers to disk.
    #[allow(dead_code)]
    pub fn flush(&mut self) {
        for file in self.files.values_mut() {
            let _ = file.writer.flush();
        }
    }

    /// Load logs from disk for deep scrollback beyond the recent buffer.
    #[allow(dead_code)]
    pub fn load_from_disk(
        &mut self,
        service: &str,
        start_line: usize,
        count: usize,
    ) -> io::Result<Vec<DisplayLog>> {
        self.flush_service(service);
        let file = match self.files.get(service) {
            Some(f) => f,
            None => return Ok(Vec::new()),
        };

        let reader = BufReader::new(File::open(&file.path)?);
        let logs: Vec<DisplayLog> = reader
            .lines()
            .skip(start_line)
            .take(count)
            .filter_map(|line| line.ok())
            .map(|line| parse_log_line(service, line))
            .collect();

        Ok(logs)
    }

    /// Read the last `count` lines from a log file by seeking from the end.
    #[allow(dead_code)]
    pub fn load_tail(&mut self, service: &str, count: usize) -> io::Result<Vec<DisplayLog>> {
        use std::io::{Read, Seek, SeekFrom};

        self.flush_service(service);
        let path = match self.files.get(service) {
            Some(f) => &f.path,
            None => return Ok(Vec::new()),
        };

        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len == 0 {
            return Ok(Vec::new());
        }

        // Read backwards in chunks, collecting them in reverse order
        let chunk_size: u64 = 8192;
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut pos = file_len;
        let mut newline_count = 0;

        loop {
            let read_start = pos.saturating_sub(chunk_size);
            let read_len = (pos - read_start) as usize;
            if read_len == 0 {
                break;
            }

            file.seek(SeekFrom::Start(read_start))?;
            let mut chunk = vec![0u8; read_len];
            file.read_exact(&mut chunk)?;

            for &b in chunk.iter().rev() {
                if b == b'\n' {
                    newline_count += 1;
                    if newline_count > count {
                        break;
                    }
                }
            }

            chunks.push(chunk);
            pos = read_start;

            if newline_count > count || pos == 0 {
                break;
            }
        }

        // Assemble chunks in correct order (they were collected newest-first)
        chunks.reverse();
        let buf: Vec<u8> = chunks.into_iter().flatten().collect();

        // BufRead::lines handles UTF-8 per-line, avoiding mid-character splits
        let cursor = io::Cursor::new(buf);
        let all_lines: Vec<String> = BufRead::lines(BufReader::new(cursor))
            .map_while(Result::ok)
            .collect();
        let start = all_lines.len().saturating_sub(count);
        let logs = all_lines[start..]
            .iter()
            .map(|line| parse_log_line(service, line.clone()))
            .collect();

        Ok(logs)
    }

    /// Flush a service's log writer to disk before reading.
    fn flush_service(&mut self, service: &str) {
        if let Some(file) = self.files.get_mut(service) {
            let _ = file.writer.flush();
        }
    }

    /// Total number of log lines for a service (or all).
    #[allow(dead_code)]
    pub fn total_lines(&self, service: Option<&str>) -> usize {
        match service {
            Some(name) => self.files.get(name).map(|f| f.total_lines).unwrap_or(0),
            None => self.files.values().map(|f| f.total_lines).sum(),
        }
    }

    /// Count of logs in recent buffer for a service (or all).
    pub fn filtered_count(&self, service: Option<&str>) -> usize {
        match service {
            Some(name) => self.recent.iter().filter(|l| l.service == name).count(),
            None => self.recent.len(),
        }
    }

    /// Count of logs in recent buffer matching a text filter.
    pub fn filtered_count_with_predicate(&self, service: Option<&str>, filter: &str) -> usize {
        let filter_lower = filter.to_lowercase();
        match service {
            Some(name) => self
                .recent
                .iter()
                .filter(|l| l.service == name && l.content.to_lowercase().contains(&filter_lower))
                .count(),
            None => self
                .recent
                .iter()
                .filter(|l| l.content.to_lowercase().contains(&filter_lower))
                .count(),
        }
    }

    /// Get visible logs filtered by a text predicate.
    pub fn get_visible_filtered(
        &self,
        service: Option<&str>,
        filter: &str,
        offset_from_bottom: usize,
        height: usize,
    ) -> Vec<&DisplayLog> {
        let filter_lower = filter.to_lowercase();
        let filtered: Vec<&DisplayLog> = if let Some(name) = service {
            self.recent
                .iter()
                .filter(|l| l.service == name && l.content.to_lowercase().contains(&filter_lower))
                .collect()
        } else {
            self.recent
                .iter()
                .filter(|l| l.content.to_lowercase().contains(&filter_lower))
                .collect()
        };

        let total = filtered.len();
        let end = total.saturating_sub(offset_from_bottom);
        let start = end.saturating_sub(height);
        filtered[start..end].to_vec()
    }

    /// Clear logs for a service (or all).
    pub fn clear(&mut self, service: Option<&str>) {
        match service {
            Some(name) => {
                self.recent.retain(|l| l.service != name);
                if let Some(file) = self.files.get_mut(name) {
                    if let Ok(f) = File::create(&file.path) {
                        file.writer = BufWriter::new(f);
                        file.total_lines = 0;
                    }
                }
            }
            None => {
                self.recent.clear();
                for file in self.files.values_mut() {
                    if let Ok(f) = File::create(&file.path) {
                        file.writer = BufWriter::new(f);
                        file.total_lines = 0;
                    }
                }
            }
        }
    }
}

fn parse_log_line(service: &str, line: String) -> DisplayLog {
    let (is_stderr, content) = if let Some(rest) = line.strip_prefix("[2] ") {
        (true, rest.to_string())
    } else if let Some(rest) = line.strip_prefix("[1] ") {
        (false, rest.to_string())
    } else if let Some(rest) = line.strip_prefix("[ERR] ") {
        // Legacy format compatibility
        (true, rest.to_string())
    } else if let Some(rest) = line.strip_prefix("[OUT] ") {
        // Legacy format compatibility
        (false, rest.to_string())
    } else {
        (false, line)
    };
    DisplayLog {
        service: service.to_string(),
        content,
        is_stderr,
    }
}

fn count_lines(path: &PathBuf) -> io::Result<usize> {
    let file = File::open(path)?;
    Ok(BufReader::new(file).lines().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_only_append_and_get_visible() {
        let mut store = LogStore::memory_only(100);
        for i in 0..10 {
            store.append(DisplayLog {
                service: "api".to_string(),
                content: format!("line {}", i),
                is_stderr: false,
            });
        }

        let visible = store.get_visible(Some("api"), 0, 5);
        assert_eq!(visible.len(), 5);
        assert_eq!(visible[0].content, "line 5");
        assert_eq!(visible[4].content, "line 9");
    }

    #[test]
    fn test_scroll_offset() {
        let mut store = LogStore::memory_only(100);
        for i in 0..20 {
            store.append(DisplayLog {
                service: "api".to_string(),
                content: format!("line {}", i),
                is_stderr: false,
            });
        }

        let visible = store.get_visible(Some("api"), 5, 5);
        assert_eq!(visible.len(), 5);
        assert_eq!(visible[0].content, "line 10");
        assert_eq!(visible[4].content, "line 14");
    }

    #[test]
    fn test_filtered_count() {
        let mut store = LogStore::memory_only(100);
        store.append(DisplayLog {
            service: "api".to_string(),
            content: "a".to_string(),
            is_stderr: false,
        });
        store.append(DisplayLog {
            service: "web".to_string(),
            content: "b".to_string(),
            is_stderr: false,
        });
        store.append(DisplayLog {
            service: "api".to_string(),
            content: "c".to_string(),
            is_stderr: false,
        });

        assert_eq!(store.filtered_count(Some("api")), 2);
        assert_eq!(store.filtered_count(Some("web")), 1);
        assert_eq!(store.filtered_count(None), 3);
    }

    #[test]
    fn test_clear_service() {
        let mut store = LogStore::memory_only(100);
        store.append(DisplayLog {
            service: "api".to_string(),
            content: "a".to_string(),
            is_stderr: false,
        });
        store.append(DisplayLog {
            service: "web".to_string(),
            content: "b".to_string(),
            is_stderr: false,
        });

        store.clear(Some("api"));
        assert_eq!(store.filtered_count(Some("api")), 0);
        assert_eq!(store.filtered_count(Some("web")), 1);
    }

    #[test]
    fn test_clear_all() {
        let mut store = LogStore::memory_only(100);
        store.append(DisplayLog {
            service: "api".to_string(),
            content: "a".to_string(),
            is_stderr: false,
        });
        store.append(DisplayLog {
            service: "web".to_string(),
            content: "b".to_string(),
            is_stderr: false,
        });

        store.clear(None);
        assert_eq!(store.filtered_count(None), 0);
    }

    #[test]
    fn test_max_recent_limit() {
        let mut store = LogStore::memory_only(5);
        for i in 0..10 {
            store.append(DisplayLog {
                service: "api".to_string(),
                content: format!("line {}", i),
                is_stderr: false,
            });
        }
        assert_eq!(store.filtered_count(None), 5);
        let visible = store.get_visible(None, 0, 5);
        assert_eq!(visible[0].content, "line 5");
    }

    #[test]
    fn test_disk_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = LogStore::new(dir.path().to_path_buf(), 100);

        store.append(DisplayLog {
            service: "api".to_string(),
            content: "hello world".to_string(),
            is_stderr: false,
        });
        store.append(DisplayLog {
            service: "api".to_string(),
            content: "error msg".to_string(),
            is_stderr: true,
        });

        assert_eq!(store.total_lines(Some("api")), 2);

        let loaded = store.load_from_disk("api", 0, 10).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content, "hello world");
        assert!(!loaded[0].is_stderr);
        assert_eq!(loaded[1].content, "error msg");
        assert!(loaded[1].is_stderr);
    }

    #[test]
    fn test_tail_reading() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = LogStore::new(dir.path().to_path_buf(), 100);

        for i in 0..50 {
            store.append(DisplayLog {
                service: "api".to_string(),
                content: format!("line {}", i),
                is_stderr: false,
            });
        }

        // Tail: read last 5 lines
        let tail = store.load_tail("api", 5).unwrap();
        assert_eq!(tail.len(), 5);
        assert_eq!(tail[0].content, "line 45");
        assert_eq!(tail[4].content, "line 49");
    }

    #[test]
    fn test_tail_fewer_lines_than_requested() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = LogStore::new(dir.path().to_path_buf(), 100);

        store.append(DisplayLog {
            service: "api".to_string(),
            content: "only line".to_string(),
            is_stderr: false,
        });

        let tail = store.load_tail("api", 100).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].content, "only line");
    }
}
