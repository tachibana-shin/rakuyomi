-- Add migration script here
CREATE TABLE chapter_state_new (
    source_id TEXT NOT NULL,
    manga_id TEXT NOT NULL,
    chapter_id TEXT NOT NULL,
    read INTEGER NOT NULL,
    last_read INTEGER NULL,
    PRIMARY KEY (source_id, manga_id, chapter_id)
);

INSERT OR IGNORE INTO chapter_state_new
SELECT * FROM chapter_state;

DROP TABLE chapter_state;

ALTER TABLE chapter_state_new RENAME TO chapter_state;
