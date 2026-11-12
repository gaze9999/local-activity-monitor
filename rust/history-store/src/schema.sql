CREATE TABLE history_state (
    namespace TEXT PRIMARY KEY, metadata TEXT NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE history_items (
    namespace TEXT NOT NULL, section TEXT NOT NULL, item_key TEXT NOT NULL,
    ordinal INTEGER NOT NULL, event_time TEXT, payload TEXT NOT NULL,
    server, operation, statement, nested,
    PRIMARY KEY(namespace, section, item_key)
);
CREATE INDEX history_time ON history_items(namespace, section, event_time);
CREATE INDEX history_recent ON history_items(namespace, section, event_time DESC, item_key DESC);
CREATE INDEX history_cleanup ON history_items(event_time);
CREATE INDEX history_aggregate ON history_items(namespace, section, event_time, server, operation, statement, nested);

CREATE TABLE session_cursors (
    source_key TEXT PRIMARY KEY, offset INTEGER NOT NULL, signature TEXT NOT NULL, payload TEXT NOT NULL
);
CREATE TABLE session_calls (
    source_key TEXT NOT NULL, call_id TEXT NOT NULL, event_time TEXT, payload TEXT NOT NULL,
    PRIMARY KEY(source_key, call_id)
);
CREATE INDEX session_calls_recent ON session_calls(source_key, event_time DESC, call_id);
