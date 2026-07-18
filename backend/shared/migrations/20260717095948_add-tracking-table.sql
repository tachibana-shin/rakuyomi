-- Add migration script here
CREATE TABLE IF NOT EXISTS manga_tracking (
    source_id TEXT NOT NULL,
    manga_id TEXT NOT NULL,
    service TEXT NOT NULL,
    remote_media_id INTEGER NOT NULL,
    remote_title TEXT NOT NULL,
    remote_url TEXT NULL,
    total_chapters INTEGER NULL,
    total_volumes INTEGER NULL,
    last_synced_progress INTEGER NULL,
    last_synced_at INTEGER NULL,
    PRIMARY KEY (source_id, manga_id, service)
) STRICT;
