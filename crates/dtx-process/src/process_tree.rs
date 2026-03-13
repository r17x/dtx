//! Cross-platform process tree enumeration.
//!
//! Walks the process table to find all transitive descendants of a given root PID.
//! - macOS: uses `sysctl(KERN_PROC_ALL)` to snapshot the process table
//! - Linux: reads `/proc/*/stat` to build the PPID→children map

use std::collections::{HashMap, VecDeque};

/// Returns all transitive descendant PIDs of `root_pid` (excludes root itself).
pub fn get_descendant_pids(root_pid: u32) -> Vec<u32> {
    let parent_map = build_ppid_map();

    // Build children map from parent map
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&pid, &ppid) in &parent_map {
        children.entry(ppid).or_default().push(pid);
    }

    // BFS from root
    let mut descendants = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(root_pid);

    while let Some(pid) = queue.pop_front() {
        if let Some(kids) = children.get(&pid) {
            for &kid in kids {
                descendants.push(kid);
                queue.push_back(kid);
            }
        }
    }

    descendants
}

/// Build a map of PID → PPID for all processes.
#[cfg(target_os = "macos")]
fn build_ppid_map() -> HashMap<u32, u32> {
    use std::mem;
    use std::ptr;

    let mut map = HashMap::new();

    unsafe {
        // First call: get process count
        let count = libc::proc_listallpids(ptr::null_mut(), 0);
        if count <= 0 {
            return map;
        }

        // Add padding for new processes between calls
        let capacity = (count as usize) * 120 / 100;
        let mut pids: Vec<libc::pid_t> = vec![0; capacity];
        let buf_size = (capacity * mem::size_of::<libc::pid_t>()) as libc::c_int;

        let actual = libc::proc_listallpids(pids.as_mut_ptr() as *mut libc::c_void, buf_size);
        if actual <= 0 {
            return map;
        }

        let actual_count = actual as usize;
        for &pid in &pids[..actual_count] {
            if pid <= 0 {
                continue;
            }

            let mut info: libc::proc_bsdinfo = mem::zeroed();
            let size = libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut libc::c_void,
                mem::size_of::<libc::proc_bsdinfo>() as libc::c_int,
            );

            if size == mem::size_of::<libc::proc_bsdinfo>() as libc::c_int {
                map.insert(info.pbi_pid, info.pbi_ppid);
            }
        }
    }

    map
}

#[cfg(target_os = "linux")]
fn build_ppid_map() -> HashMap<u32, u32> {
    let mut map = HashMap::new();

    let proc_dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return map,
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Only numeric directories (PIDs)
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let stat_path = format!("/proc/{}/stat", pid);
        let stat = match std::fs::read_to_string(&stat_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Format: pid (comm) state ppid ...
        // Find closing paren to handle comm with spaces
        if let Some(close_paren) = stat.rfind(')') {
            let rest = &stat[close_paren + 2..]; // skip ") "
            let fields: Vec<&str> = rest.split_whitespace().collect();
            // fields[0] = state, fields[1] = ppid
            if let Some(ppid_str) = fields.get(1) {
                if let Ok(ppid) = ppid_str.parse::<u32>() {
                    map.insert(pid, ppid);
                }
            }
        }
    }

    map
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn build_ppid_map() -> HashMap<u32, u32> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_descendant_pids_returns_vec() {
        // Use current process — guaranteed to exist in the process table.
        // It may or may not have descendants depending on the environment,
        // so just verify it returns without error.
        let descendants = get_descendant_pids(std::process::id());
        // Result is a Vec (possibly empty if no children); just confirm it runs.
        let _ = descendants;
    }

    #[test]
    fn test_get_descendant_pids_nonexistent() {
        // Very high PID that doesn't exist
        let descendants = get_descendant_pids(u32::MAX);
        assert!(descendants.is_empty());
    }

    #[test]
    fn test_build_ppid_map_contains_current_process() {
        let map = build_ppid_map();
        // The current process should appear in the ppid map
        assert!(
            map.contains_key(&std::process::id()),
            "ppid map should contain current process"
        );
    }
}
