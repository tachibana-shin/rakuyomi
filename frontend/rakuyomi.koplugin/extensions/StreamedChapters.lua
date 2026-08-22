--[[--
StreamedChapters manages "descriptor" files for streamed chapters.

A descriptor is a tiny JSON file named `<hash>.rcbz` living under
`<home>/stream/`. It identifies the chapter to stream and carries just
enough metadata for KOReader's document properties:

    {
      "source_id": "...",
      "manga_id": "...",
      "chapter_id": "...",
      "title": "Chapter 12",
      "series": "Some Manga"
    }

The extension is registered as a document provider by
`extensions/StreamedChapterDocument.lua`, which reads the descriptor and
serves pages fetched on demand from the backend server.

Descriptors are intentionally kept on disk forever: they are tiny, and
KOReader stores reading progress in `<file>.sdr/` next to them, which would
be lost if we deleted them.
]]

local lfs = require("libs/libkoreader-lfs")
local logger = require("logger")
local rapidjson = require("rapidjson")
local md5 = require("ffi/sha2").md5

local Paths = require("Paths")

--- @class StreamedChapters
local StreamedChapters = {}

--- @return string -- The directory holding descriptor files.
function StreamedChapters.descriptorsDirectory()
  return Paths.getHomeDirectory() .. "/stream"
end

--- Computes the deterministic descriptor file path for a chapter.
--- @param source_id string
--- @param manga_id string
--- @param chapter_id string
--- @return string
function StreamedChapters.descriptorPath(source_id, manga_id, chapter_id)
  local hash = md5(source_id .. "\0" .. manga_id .. "\0" .. chapter_id)
  return StreamedChapters.descriptorsDirectory() .. "/" .. hash .. ".rcbz"
end

--- Creates (or overwrites) the descriptor file for a chapter.
--- @param manga Manga The manga the chapter belongs to.
--- @param chapter Chapter The chapter to create a descriptor for.
--- @return string|nil path The descriptor path, or nil on failure.
--- @return string|nil error
function StreamedChapters.createDescriptor(manga, chapter)
  local directory = StreamedChapters.descriptorsDirectory()
  lfs.mkdir(directory)

  local path = StreamedChapters.descriptorPath(manga.source.id, manga.id, chapter.id)
  local payload = {
    source_id = chapter.source_id,
    manga_id = chapter.manga_id,
    chapter_id = chapter.id,
    title = chapter.title,
    series = manga.title,
    scanlator = chapter.scanlator,
    language = chapter.lang,
  }

  local encoded, err = rapidjson.encode(payload)
  if not encoded then
    logger.err("StreamedChapters: failed to encode descriptor:", err)
    return nil, err
  end

  local file, io_err = io.open(path, "w")
  if not file then
    logger.err("StreamedChapters: failed to open", path, "for writing:", io_err)
    return nil, tostring(io_err)
  end
  file:write(encoded)
  file:close()

  return path
end

--- Reads a descriptor file.
--- @param path string The descriptor path.
--- @return table|nil descriptor The parsed descriptor, or nil on failure.
function StreamedChapters.readDescriptor(path)
  local file = io.open(path, "r")
  if not file then
    return nil
  end

  local content = file:read("*a")
  file:close()

  local ok, decoded = pcall(rapidjson.decode, content)
  if not ok or type(decoded) ~= "table" then
    logger.warn("StreamedChapters: failed to parse descriptor at", path)
    return nil
  end

  return decoded
end

return StreamedChapters
