-- Add reading start/completion dates to tracking bindings
ALTER TABLE manga_tracking ADD COLUMN started_at INTEGER NULL;
ALTER TABLE manga_tracking ADD COLUMN completed_at INTEGER NULL;
