-- Add migration script here
CREATE TABLE last_check_update (
    source_id TEXT NOT NULL,
    manga_id TEXT NOT NULL,
    last_check INTEGER NOT NULL,
    next_ts_arima INTEGER NOT NULL,
    PRIMARY KEY (source_id, manga_id)
) STRICT;

CREATE TABLE notifications (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL,
    manga_id        TEXT NOT NULL,
    chapter_id      TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    is_read         INTEGER NOT NULL DEFAULT 0
);
