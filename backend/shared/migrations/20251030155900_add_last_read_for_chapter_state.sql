-- Add migration script here
ALTER TABLE chapter_state
ADD COLUMN last_read INTEGER NULL;
