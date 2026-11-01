-- Schema version 1 (spec §8.2). History and memories; the audit log and settings tables
-- come with the steps that use them.

CREATE TABLE conversations (
    id         TEXT PRIMARY KEY,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER,
    title      TEXT
);

CREATE TABLE messages (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'tool')),
    content         TEXT NOT NULL,
    ts              INTEGER NOT NULL,
    tool_call_json  TEXT
);
CREATE INDEX messages_by_conversation ON messages (conversation_id, ts);

CREATE TABLE memories (
    id                TEXT PRIMARY KEY,
    content           TEXT NOT NULL,
    status            TEXT NOT NULL CHECK (status IN ('proposed', 'active', 'archived')),
    source_message_id TEXT,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);
CREATE INDEX memories_by_status ON memories (status, updated_at);

-- Full-text index over the memory text. The text itself lives only in `memories`; the
-- triggers keep the index in step, including on delete.
CREATE VIRTUAL TABLE memories_fts USING fts5(
    content,
    content = 'memories',
    content_rowid = 'rowid'
);

CREATE TRIGGER memories_after_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts (rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TRIGGER memories_after_delete AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts (memories_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
END;

CREATE TRIGGER memories_after_update AFTER UPDATE OF content ON memories BEGIN
    INSERT INTO memories_fts (memories_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
    INSERT INTO memories_fts (rowid, content) VALUES (new.rowid, new.content);
END;
