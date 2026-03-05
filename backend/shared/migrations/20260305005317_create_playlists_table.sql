-- Create playlists table
CREATE TABLE playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL
) STRICT;

-- Create playlist_mangas table to link mangas to playlists
CREATE TABLE playlist_mangas (
    playlist_id INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    manga_id TEXT NOT NULL,
    PRIMARY KEY (playlist_id, source_id, manga_id),
    FOREIGN KEY (playlist_id) REFERENCES playlists (id) ON DELETE CASCADE
) STRICT;
