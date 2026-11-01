//! The real `memory.propose` target: proposals go into the SQLite store and the window is
//! told, so it can show the confirmation card (`memory.proposed`).

use super::store::{MemoryStore, StoreError};
use crate::ipc::types::{Memory, MemoryStatus};
use crate::tools::ports::MemoryInbox;
use std::sync::Arc;

pub struct StoreInbox {
    store: Arc<MemoryStore>,
    now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    on_proposed: Box<dyn Fn(Memory) + Send + Sync>,
}

impl StoreInbox {
    pub fn new(
        store: Arc<MemoryStore>,
        now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
        on_proposed: Box<dyn Fn(Memory) + Send + Sync>,
    ) -> Self {
        Self {
            store,
            now_ms,
            on_proposed,
        }
    }
}

impl MemoryInbox for StoreInbox {
    fn propose(&self, text: &str) -> Result<String, String> {
        match self.store.propose(text, None, (self.now_ms)()) {
            Ok(m) => {
                // Only a row still waiting for an answer needs a card; one the user already
                // accepted (the same text) does not.
                if m.status == MemoryStatus::Proposed {
                    (self.on_proposed)(m.clone());
                }
                Ok(m.id)
            }
            Err(StoreError::Invalid(m)) => Err(m),
            Err(StoreError::NotFound) => Err("That memory no longer exists.".into()),
            Err(StoreError::Db(detail)) => {
                tracing::error!(detail = %detail, "memory database error");
                Err("The memory database failed.".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::db::open_in_memory;
    use std::sync::Mutex;

    fn inbox() -> (StoreInbox, Arc<MemoryStore>, Arc<Mutex<Vec<Memory>>>) {
        let store = Arc::new(MemoryStore::new(open_in_memory().unwrap()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let i = StoreInbox::new(
            store.clone(),
            Box::new(|| 5),
            Box::new(move |m| sink.lock().unwrap().push(m)),
        );
        (i, store, seen)
    }

    #[test]
    fn a_proposal_is_stored_and_announced_once() {
        let (i, store, seen) = inbox();
        let id = i.propose("Likes tea").unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(seen.lock().unwrap()[0].status, MemoryStatus::Proposed);
        assert_eq!(store.list(None).unwrap()[0].id, id);
        // Asking again before an answer is the same memory; the card may show again.
        assert_eq!(i.propose("Likes tea").unwrap(), id);
        // Once accepted, the same text is not proposed again.
        store.accept(&id, 6).unwrap();
        let before = seen.lock().unwrap().len();
        assert_eq!(i.propose("Likes tea").unwrap(), id);
        assert_eq!(seen.lock().unwrap().len(), before);
    }

    #[test]
    fn bad_text_is_refused_with_a_reason_and_announces_nothing() {
        let (i, _, seen) = inbox();
        assert!(i.propose("   ").is_err());
        assert!(i.propose(&"x".repeat(501)).unwrap_err().contains("500"));
        assert!(seen.lock().unwrap().is_empty());
    }
}
