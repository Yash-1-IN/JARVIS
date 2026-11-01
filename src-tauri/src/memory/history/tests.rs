use super::*;
use crate::memory::db::open_in_memory;

const MIN: u64 = 60_000;
const DAY: u64 = 86_400_000;

fn store() -> MemoryStore {
    MemoryStore::new(open_in_memory().unwrap())
}

fn say(s: &MemoryStore, role: HistoryRole, text: &str, at: u64) {
    assert!(s.record_message(role, text, None, at).unwrap().stored);
}

#[test]
fn retention_defaults_to_30_days_and_is_saved() {
    let s = store();
    assert_eq!(s.history_retention().unwrap(), HistoryRetention::Days30);
    s.set_history_retention(HistoryRetention::Forever, 1)
        .unwrap();
    assert_eq!(s.history_retention().unwrap(), HistoryRetention::Forever);
    s.set_history_retention(HistoryRetention::Days7, 2).unwrap();
    assert_eq!(s.history_retention().unwrap(), HistoryRetention::Days7);
}

#[test]
fn messages_in_a_row_share_a_conversation_and_idle_time_starts_a_new_one() {
    let s = store();
    let t0 = 1_000 * DAY;
    say(&s, HistoryRole::User, "hello there", t0);
    say(&s, HistoryRole::Assistant, "hi", t0 + MIN);
    say(&s, HistoryRole::User, "still here", t0 + 9 * MIN);
    // Eleven minutes after the last message: a new conversation.
    say(&s, HistoryRole::User, "a new topic", t0 + 20 * MIN + 1);
    let list = s.list_conversations(10, None).unwrap();
    assert_eq!(list.len(), 2);
    // Newest first, with its title from the first thing the user said.
    assert_eq!(list[0].title.as_deref(), Some("a new topic"));
    assert_eq!((list[0].message_count, list[0].ended_at), (1, None));
    assert_eq!(list[1].title.as_deref(), Some("hello there"));
    assert_eq!(list[1].message_count, 3);
    assert_eq!(
        list[1].ended_at,
        Some(t0 + 9 * MIN),
        "ended at its last message"
    );
}

#[test]
fn a_conversation_is_read_back_oldest_first_with_roles() {
    let s = store();
    say(&s, HistoryRole::User, "find my tax file", 10);
    s.record_message(
        HistoryRole::Tool,
        "Search for files named \"tax\"",
        Some("{}"),
        11,
    )
    .unwrap();
    say(&s, HistoryRole::Assistant, "Found one.", 12);
    let id = s.list_conversations(5, None).unwrap()[0].id.clone();
    let msgs = s.get_conversation(&id).unwrap();
    let got: Vec<_> = msgs.iter().map(|m| (m.role, m.content.as_str())).collect();
    assert_eq!(
        got,
        vec![
            (HistoryRole::User, "find my tax file"),
            (HistoryRole::Tool, "Search for files named \"tax\""),
            (HistoryRole::Assistant, "Found one."),
        ]
    );
    assert_eq!(s.get_conversation("nope"), Err(StoreError::NotFound));
}

#[test]
fn listing_pages_with_before_and_clamps_the_limit() {
    let s = store();
    for i in 0..5u64 {
        say(&s, HistoryRole::User, &format!("topic {i}"), i * DAY);
    }
    let first = s.list_conversations(2, None).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].title.as_deref(), Some("topic 4"));
    let next = s.list_conversations(2, Some(&first[1].id)).unwrap();
    assert_eq!(next[0].title.as_deref(), Some("topic 2"));
    let rest = s.list_conversations(500, Some(&next[1].id)).unwrap();
    assert_eq!(rest.len(), 1);
    assert!(
        s.list_conversations(5, Some("no-such-id"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        s.list_conversations(0, None).unwrap().len(),
        1,
        "limit is at least 1"
    );
}

#[test]
fn off_stores_nothing_and_empty_or_secret_text_is_handled() {
    let s = store();
    s.set_history_retention(HistoryRetention::Off, 1).unwrap();
    assert!(
        !s.record_message(HistoryRole::User, "private", None, 2)
            .unwrap()
            .stored
    );
    assert!(s.list_conversations(5, None).unwrap().is_empty());
    s.set_history_retention(HistoryRetention::Forever, 3)
        .unwrap();
    assert!(
        !s.record_message(HistoryRole::User, "  \n ", None, 4)
            .unwrap()
            .stored
    );
    say(
        &s,
        HistoryRole::User,
        "my key is sk-ant-api03-abcdefghijklmnop ok",
        5,
    );
    let id = s.list_conversations(5, None).unwrap()[0].id.clone();
    let text = &s.get_conversation(&id).unwrap()[0].content;
    assert!(!text.contains("abcdefghijklmnop"), "{text}");
    // Long text is cut.
    say(
        &s,
        HistoryRole::Assistant,
        &"y".repeat(MAX_MESSAGE_CHARS + 500),
        6,
    );
    let msgs = s.get_conversation(&id).unwrap();
    assert_eq!(msgs[1].content.chars().count(), MAX_MESSAGE_CHARS);
}

#[test]
fn purge_removes_only_what_is_older_than_the_period() {
    let s = store();
    let now = 2_000 * DAY;
    say(&s, HistoryRole::User, "ancient", now - 40 * DAY);
    say(&s, HistoryRole::User, "recent", now - 2 * DAY);
    assert_eq!(s.purge_history(now).unwrap(), 1, "default is 30 days");
    let list = s.list_conversations(10, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title.as_deref(), Some("recent"));
    // A shorter period applies the moment it is chosen.
    s.set_history_retention(HistoryRetention::Days7, now)
        .unwrap();
    assert_eq!(s.list_conversations(10, None).unwrap().len(), 1);
    s.set_history_retention(HistoryRetention::Days7, now + 10 * DAY)
        .unwrap();
    assert!(s.list_conversations(10, None).unwrap().is_empty());
    // Forever never purges.
    s.set_history_retention(HistoryRetention::Forever, now)
        .unwrap();
    say(&s, HistoryRole::User, "old but kept", now - 900 * DAY);
    assert_eq!(s.purge_history(now).unwrap(), 0);
}

#[test]
fn delete_all_empties_history_and_keeps_memories_and_settings() {
    let s = store();
    say(&s, HistoryRole::User, "one", 1);
    say(&s, HistoryRole::Assistant, "two", 2);
    let m = s.propose("Likes tea", None, 3).unwrap();
    s.accept(&m.id, 4).unwrap();
    s.set_history_retention(HistoryRetention::Days365, 5)
        .unwrap();
    s.delete_all_history().unwrap();
    assert!(s.list_conversations(10, None).unwrap().is_empty());
    let conn = s.conn();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "messages go with their conversations");
    drop(conn);
    assert_eq!(s.list(None).unwrap().len(), 1);
    assert_eq!(s.history_retention().unwrap(), HistoryRetention::Days365);
}

#[test]
fn deleted_history_is_gone_from_every_file_on_disk() {
    let dir = std::env::temp_dir().join(format!("jarvis-hist-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let s = MemoryStore::new(
        crate::memory::db::open(&dir.join("jarvis.db"), 1)
            .unwrap()
            .conn,
    );
    for i in 0..30 {
        say(
            &s,
            HistoryRole::User,
            &format!("filler line {i} about gardens"),
            i * DAY,
        );
    }
    say(
        &s,
        HistoryRole::User,
        "quokkahistoryword secretplan",
        31 * DAY,
    );
    let found = |needle: &str| {
        std::fs::read_dir(&dir).unwrap().flatten().any(|e| {
            let b = std::fs::read(e.path())
                .unwrap_or_default()
                .to_ascii_lowercase();
            b.windows(needle.len()).any(|w| w == needle.as_bytes())
        })
    };
    assert!(
        found("quokkahistoryword"),
        "the check can see the text before deleting"
    );
    s.delete_all_history().unwrap();
    assert!(
        !found("quokkahistoryword") && !found("secretplan"),
        "text left on disk"
    );
}

#[test]
fn the_export_has_memories_and_history_and_no_keys() {
    let s = store();
    say(&s, HistoryRole::User, "hello", 10);
    let m = s.propose("Likes tea", None, 11).unwrap();
    s.accept(&m.id, 12).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&s.export_json(99).unwrap()).unwrap();
    assert_eq!(doc["format"], "jarvis-export");
    assert_eq!(doc["exportedAt"], 99);
    assert_eq!(doc["historyRetention"], "days30");
    assert_eq!(doc["memories"][0]["content"], "Likes tea");
    assert_eq!(doc["memories"][0]["status"], "active");
    assert_eq!(doc["history"][0]["messages"][0]["content"], "hello");
    assert_eq!(doc["history"][0]["messages"][0]["role"], "user");
    let text = doc.to_string();
    assert!(!text.contains("sk-") && !text.to_lowercase().contains("apikey"));
}

#[test]
fn recording_says_when_a_new_conversation_began() {
    let s = store();
    let first = s
        .record_message(HistoryRole::User, "one", None, 10)
        .unwrap();
    assert_eq!((first.stored, first.new_conversation), (true, true));
    let same = s
        .record_message(HistoryRole::Assistant, "two", None, 10 + MIN)
        .unwrap();
    assert_eq!((same.stored, same.new_conversation), (true, false));
    let later = s
        .record_message(HistoryRole::User, "three", None, 10 + 12 * MIN)
        .unwrap();
    assert!(later.new_conversation, "after ten idle minutes");
}
