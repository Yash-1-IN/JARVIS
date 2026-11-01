//! In-memory activity log: what ran, on what, and how it ended. Never file contents or
//! arguments (spec §9.3). Persistence to SQLite arrives with M5.

use crate::ipc::types::ActivityEntry;
use std::collections::VecDeque;
use std::sync::Mutex;

pub const MAX_ENTRIES: usize = 10_000;

pub struct ActivityLog {
    rows: Mutex<VecDeque<ActivityEntry>>,
    cap: usize,
}

impl ActivityLog {
    pub fn new(cap: usize) -> Self {
        Self {
            rows: Mutex::new(VecDeque::new()),
            cap: cap.max(1),
        }
    }

    fn rows(&self) -> std::sync::MutexGuard<'_, VecDeque<ActivityEntry>> {
        self.rows.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Append-only; the oldest rows drop off past the cap.
    pub fn append(&self, e: ActivityEntry) {
        let mut r = self.rows();
        r.push_back(e);
        while r.len() > self.cap {
            r.pop_front();
        }
    }

    /// Newest first. `before` is an entry id: only older entries are returned. An unknown
    /// id gives nothing. `limit` is clamped to 1..=200.
    pub fn list(&self, limit: usize, before: Option<&str>) -> Vec<ActivityEntry> {
        let r = self.rows();
        let limit = limit.clamp(1, 200);
        let end = match before {
            None => r.len(),
            Some(id) => match r.iter().position(|e| e.id == id) {
                Some(i) => i,
                None => return Vec::new(),
            },
        };
        r.iter().take(end).rev().take(limit).cloned().collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.rows().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::types::{ToolDecision, ToolOutcome, ToolRisk};

    fn entry(n: u32) -> ActivityEntry {
        ActivityEntry {
            id: format!("id{n:03}"),
            ts: u64::from(n),
            request_id: "r".into(),
            tool: "time.now".into(),
            risk: ToolRisk::Low,
            summary: "s".into(),
            targets: vec![],
            decision: ToolDecision::AutoAllowed,
            outcome: ToolOutcome::Ok,
            duration_ms: 1,
        }
    }

    #[test]
    fn newest_first_with_paging() {
        let log = ActivityLog::new(100);
        for n in 1..=5 {
            log.append(entry(n));
        }
        let page: Vec<_> = log.list(2, None).into_iter().map(|e| e.id).collect();
        assert_eq!(page, ["id005", "id004"]);
        let next: Vec<_> = log
            .list(10, Some("id004"))
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(next, ["id003", "id002", "id001"]);
        assert!(log.list(10, Some("nope")).is_empty());
    }

    #[test]
    fn cap_drops_the_oldest_and_limit_is_clamped() {
        let log = ActivityLog::new(3);
        for n in 1..=5 {
            log.append(entry(n));
        }
        assert_eq!(log.len(), 3);
        assert_eq!(log.list(1000, None).len(), 3);
        assert_eq!(
            log.list(0, None).len(),
            1,
            "a limit of zero is treated as one"
        );
        assert_eq!(log.list(1, None)[0].id, "id005");
    }
}
