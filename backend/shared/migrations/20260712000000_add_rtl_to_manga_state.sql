-- Add viewer override column to manga_state for per-manga viewer preference.
-- NULL means "use the default from manga_details".
ALTER TABLE manga_state ADD COLUMN viewer INTEGER NULL;
