//! Directory sizes: a per-path cache with a TTL and a `du -sk` runner.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::core::git::run_with_timeout;

pub struct SizeCache {
    ttl: Duration,
    /// None records a failed measurement: not retried until the TTL passes.
    entries: HashMap<String, (Instant, Option<u64>)>,
}

impl SizeCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    /// Fresh value only: an entry older than the TTL is not returned.
    pub fn get(&self, path: &str, now: Instant) -> Option<u64> {
        let (at, bytes) = self.entries.get(path)?;
        if now.saturating_duration_since(*at) < self.ttl {
            *bytes
        } else {
            None
        }
    }

    fn is_fresh(&self, path: &str, now: Instant) -> bool {
        self.entries
            .get(path)
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) < self.ttl)
    }

    pub fn put(&mut self, path: &str, bytes: u64, now: Instant) {
        self.entries.insert(path.to_string(), (now, Some(bytes)));
    }

    /// Records a failed or timed-out measurement; `get` stays None, `stale` skips it.
    pub fn put_failure(&mut self, path: &str, now: Instant) {
        self.entries.insert(path.to_string(), (now, None));
    }

    /// Paths with no fresh value, in input order.
    pub fn stale(&self, paths: &[String], now: Instant) -> Vec<String> {
        paths
            .iter()
            .filter(|p| !self.is_fresh(p, now))
            .cloned()
            .collect()
    }
}

/// First column of `du -sk` output, in bytes.
fn parse_du_kib(output: &str) -> Option<u64> {
    let kib: u64 = output.split_whitespace().next()?.parse().ok()?;
    kib.checked_mul(1024)
}

/// Size of `path` in bytes (`du -sk`); None when missing, unreadable or slower than `timeout`.
pub fn du(path: &Path, timeout: Duration) -> Option<u64> {
    let mut cmd = Command::new("du");
    cmd.arg("-sk").arg(path);
    parse_du_kib(&run_with_timeout(cmd, timeout)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_returns_fresh_values_only() {
        let t0 = Instant::now();
        let mut c = SizeCache::new(Duration::from_secs(600));
        assert_eq!(c.get("/a", t0), None);
        c.put("/a", 42, t0);
        assert_eq!(c.get("/a", t0 + Duration::from_secs(599)), Some(42));
        assert_eq!(c.get("/a", t0 + Duration::from_secs(600)), None);
    }

    #[test]
    fn stale_lists_missing_and_expired_paths() {
        let t0 = Instant::now();
        let mut c = SizeCache::new(Duration::from_secs(600));
        c.put("/old", 1, t0);
        c.put("/new", 2, t0 + Duration::from_secs(500));
        let paths: Vec<String> = ["/old", "/new", "/missing"]
            .into_iter()
            .map(String::from)
            .collect();
        let now = t0 + Duration::from_secs(700);
        assert_eq!(c.stale(&paths, now), vec!["/old", "/missing"]);
    }

    #[test]
    fn a_failed_measurement_is_not_stale_until_the_ttl_passes() {
        let t0 = Instant::now();
        let mut c = SizeCache::new(Duration::from_secs(600));
        c.put_failure("/slow", t0);
        let paths = vec!["/slow".to_string()];
        let soon = t0 + Duration::from_secs(60);
        assert_eq!(c.get("/slow", soon), None);
        assert!(c.stale(&paths, soon).is_empty());
        assert_eq!(c.stale(&paths, t0 + Duration::from_secs(601)), paths);
    }

    #[test]
    fn du_measures_a_directory_and_rejects_a_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("big.bin"), vec![1u8; 1024 * 1024]).unwrap();
        let bytes = du(tmp.path(), Duration::from_secs(30)).unwrap();
        assert!(bytes >= 1024 * 1024, "{bytes}");
        assert_eq!(
            du(&tmp.path().join("missing"), Duration::from_secs(30)),
            None
        );
    }

    #[test]
    fn parse_du_kib_reads_the_first_column() {
        assert_eq!(parse_du_kib("12\t/some path\n"), Some(12 * 1024));
        assert_eq!(parse_du_kib(""), None);
        assert_eq!(parse_du_kib("x\t/p\n"), None);
    }
}
