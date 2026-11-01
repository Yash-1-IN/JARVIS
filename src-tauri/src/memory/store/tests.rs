use super::*;
use crate::memory::db::open_in_memory;

fn store() -> MemoryStore {
    MemoryStore::new(open_in_memory().unwrap())
}

#[test]
fn a_new_memory_is_only_proposed_and_is_never_retrieved() {
    let s = store();
    let m = s.propose("Prefers green tea", None, 10).unwrap();
    assert_eq!(m.status, MemoryStatus::Proposed);
    assert_eq!((m.created_at, m.updated_at), (10, 10));
    assert!(s.retrieve("what tea do I like", 5).unwrap().is_empty());
    assert_eq!(s.list(Some(MemoryStatus::Proposed)).unwrap().len(), 1);
    assert!(s.list(Some(MemoryStatus::Active)).unwrap().is_empty());
}

#[test]
fn accepting_makes_it_retrievable_and_is_idempotent() {
    let s = store();
    let m = s.propose("Prefers green tea", None, 10).unwrap();
    let a = s.accept(&m.id, 20).unwrap();
    assert_eq!((a.status, a.updated_at), (MemoryStatus::Active, 20));
    let again = s.accept(&m.id, 99).unwrap();
    assert_eq!(again.updated_at, 20, "accepting twice changes nothing");
    let found = s.retrieve("which tea do I like?", 5).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, m.id);
}

#[test]
fn denying_a_proposal_leaves_nothing_behind() {
    let s = store();
    let m = s.propose("Secret plan: buy a boat", None, 1).unwrap();
    s.delete(&m.id).unwrap();
    assert!(s.list(None).unwrap().is_empty());
    assert!(s.retrieve("boat", 5).unwrap().is_empty());
    assert_eq!(s.accept(&m.id, 2), Err(StoreError::NotFound));
}

#[test]
fn deleted_text_is_gone_from_the_index_retrieval_and_the_file() {
    // The spec's acceptance test (§13 M5): deleted text is absent from FTS and retrieval.
    let s = store();
    let keep = s.propose("Likes hiking in autumn", None, 1).unwrap();
    let gone = s.propose("Zxqwvbn is the codeword", None, 2).unwrap();
    s.accept(&keep.id, 3).unwrap();
    s.accept(&gone.id, 3).unwrap();
    assert_eq!(s.retrieve("zxqwvbn", 5).unwrap().len(), 1);
    s.delete(&gone.id).unwrap();
    assert!(s.retrieve("zxqwvbn", 5).unwrap().is_empty());
    let conn = s.conn();
    let in_index: i64 = conn
        .query_row(
            "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'zxqwvbn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(in_index, 0);
    let in_table: i64 = conn
        .query_row(
            "SELECT count(*) FROM memories WHERE content LIKE '%zxqwvbn%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(in_table, 0);
    drop(conn);
    assert_eq!(
        s.retrieve("hiking", 5).unwrap().len(),
        1,
        "the other row stays"
    );
}

#[test]
fn editing_updates_the_text_the_index_and_keeps_the_status() {
    let s = store();
    let m = s.propose("Lives in Pune", None, 1).unwrap();
    let a = s.accept(&m.id, 2).unwrap();
    let e = s.edit(&a.id, "Lives in Mumbai", 3).unwrap();
    assert_eq!(
        (e.content.as_str(), e.status, e.updated_at),
        ("Lives in Mumbai", MemoryStatus::Active, 3)
    );
    assert!(
        s.retrieve("pune", 5).unwrap().is_empty(),
        "old text is gone from the index"
    );
    assert_eq!(s.retrieve("mumbai", 5).unwrap().len(), 1);
    assert_eq!(s.edit("nope", "x1", 4), Err(StoreError::NotFound));
}

#[test]
fn text_is_cleaned_masked_and_limited() {
    let s = store();
    let m = s.propose("  Likes\n  tea\u{7} a lot  ", None, 1).unwrap();
    assert_eq!(m.content, "Likes tea a lot");
    // A key-shaped secret is masked before it is stored.
    let k = s
        .propose("My key is sk-ant-api03-abcdefghijklmnop", None, 1)
        .unwrap();
    assert!(!k.content.contains("abcdefghijklmnop"), "{}", k.content);
    for bad in ["", "   \n\t ", &"x".repeat(MAX_MEMORY_CHARS + 1)] {
        assert!(matches!(
            s.propose(bad, None, 1),
            Err(StoreError::Invalid(_))
        ));
    }
    // Exactly the limit is fine, counted in characters not bytes.
    let max = "é".repeat(MAX_MEMORY_CHARS);
    assert!(s.propose(&max, None, 1).is_ok());
}

#[test]
fn the_same_text_is_one_memory() {
    let s = store();
    let a = s.propose("Owns a bike", None, 1).unwrap();
    let b = s.propose("Owns   a bike", None, 2).unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(s.list(None).unwrap().len(), 1);
}

#[test]
fn list_is_newest_first_and_filters_by_status() {
    let s = store();
    let a = s.propose("first one", None, 1).unwrap();
    let b = s.propose("second one", None, 2).unwrap();
    s.accept(&a.id, 3).unwrap();
    let all: Vec<_> = s.list(None).unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(all, vec![a.id.clone(), b.id.clone()], "a changed last");
    let active = s.list(Some(MemoryStatus::Active)).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, a.id);
}

#[test]
fn retrieval_is_ranked_limited_and_safe_against_odd_input() {
    let s = store();
    for (i, t) in [
        "Likes tea",
        "Likes tea and biscuits and more tea",
        "Likes coffee",
        "Plays chess on Sundays",
    ]
    .iter()
    .enumerate()
    {
        let m = s.propose(t, None, i as u64 + 1).unwrap();
        s.accept(&m.id, i as u64 + 10).unwrap();
    }
    let two = s.retrieve("tea", 2).unwrap();
    assert_eq!(two.len(), 2);
    assert!(two.iter().all(|m| m.content.contains("tea")));
    // FTS syntax characters in the question never break the query.
    for q in [
        "\"",
        "tea AND",
        "NEAR(",
        "tea* OR (",
        "a",
        "",
        "!!!",
        "' OR 1=1 --",
    ] {
        assert!(s.retrieve(q, 5).is_ok(), "{q:?}");
    }
    assert!(s.retrieve("!!!", 5).unwrap().is_empty());
    assert_eq!(s.retrieve("sundays chess", 5).unwrap().len(), 1);
}

#[test]
fn archived_memories_are_not_retrieved() {
    let s = store();
    let m = s.propose("Old fact about llamas", None, 1).unwrap();
    s.accept(&m.id, 2).unwrap();
    s.conn()
        .execute(
            "UPDATE memories SET status = 'archived' WHERE id = ?1",
            [&m.id],
        )
        .unwrap();
    assert!(s.retrieve("llamas", 5).unwrap().is_empty());
    assert_eq!(s.list(Some(MemoryStatus::Archived)).unwrap().len(), 1);
}
