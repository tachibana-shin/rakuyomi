-- Add migration script here
ALTER TABLE chapter_informations
ADD COLUMN thumbnail TEXT NULL;
ALTER TABLE chapter_informations
ADD COLUMN lang TEXT NULL;
