-- Add migration script here
ALTER TABLE chapter_informations
ADD COLUMN locked INTEGER NOT NULL DEFAULT 0;
