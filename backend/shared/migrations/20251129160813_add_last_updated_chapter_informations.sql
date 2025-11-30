-- Add migration script here
ALTER TABLE chapter_informations
ADD COLUMN last_updated INTEGER NULL;
