CREATE TABLE relay_events (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL UNIQUE,
    event      BLOB NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;
