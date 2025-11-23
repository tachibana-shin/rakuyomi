-- Add migration script here
CREATE TABLE IF NOT EXISTS manga_details (
    source_id      TEXT NOT NULL,
    id             TEXT NOT NULL,
    title          TEXT,
    author         TEXT,
    artist         TEXT,
    description    TEXT,
    tags           TEXT,          -- JSON array of strings
    cover_url      TEXT,
    url            TEXT,

    status         INTEGER NOT NULL,  -- PublishingStatus (u8)
    nsfw           INTEGER NOT NULL,  -- MangaContentRating (u8)
    viewer         INTEGER NOT NULL,  -- MangaViewer (u8)

    last_updated   TEXT,  -- ISO8601 timestamp
    last_opened    TEXT,
    date_added     TEXT,

    PRIMARY KEY (source_id, id)
);
