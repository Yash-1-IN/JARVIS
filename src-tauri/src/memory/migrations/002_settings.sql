-- Schema version 2: the settings table of spec §8.2 (history retention lives here).
CREATE TABLE settings (
    key            TEXT PRIMARY KEY,
    value_json     TEXT NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1
);
