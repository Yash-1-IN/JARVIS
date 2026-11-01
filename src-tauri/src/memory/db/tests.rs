use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jarvis-db-{name}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&d).unwrap();
    d.join("jarvis.db")
}

fn tables(conn: &Connection) -> Vec<String> {
    let mut s = conn
        .prepare("SELECT name FROM sqlite_master WHERE type IN ('table') ORDER BY name")
        .unwrap();
    s.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn a_new_database_has_the_schema_and_fts5_works() {
    let conn = open_in_memory().unwrap();
    let t = tables(&conn);
    for want in ["conversations", "messages", "memories", "memories_fts"] {
        assert!(t.iter().any(|n| n == want), "{want} in {t:?}");
    }
    assert_eq!(schema_version(&conn).unwrap(), MIGRATIONS.len());
    // FTS5 is compiled into the bundled SQLite (spec §8.1: confirm during M5 setup).
    conn.execute(
        "INSERT INTO memories (id, content, status, created_at, updated_at)
         VALUES ('a', 'likes green tea', 'active', 1, 1)",
        [],
    )
    .unwrap();
    let hit: i64 = conn
        .query_row(
            "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'tea'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hit, 1);
}

#[test]
fn a_file_database_uses_wal_secure_delete_and_foreign_keys() {
    let path = tmp("pragmas");
    let o = open(&path, 1).unwrap();
    assert!(o.recovered_from.is_none());
    let mode: String = o
        .conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    let sd: i64 = o
        .conn
        .query_row("PRAGMA secure_delete", [], |r| r.get(0))
        .unwrap();
    assert_ne!(sd, 0);
    let fk: i64 = o
        .conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1);
}

#[test]
fn reopening_keeps_the_data_and_does_not_migrate_twice() {
    let path = tmp("reopen");
    {
        let o = open(&path, 1).unwrap();
        o.conn
            .execute(
                "INSERT INTO memories (id, content, status, created_at, updated_at)
                 VALUES ('a', 'kept', 'active', 1, 1)",
                [],
            )
            .unwrap();
    }
    let o = open(&path, 2).unwrap();
    let n: i64 = o
        .conn
        .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(schema_version(&o.conn).unwrap(), MIGRATIONS.len());
}

#[test]
fn a_failing_migration_rolls_back_and_names_its_version() {
    let mut conn = Connection::open_in_memory().unwrap();
    let ok = "CREATE TABLE one (x INTEGER);";
    let bad = "CREATE TABLE two (y INTEGER); THIS IS NOT SQL;";
    migrate_with(&mut conn, &[ok]).unwrap();
    let e = migrate_with(&mut conn, &[ok, bad]).unwrap_err();
    assert!(matches!(e, DbError::Migration { version: 2, .. }), "{e}");
    // Nothing from the failed step is left, and the version did not move.
    assert_eq!(schema_version(&conn).unwrap(), 1);
    assert!(!tables(&conn).iter().any(|t| t == "two"));
    // The next start with a fixed migration list carries on from version 1.
    migrate_with(&mut conn, &[ok, "CREATE TABLE two (y INTEGER);"]).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 2);
}

#[test]
fn a_database_from_a_newer_build_is_refused_untouched() {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate_with(
        &mut conn,
        &["CREATE TABLE a (x INTEGER);", "CREATE TABLE b (x INTEGER);"],
    )
    .unwrap();
    let e = migrate_with(&mut conn, &["CREATE TABLE a (x INTEGER);"]).unwrap_err();
    assert!(matches!(e, DbError::TooNew { found: 2, known: 1 }), "{e}");
    assert_eq!(schema_version(&conn).unwrap(), 2);
}

#[test]
fn a_corrupt_file_is_backed_up_and_replaced_with_an_empty_database() {
    let path = tmp("corrupt");
    std::fs::write(
        &path,
        b"this is definitely not a sqlite database, just text",
    )
    .unwrap();
    let o = open(&path, 42).unwrap();
    let backup = o.recovered_from.expect("the broken file was moved aside");
    assert!(backup.to_string_lossy().ends_with(".corrupt-42"));
    assert_eq!(
        std::fs::read(&backup).unwrap(),
        b"this is definitely not a sqlite database, just text"
    );
    let n: i64 = o
        .conn
        .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(schema_version(&o.conn).unwrap(), MIGRATIONS.len());
}

#[test]
fn deleting_a_conversation_removes_its_messages() {
    let conn = open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO conversations (id, started_at) VALUES ('c', 1);
         INSERT INTO messages (id, conversation_id, role, content, ts) VALUES ('m', 'c', 'user', 'hi', 1);
         DELETE FROM conversations WHERE id = 'c';",
    )
    .unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    // A message with an unknown role is refused by the schema.
    conn.execute(
        "INSERT INTO conversations (id, started_at) VALUES ('d', 1)",
        [],
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, ts) VALUES ('x', 'd', 'system', 'no', 1)",
            [],
        )
        .is_err()
    );
}
