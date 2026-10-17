//! State of the Worktrees tool, shared by the scan, the size worker and the commands.

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use super::scan::WorktreesSnapshot;
use super::size::SizeCache;

pub type WorktreesRef<'a> = tauri::State<'a, Arc<WorktreesState>>;

const SIZE_TTL: Duration = Duration::from_secs(600);

pub struct WorktreesState {
    pub snapshot: RwLock<WorktreesSnapshot>,
    pub scanning: AtomicBool,
    /// Set by every scan request; the running scan reads it when it finishes.
    pub rescan_pending: AtomicBool,
    pub sizes: Mutex<SizeCache>,
    /// Paths waiting for `du`, oldest first.
    pub size_queue: Mutex<VecDeque<String>>,
    /// Path `du` is running on; not queued again meanwhile.
    pub size_in_flight: Mutex<Option<String>>,
    /// Wakes the size worker when `size_queue` gets a path.
    pub size_wake: Condvar,
}

impl Default for WorktreesState {
    fn default() -> Self {
        Self::new()
    }
}

impl WorktreesState {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(WorktreesSnapshot::default()),
            scanning: AtomicBool::new(false),
            rescan_pending: AtomicBool::new(false),
            sizes: Mutex::new(SizeCache::new(SIZE_TTL)),
            size_queue: Mutex::new(VecDeque::new()),
            size_in_flight: Mutex::new(None),
            size_wake: Condvar::new(),
        }
    }

    /// Queues paths for `du`, skipping those already queued.
    pub fn enqueue_sizes(&self, paths: Vec<String>) {
        let mut queue = self.size_queue.lock().unwrap_or_else(|e| e.into_inner());
        let in_flight = self
            .size_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for path in paths {
            if !queue.contains(&path) && in_flight.as_ref() != Some(&path) {
                queue.push_back(path);
            }
        }
        drop(in_flight);
        drop(queue);
        self.size_wake.notify_one();
    }

    /// Blocks until a path is queued.
    pub fn next_size_job(&self) -> String {
        let mut queue = self.size_queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(path) = queue.pop_front() {
                // Set while the queue lock is held: no gap in which the path is in neither.
                *self
                    .size_in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
                return path;
            }
            queue = self
                .size_wake
                .wait(queue)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The measurement started by `next_size_job` is over.
    pub fn finish_size_job(&self) {
        *self
            .size_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Writes a measured size into the snapshot; false when no entry has that path.
    pub fn apply_size(&self, path: &str, bytes: u64) -> bool {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|e| e.into_inner());
        let mut found = false;
        for repo in &mut snapshot.repos {
            if repo.path == path {
                repo.main_size = Some(bytes);
                found = true;
            }
            for wt in repo.worktrees.iter_mut().filter(|w| w.path == path) {
                wt.size_bytes = Some(bytes);
                found = true;
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::worktrees::inspect::{UpstreamState, WorktreeInfo};
    use crate::tools::worktrees::scan::RepoGroup;

    #[test]
    fn enqueue_skips_paths_already_queued() {
        let s = WorktreesState::new();
        s.enqueue_sizes(vec!["/a".into(), "/b".into()]);
        s.enqueue_sizes(vec!["/a".into(), "/c".into()]);
        assert_eq!(s.next_size_job(), "/a");
        assert_eq!(s.next_size_job(), "/b");
        assert_eq!(s.next_size_job(), "/c");
    }

    #[test]
    fn the_path_being_measured_is_not_queued_again() {
        let s = WorktreesState::new();
        s.enqueue_sizes(vec!["/a".into()]);
        assert_eq!(s.next_size_job(), "/a");
        s.enqueue_sizes(vec!["/a".into(), "/b".into()]);
        assert_eq!(s.size_queue.lock().unwrap().len(), 1);
        s.finish_size_job();
        s.enqueue_sizes(vec!["/a".into()]);
        assert_eq!(s.size_queue.lock().unwrap().len(), 2);
    }

    #[test]
    fn apply_size_updates_the_main_checkout_and_the_worktree() {
        let s = WorktreesState::new();
        s.snapshot.write().unwrap().repos = vec![RepoGroup {
            path: "/r".into(),
            name: "r".into(),
            main_size: None,
            worktrees: vec![WorktreeInfo {
                path: "/w".into(),
                branch: None,
                last_commit_at: None,
                changes: Some(0),
                upstream: UpstreamState::None,
                merged: false,
                trusted: true,
                size_bytes: None,
            }],
        }];
        assert!(s.apply_size("/r", 10));
        assert!(s.apply_size("/w", 20));
        assert!(!s.apply_size("/other", 30));
        let snap = s.snapshot.read().unwrap();
        assert_eq!(snap.repos[0].main_size, Some(10));
        assert_eq!(snap.repos[0].worktrees[0].size_bytes, Some(20));
    }
}
