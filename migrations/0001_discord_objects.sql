CREATE TABLE discord_objects (
    kind   TEXT NOT NULL CHECK (kind IN ('role', 'category', 'channel', 'message', 'forum_post')),
    key    TEXT NOT NULL,
    id     TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('created', 'adopted')),
    PRIMARY KEY (kind, key)
) STRICT;
