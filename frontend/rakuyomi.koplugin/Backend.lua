local logger = require("logger")
local ffiutil = require("ffi/util")
local rapidjson = require("rapidjson")
---@diagnostic disable-next-line: different-requires
local util = require("util")

local Device = require("device")
local Platform = require("Platform")

local device_is_slow = Device:isKobo()

-- Seconds before we log a warning (slow devices) or kill the server (others)
local SERVER_STARTUP_TIMEOUT_SECONDS = tonumber(os.getenv('RAKUYOMI_SERVER_STARTUP_TIMEOUT') or (device_is_slow and 10 or 5))

-- Hard cap for slow devices; only used when device_is_slow
local SERVER_STARTUP_ABSOLUTE_TIMEOUT_SECONDS = tonumber(os.getenv('RAKUYOMI_SERVER_STARTUP_ABSOLUTE_TIMEOUT') or 180)

--- @class Backend
--- @field private server Server
local Backend = {}

local function replaceRapidJsonNullWithNilRecursively(maybeTable)
  if type(maybeTable) ~= "table" then
    return maybeTable
  end

  local t = maybeTable

  for key, value in pairs(t) do
    if value == rapidjson.null then
      t[key] = nil
    else
      t[key] = replaceRapidJsonNullWithNilRecursively(value)
    end
  end

  return t
end

--- @class RequestParameters
--- @field path string The path of the request
--- @field method string? The request method to be used
--- @field body unknown? The request body to be sent. Must be encodable as JSON.
--- @field query_params table<string, string|number|nil>? The query parameters to be sent on request.
--- @field timeout number? The timeout used for this request. If unset, the default value for the platform will be used (usually 60 seconds).

--- @class SuccessfulResponse<T>: { type: 'SUCCESS', body: T }
--- @class ErrorResponse: { type: 'ERROR', status: number, message: string }

--- Performs a HTTP request, using JSON to encode the request body and to decode the response body.
--- @private
--- @param request RequestParameters The parameters used for this request.
--- @generic T: any
--- @nodiscard
--- @return SuccessfulResponse<T>|ErrorResponse # The parsed JSON response or nil, if there was an error.
function Backend.requestJson(request)
  assert(Backend.server ~= nil, "backend wasn't initialized!")
  local url = require("socket.url")

  -- FIXME naming
  local query_params = request.query_params or {}
  local parts = {}
  for name, value in pairs(query_params) do
    table.insert(parts, name .. "=" .. url.escape(value))
  end
  local built_query_params = table.concat(parts, "&")

  local path_and_query = request.path
  if built_query_params ~= "" then
    path_and_query = path_and_query .. "?" .. built_query_params
  end

  local headers = {}
  local serialized_body = nil
  if request.body ~= nil then
    serialized_body = rapidjson.encode(request.body)
    headers["Content-Type"] = "application/json"
    headers["Content-Length"] = tostring(serialized_body:len())
  end

  logger.info('Requesting to', path_and_query)

  local response = Backend.server:request(
    {
      path = path_and_query,
      method = request.method or "GET",
      headers = headers,
      body = serialized_body,
      timeout_seconds = request.timeout,
    }
  )

  if response.type == 'ERROR' then
    return response
  end

  -- Under normal conditions, we should always have a request body, even when the status code
  -- is not 2xx
  local parsed_body, err = rapidjson.decode(response.body)
  if err then
    error("Expected to be able to decode the response body as JSON: " ..
      response.body .. "(status code: " .. response.status .. ")")
  end

  if not (response.status and response.status >= 200 and response.status <= 299) then
    logger.err("Request failed with status code", response.status, "and body", parsed_body)
    local error_message = parsed_body.message
    assert(error_message ~= nil, "Request failed without error message")

    return { type = 'ERROR', status = response.status, message = error_message }
  end

  return { type = 'SUCCESS', body = replaceRapidJsonNullWithNilRecursively(parsed_body) }
end

---@return boolean
local function waitUntilHttpServerIsReady()
  local start_time = os.time()
  local patience_warned = false

  while true do
    local ok, response = pcall(function()
      return Backend.requestJson({
        path = '/health-check',
        timeout = 1,
      })
    end)

    if ok and response.type == 'SUCCESS' then
      return true
    end

    local elapsed = os.time() - start_time
    local deadline = device_is_slow and SERVER_STARTUP_ABSOLUTE_TIMEOUT_SECONDS
                   or SERVER_STARTUP_TIMEOUT_SECONDS

    if elapsed >= deadline then
      return false
    end

    if device_is_slow
      and not patience_warned
      and elapsed >= SERVER_STARTUP_TIMEOUT_SECONDS then
      patience_warned = true
      logger.warn("Backend not ready after", SERVER_STARTUP_TIMEOUT_SECONDS,
        "s; continuing to wait (max", SERVER_STARTUP_ABSOLUTE_TIMEOUT_SECONDS, "s)")
    end

    ffiutil.sleep(1)
  end
end

---@return boolean success Whether the backend was initialized successfully.
---@return string|nil logs On error, the last logs written by the server.
function Backend.initialize()
  assert(Backend.server == nil, "backend was already initialized!")

  Backend.server = Platform:startServer()

  if not waitUntilHttpServerIsReady() then
    local logBuffer = Backend.server:getLogBuffer()
    Backend.server:stop()
    Backend.server = nil

    return false, table.concat(logBuffer, "\n")
  end

  return true, nil
end

---@return boolean, string|nil
local backendInitialized, logs
function Backend.getBackend()
  if backendInitialized and Backend.running() then return true, nil end
  backendInitialized, logs = Backend.initialize()
  if backendInitialized then
    local messages = Backend.drainStartupLog()
    if #messages > 0 then
      local UIManager = require('ui/uimanager')
      local InfoMessage = require('ui/widget/infomessage')

      UIManager:show(InfoMessage:new {
        text = table.concat(messages, "\n\n"),
      })
    end
  end
end

function Backend.getLogs()
  return logs
end

function Backend.getInitialized()
  return backendInitialized
end

--- Drain any startup warnings from the server.
---@return string[] messages
function Backend.drainStartupLog()
  local response = Backend.requestJson({
    path = '/system/startup-log',
    timeout = 5,
  })
  if response.type == 'SUCCESS' then
    ---@diagnostic disable-next-line: undefined-field
    return response.body.messages or {}
  end
  return {}
end

---@return boolean
function Backend.running()
  return Backend.server ~= nil
end

--- @class SourceInformation
--- @field id string The ID of the source.
--- @field name string The name of the source.
--- @field version number The version of the source.
--- @field source_of_source string|nil The domain source load source.

--- @class Manga
--- @field id string The ID of the manga.
--- @field source SourceInformation The source information for this manga.
--- @field title string The title of this manga.
--- @field manga_cover string|nil The cover of this manga.
--- @field unread_chapters_count number|nil The number of unread chapters for this manga, or `nil` if we do not know how many chapters this manga has.
--- @field last_read number|nil The timestamp (in seconds since epoch) of when this manga was last read, or `nil` if we don't know.
--- @field in_library boolean Whether this manga is in the user's library.
--- @field viewer MangaViewer The preferred viewer mode from the source ("DefaultViewer", "Rtl", "Ltr", "Vertical", "Scroll").
--- @field state_viewer boolean The viewer mode set by user?

--- @class Chapter
--- @field id string The ID of this chapter.
--- @field source_id string The ID of the source for this chapter.
--- @field manga_id string The ID of the manga that this chapter belongs to.
--- @field scanlator string? The scanlation group that worked on this chapter.
--- @field chapter_num number? The chapter number.
--- @field volume_num number? The volume that this chapter belongs to, if known.
--- @field read boolean If this chapter was read to its end.
--- @field last_read number? The timestamp (in seconds since epoch) of when this chapter was last read to its end.
--- @field downloaded boolean If this chapter was already downloaded to the storage.
--- @field title string? The title of this chapter, if any.
--- @field locked boolean The locked
--- @field lang string? The language code
--- @field on_tmpfs boolean? The chapter is stored in tmpfs.
--- @field file string? The file path of the chapter (only use in frontend).

--- @class SourceMangaSearchResults
--- @field source_information SourceInformation Information about the source that generated those results.
--- @field mangas Manga[] Found mangas.

--- @class FileSummary
--- @field filenames string[] The names
--- @field total_size number The total size
--- @field total_text string The total size text format kb, mb...

--- @class CpuInfo
--- @field model string CPU model name.
--- @field cores number Number of CPU cores.
--- @field usage_percent number Overall CPU usage percentage.

--- @class MemoryInfo
--- @field total_bytes number Total physical memory in bytes.
--- @field available_bytes number Available memory in bytes.
--- @field used_bytes number Used memory in bytes.

--- @class FilesystemInfo
--- @field path string Mount point path.
--- @field total_bytes number Total size in bytes.
--- @field used_bytes number Used size in bytes.
--- @field free_bytes number Free size in bytes.

--- @class ProcessInfo
--- @field memory_rss_bytes number Resident set size of the server process in bytes.
--- @field memory_virtual_bytes number Virtual memory size of the server process in bytes.

--- @class SystemStats
--- @field cpu CpuInfo CPU information.
--- @field memory MemoryInfo System memory information.
--- @field tmpfs FilesystemInfo|nil Tmpfs filesystem information, if available.
--- @field tmpfs_mount_error string|nil Error message if tmpfs mount failed.
--- @field storage FilesystemInfo Data storage filesystem information.
--- @field process ProcessInfo Server process memory usage.

--- Publishing status of a manga.
---
--- @enum PublishingStatus
PublishingStatus = {
  Unknown      = 'Unknown',       -- Status cannot be determined from the source
  Ongoing      = 'Ongoing',       -- Still releasing new chapters
  Completed    = 'Completed',     -- Fully published and finished
  Cancelled    = 'Cancelled',     -- Publication ended prematurely
  Hiatus       = 'Hiatus',        -- Temporarily stopped by author/publisher
  NotPublished = 'Not Published', -- Announced but not yet started
}

--- Content rating for a manga, used to decide filtering and NSFW handling.
---
--- @enum MangaContentRating
MangaContentRating = {
  Safe       = 'Safe',       -- No adult content
  Suggestive = 'Suggestive', -- Mildly sexual themes or suggestive content
  Nsfw       = 'NSFW',       -- Explicit adult content
}

--- Preferred reading mode for a manga.
--- This determines how pages should be displayed in the reader UI.
---
--- @enum MangaViewer
MangaViewer = {
  DefaultViewer = 0, -- Use the source's default or the app's default setting
  Rtl           = 1, -- Right-to-left page navigation (typical for Japanese manga)
  Ltr           = 2, -- Left-to-right navigation (Western comics / manhwa translations)
  Vertical      = 3, -- Vertical strip reading (webtoons, long-strip manga)
  Scroll        = 4, -- Free scrolling mode (continuous scroll)
}
--- Reverse map: numeric viewer id -> name string.
local MangaViewerName = {}
for name, id in pairs(MangaViewer) do
  MangaViewerName[id] = name
end
Backend.MangaViewerName = MangaViewerName

--- Represents a manga entry returned by a source or stored locally.
--- This table contains all metadata used to describe a manga.
---
--- @class MManga
--- @field source_id string                -- Unique ID of the source (e.g. "mangadex", "asurascans")
--- @field id string                       -- Manga ID inside the source. Usually short.
--- @field title string|nil                -- Manga title, may be missing if source doesn't provide it
--- @field author string|nil               -- Name of the author
--- @field artist string|nil               -- Name of the artist
--- @field description string|nil          -- Summary or description text
--- @field tags string[]|nil               -- List of genre / tags (e.g. {"Action", "Romance"})
--- @field cover_url string|nil            -- URL to the cover image
--- @field url string|nil                  -- URL to the manga page on the source website
---
--- @field status PublishingStatus         -- Current publication status (e.g. ONGOING, COMPLETED)
--- @field nsfw MangaContentRating         -- NSFW rating (e.g. SAFE, SUGGESTIVE, EXPLICIT)
--- @field viewer MangaViewer              -- Suggested reading mode (e.g. paged, long-strip)
---
--- @field last_updated string|nil         -- Timestamp of latest metadata update (ISO8601)
--- @field last_opened string|nil          -- When the user last opened this manga
--- @field last_read string|nil            -- When the user last read a chapter
--- @field date_added string|nil           -- When this manga was added to the library
---
--- Notes:
--- - Some fields are optional because many sources do not provide full metadata.
--- - All timestamps are expected to be ISO8601 strings.
--- - `status`, `nsfw`, and `viewer` are enums defined elsewhere in the system.


--- Lists mangas added to the user's library.
--- @return SuccessfulResponse<Manga[]>|ErrorResponse
function Backend.getMangasInLibrary()
  return Backend.requestJson({
    path = "/library",
  })
end

--- @class StorageStats
--- @field total_bytes number The total number of bytes used by downloaded chapters.

--- Gets the total size of downloaded chapters. Served from an in-memory
--- counter on the backend, so this is cheap to call.
--- @return SuccessfulResponse<StorageStats>|ErrorResponse
function Backend.getStorageStats()
  return Backend.requestJson({
    path = "/storage-stats",
  })
end

--- Lists path files invalidate
--- @param modeInvalid boolean
--- @return SuccessfulResponse<FileSummary>|ErrorResponse
function Backend.findOrphanOrReadFiles(modeInvalid)
  return Backend.requestJson({
    path = "/find-orphan-or-read-files",
    query_params = { invalid = modeInvalid and "true" or "false" }
  })
end

--- Delete file
--- @param filename string The name of the file to delete.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.removeFile(filename)
  return Backend.requestJson({
    path = "/delete-file",
    body = filename,
    method = "POST"
  })
end

--- Get system resource statistics.
--- @return SuccessfulResponse<SystemStats>|ErrorResponse
function Backend.getSystemStats()
  return Backend.requestJson({
    path = "/system/stats",
    method = "GET"
  })
end

--- Sync database
--- @param accept_migrate_local boolean Flag if true allow migrate database local from WebDAV
--- @param accept_replace_remote boolean Flag if true allow replace database remote from local
--- @return SuccessfulResponse<string>|ErrorResponse
function Backend.syncDatabase(accept_migrate_local, accept_replace_remote)
  return Backend.requestJson({
    path = "/sync-database",
    body = { accept_migrate_local, accept_replace_remote },
    method = "POST"
  })
end

--- Adds a manga to the user's library.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.addMangaToLibrary(source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/add-to-library",
    method = "POST"
  })
end

--- Removes a manga from the user's library.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.removeMangaFromLibrary(source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/remove-from-library",
    method = "POST"
  })
end

--- @class SearchError
--- @field source_id string
--- @field reason string

--- Searches manga from the manga sources.
--- @param cancel_id number
--- @search_text string
--- @param exclude string[]|nil
--- @param page number|nil
--- @return SuccessfulResponse<[Manga[], SearchError[], boolean]>|ErrorResponse
function Backend.searchMangas(cancel_id, search_text, exclude, page)
  return Backend.requestJson({
    path = "/mangas",
    query_params = {
      q = search_text,
      exclude = exclude and table.concat(exclude, ",") or nil,
      cancel_id = cancel_id,
      page = page,
    }
  })
end

--- Lists chapters from a given manga that are already cached into the database.
--- @return SuccessfulResponse<Chapter[]>|ErrorResponse
function Backend.listCachedChapters(source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters",
  })
end

--- Refreshes the chapters of a given manga on the database.
--- @param cancel_id number
--- @param source_id string
--- @param manga_id string
--- @return SuccessfulResponse<{}>|ErrorResponse
function Backend.refreshChapters(cancel_id, source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/refresh-chapters",
    method = "POST",
    body = cancel_id,
  })
end

--- Gets the cached details of a given manga from the database.
--- @return SuccessfulResponse<[MManga, number]>|ErrorResponse
function Backend.cachedMangaDetails(cancel_id, source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/details",
    query_params = { cancel_id = cancel_id }
  })
end

--- Refreshes the details of a given manga on the database.
--- @param cancel_id number
--- @param source_id string
--- @param manga_id string
--- @return SuccessfulResponse<{}>|ErrorResponse
function Backend.refreshMangaDetails(cancel_id, source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/refresh-details",
    method = "POST",
    body = cancel_id,
  })
end

--- Refreshes the details of a given manga on the database.
--- @return SuccessfulResponse<number|nil>|ErrorResponse
function Backend.markChaptersAsRead(source_id, manga_id, range, state)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/mark-as-read",
    method = "POST",
    body = {
      range = range,
      state = state
    }
  })
end

--- Begins downloading all chapters from a given manga to the storage.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.downloadAllChapters(source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters/download-all",
    method = "POST",
  })
end

--- @alias DownloadAllChaptersProgress { type: 'INITIALIZING' }|{ type: 'PROGRESSING', downloaded: number, total: number }|{ type: 'FINISHED' }|{ type: 'CANCELLED' }

--- Checks the status of a "download all chapters" operation.
--- @return SuccessfulResponse<DownloadAllChaptersProgress>|ErrorResponse
function Backend.getDownloadAllChaptersProgress(source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" ..
        source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters/download-all-progress",
  })
end

--- Requests cancellation of a "download all chapters" operation. This can only be called
--- when the operation status is `PROGRESSING`.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.cancelDownloadAllChapters(source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" ..
        source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters/cancel-download-all",
    method = "POST",
  })
end

--- @class DownloadError
--- @field page_index number
--- @field url string
--- @field reason string
--- @field attempts number

--- Downloads the given chapter to the storage.
--- @param cancel_id number
--- @param source_id string
--- @param manga_id string
--- @param chapter_id string
--- @param chapter_num string
--- @return SuccessfulResponse<string>|ErrorResponse
function Backend.downloadChapter(cancel_id, source_id, manga_id, chapter_id, chapter_num)
  local query_params = {}

  if chapter_num ~= nil then
    query_params.chapter_num = chapter_num
  end

  return Backend.requestJson({
    path = "/mangas/" ..
        source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters/" .. util.urlEncode(chapter_id) .. "/download",
    query_params = query_params,
    body = cancel_id,
    method = "POST",
  })
end

--- @param source_id string
--- @param manga_id string
--- @param chapter_id string
--- @param use_ram boolean|nil
--- @return SuccessfulResponse<boolean>|ErrorResponse
function Backend.revokeChapter(source_id, manga_id, chapter_id, use_ram)
  local query_params = {}

  if use_ram ~= nil then
    query_params.use_ram = use_ram and "true" or "false"
  end

  return Backend.requestJson({
    path = "/mangas/" ..
        source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters/" .. util.urlEncode(chapter_id) .. "/revoke",
    query_params = query_params,
    method = "POST",
  })
end

--- Updates the last read position for the chapter.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.updateLastReadChapter(source_id, manga_id, chapter_id)
  return Backend.requestJson({
    path = "/mangas/" ..
        source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters/" .. util.urlEncode(chapter_id) .. "/update-last-read",
    method = "POST",
  })
end

--- Marks the chapter as read.
--- @param value boolean|nil
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.markChapterAsRead(source_id, manga_id, chapter_id, value)
  return Backend.requestJson({
    path = "/mangas/" ..
        source_id .. "/" .. util.urlEncode(manga_id) .. "/chapters/" .. util.urlEncode(chapter_id) .. "/mark-as-read",
    body = { state = value },
    method = "POST",
  })
end

--- Lists information about the installed sources.
--- @return SuccessfulResponse<SourceInformation[]>|ErrorResponse
function Backend.listInstalledSources()
  return Backend.requestJson({
    path = "/installed-sources",
  })
end

--- Lists information about sources available via our source lists.
--- @return SuccessfulResponse<SourceInformation[]>|ErrorResponse
function Backend.listAvailableSources()
  return Backend.requestJson({
    path = "/available-sources",
  })
end

--- Installs a source.
--- @return SuccessfulResponse<SourceInformation[]>|ErrorResponse
function Backend.installSource(source_id, source_of_source)
  return Backend.requestJson({
    path = "/available-sources/" .. source_id .. "/install",
    method = "POST",
    body = source_of_source,
  })
end

--- Uninstalls a source.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.uninstallSource(source_id)
  return Backend.requestJson({
    path = "/installed-sources/" .. source_id,
    method = "DELETE",
  })
end

--- @class GroupSettingDefinition: { type: 'group', title: string|nil, items: SettingDefinition[], footer: string|nil }
--- @class SwitchSettingDefinition: { type: 'switch', title: string, key: string, default: boolean }
--- @class SelectSettingDefinition: { type: 'select', title: string, key: string, values: string[], titles: string[]|nil, default: string  }
--- @class MultiSelectSettingDefinition: { type: 'multi-select', title: string, key: string, values: string[], titles: string[]|nil, default: string[]  }
--- @class LoginSettingDefinition: { type: 'login', title: string, key: string, values: string[], titles: string[]|nil, default: string[]  }
--- @class ButtonSettingDefinition: { type: 'button', title: string, key: string, confirmTitle: string|nil, confirmMessage: string|nil  }
--- @class EditableListSettingDefinition: { type: 'editable-list', title: string, key: string, values: string[], titles: string[]|nil, default: string[]  }
--- @class TextSettingDefinition: { type: 'text', placeholder: string|nil, key: string, default: string|nil }
--- @class LinkSettingDefinition: { type: 'link', title: string, url: string }

--- @alias SettingDefinition GroupSettingDefinition|SwitchSettingDefinition|SelectSettingDefinition|MultiSelectSettingDefinition|LoginSettingDefinition|ButtonSettingDefinition|EditableListSettingDefinition|TextSettingDefinition|LinkSettingDefinition

--- Lists the setting definitions for a given source.
--- @return SuccessfulResponse<SettingDefinition[]>|ErrorResponse
function Backend.getSourceSettingDefinitions(source_id)
  return Backend.requestJson({
    path = "/installed-sources/" .. source_id .. "/setting-definitions",
  })
end

--- Finds the stored settings for a given source.
--- @return SuccessfulResponse<table<string, string|boolean>>|ErrorResponse
function Backend.getSourceStoredSettings(source_id)
  return Backend.requestJson({
    path = "/installed-sources/" .. source_id .. "/stored-settings",
  })
end

function Backend.setSourceStoredSettings(source_id, stored_settings)
  return Backend.requestJson({
    path = "/installed-sources/" .. source_id .. "/stored-settings",
    method = 'POST',
    body = stored_settings,
  })
end

--- Gets the preferred scanlator for a manga.
--- @return SuccessfulResponse<string|nil>|ErrorResponse
function Backend.getPreferredScanlator(source_id, manga_id)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/preferred-scanlator",
    method = "GET"
  })
end

--- Sets the preferred scanlator for a manga.
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.setPreferredScanlator(source_id, manga_id, preferred_scanlator)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/preferred-scanlator",
    method = "POST",
    body = {
      preferred_scanlator = preferred_scanlator
    }
  })
end

--- Sets the viewer override for a manga.
---@param source_id string
---@param manga_id string
---@param viewer MangaViewer
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.setViewer(source_id, manga_id, viewer)
  return Backend.requestJson({
    path = "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id) .. "/viewer",
    method = "POST",
    body = {
      viewer = viewer
    }
  })
end

--- @alias ChapterSortingMode 'chapter_ascending'|'chapter_descending'
--- @alias LibraryViewMode 'base' | 'cover' | 'grid'
--- @alias SearchViewMode 'base' | 'cover' | 'grid'
--- @alias ChapterTitleFormat 'title' | 'series_title' | 'series_chapter_number'
--- @class Settings: { chapter_sorting_mode: ChapterSortingMode, preload_chapters: number, library_view_mode: LibraryViewMode, search_view_mode: SearchViewMode, chapter_title_format: ChapterTitleFormat }

--- Reads the application settings.
--- @return SuccessfulResponse<Settings>|ErrorResponse
function Backend.getSettings()
  return Backend.requestJson({
    path = "/settings"
  })
end

--- Updates the application settings.
--- @return SuccessfulResponse<Settings>|ErrorResponse
function Backend.setSettings(settings)
  return Backend.requestJson({
    path = "/settings",
    method = 'PUT',
    body = settings
  })
end

--- @class TestProxyResponse: { ok: boolean, message: string }
--- Tests whether a proxy URL is reachable.
--- @param proxy_url string
--- @return SuccessfulResponse<TestProxyResponse>|ErrorResponse
function Backend.testProxy(proxy_url)
  return Backend.requestJson({
    path = "/settings/test-proxy",
    method = 'POST',
    body = { proxy_url = proxy_url },
    timeout = 20,
  })
end

--- @class MountTmpFSConfig
--- @field enabled boolean
--- @field ram_storage_size_mb number
--- @param config MountTmpFSConfig
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.mountFS(config)
  return Backend.requestJson({
    path = "/settings/mount-tmpfs",
    method = 'POST',
    body = config
  })
end

--- Creates a new download chapter job. Returns the job's UUID.
--- @return SuccessfulResponse<string>|ErrorResponse
function Backend.createDownloadChapterJob(source_id, manga_id, chapter_id, chapter_num, current_chapter_id)
  return Backend.requestJson({
    path = "/jobs/download-chapter",
    method = 'POST',
    body = {
      source_id = source_id,
      manga_id = manga_id,
      chapter_id = chapter_id,
      chapter_num = chapter_num,
      current_chapter_id = current_chapter_id,
    }
  })
end

--- Creates a new download unread chapters job. Returns the job's UUID.
--- @return SuccessfulResponse<string>|ErrorResponse
function Backend.createDownloadUnreadChaptersJob(source_id, manga_id, amount, langs)
  return Backend.requestJson({
    path = "/jobs/download-unread-chapters",
    method = 'POST',
    body = {
      source_id = source_id,
      manga_id = manga_id,
      amount = amount,
      langs = #langs > 0 and langs or nil,
    }
  })
end

--- Creates a new download scanlator chapters job. Returns the job's UUID.
--- @return SuccessfulResponse<string>|ErrorResponse
function Backend.createDownloadScanlatorChaptersJob(source_id, manga_id, scanlator, amount, langs)
  local body = {
    source_id = source_id,
    manga_id = manga_id,
    scanlator = scanlator,
    amount = amount,
    langs = #langs > 0 and langs or nil,
  }

  return Backend.requestJson({
    path = "/jobs/download-scanlator-chapters",
    method = 'POST',
    body = body
  })
end

--- Creates a new refresh library chapters job. Returns the job's UUID.
--- @return SuccessfulResponse<string>|ErrorResponse
function Backend.refreshLibraryChaptersJob()
  return Backend.requestJson({
    path = "/jobs/refresh-library-chapters",
    method = 'POST',
  })
end

--- Creates a new refresh library details job. Returns the job's UUID.
--- @return SuccessfulResponse<string>|ErrorResponse
function Backend.refreshLibraryDetailsJob()
  return Backend.requestJson({
    path = "/jobs/refresh-library-details",
    method = 'POST',
  })
end

--- @class DownloadProgress: { type: 'INITIALIZING' }|{ type: 'DOWNLOADING', processed: number, total: number }

--- @class PendingJob<T>: { type: 'PENDING', data: T }
--- @class CompletedJob<T>: { type: 'COMPLETED', data: T }
--- @class ErroredJob: { type: 'ERROR', data: ErrorResponse }

--- @alias DownloadChapterJobCompleted [string, DownloadError[], boolean]
--- @alias DownloadChapterJobDetails PendingJob<DownloadProgress|nil>|CompletedJob<DownloadChapterJobCompleted> |ErroredJob

--- Gets details about a job.
--- @return SuccessfulResponse<DownloadChapterJobDetails>|ErrorResponse
function Backend.getJobDetails(id)
  if id == nil then
    return { type = 'ERROR', message = 'job id is nil' }
  end
  return Backend.requestJson({
    path = "/jobs/" .. id,
    method = 'GET'
  })
end

--- Requests for a job to be cancelled.
--- @return SuccessfulResponse<DownloadChapterJobDetails>|ErrorResponse
function Backend.requestJobCancellation(id)
  return Backend.requestJson({
    path = "/jobs/" .. id,
    method = 'DELETE'
  })
end

--- @class UpdateInfo
--- @field public available boolean Whether an update is available
--- @field public current_version string The current version of rakuyomi
--- @field public latest_version string The latest available version
--- @field public release_url string URL to the release page
--- @field public auto_installable boolean Whether the update can be automatically installed

--- Checks if there is an update available for rakuyomi
--- @return SuccessfulResponse<UpdateInfo>|ErrorResponse
function Backend.checkForUpdates()
  return Backend.requestJson({
    path = "/update/check",
    method = "GET"
  })
end

--- Updates the plugin to the given version.
--- @param version string
function Backend.installUpdate(version)
  return Backend.requestJson({
    path = "/update/install",
    method = "POST",
    body = {
      version = version,
    },
    timeout = 120,
  })
end

function Backend.cleanup()
  if Backend.server ~= nil then
    Backend.server:stop()
    Backend.server = nil
    backendInitialized, logs = nil, nil
  end
end

--- @return SuccessfulResponse<number>|ErrorResponse
function Backend.getCountNotification()
  return Backend.requestJson({
    path = "/count-notifications",
    method = 'GET'
  })
end

--- @class MangaId
--- @field source_id string
--- @field manga_id string

--- @class ChapterId
--- @field manga_id MangaId
--- @field chapter_id string

--- @class Notification
--- @field id number
--- @field chapter_id ChapterId
--- @field manga_title string
--- @field manga_cover string|nil
--- @field manga_status number|nil
--- @field chapter_title string|nil
--- @field chapter_number number
--- @field created_at number

--- @return SuccessfulResponse<Notification[]>|ErrorResponse
function Backend.getNotifications()
  return Backend.requestJson({
    path = "/notifications",
    method = 'GET'
  })
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.removeNotification(id)
  return Backend.requestJson({
    path = "/notifications/" .. id,
    method = 'DELETE'
  })
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.clearNotifications()
  return Backend.requestJson({
    path = "/clear-notifications",
    method = 'POST'
  })
end

local idx = 0
function Backend.createCancelId()
  idx = idx + 1
  return idx
end

--- @param cancel_id number
--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.cancel(cancel_id)
  return Backend.requestJson({
    path = "/cancel-request",
    method = 'POST',
    body = cancel_id,
  })
end

--- @return SuccessfulResponse<nil>|ErrorResponse
--- @param cancel_id number
--- @param source_id string
---@param key string
function Backend.handleSourceNotification(cancel_id, source_id, key)
  return Backend.requestJson({
    path         = "/" .. source_id .. "/handle-source-notification/" .. key,
    query_params = { cancel_id = cancel_id },
    method       = 'POST'
  })
end

--- @class Playlist
--- @field id number
--- @field name string

--- @return SuccessfulResponse<Playlist[]>|ErrorResponse
function Backend.getPlaylists()
  return Backend.requestJson({
    path = "/playlists",
  })
end

--- @return SuccessfulResponse<Playlist>|ErrorResponse
function Backend.createPlaylist(name)
  return Backend.requestJson({
    path = "/playlists",
    method = 'POST',
    body = { name = name },
  })
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.deletePlaylist(id)
  return Backend.requestJson({
    path = "/playlists/" .. id,
    method = 'DELETE',
  })
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.renamePlaylist(id, name)
  return Backend.requestJson({
    path = "/playlists/" .. id,
    method = 'PUT',
    body = { name = name },
  })
end

--- @return SuccessfulResponse<Manga[]>|ErrorResponse
function Backend.getMangasInPlaylist(id)
  return Backend.requestJson({
    path = "/playlists/" .. id .. "/mangas",
  })
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.addMangaToPlaylist(playlist_id, source_id, manga_id)
  return Backend.requestJson({
    path = "/playlists/" .. playlist_id .. "/mangas",
    method = 'POST',
    body = {
      source_id = source_id,
      manga_id = manga_id,
    },
  })
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function Backend.removeMangaFromPlaylist(playlist_id, source_id, manga_id)
  return Backend.requestJson({
    path = "/playlists/" .. playlist_id .. "/mangas/" .. source_id .. "/" .. util.urlEncode(manga_id),
    method = 'DELETE',
  })
end

--- @class CookieSyncStatus
--- @field paired boolean
--- @field server_url string|nil
--- @field device_name string|nil
--- @field chat_id number|nil
--- @field cookie_count number

--- @return SuccessfulResponse<CookieSyncStatus>|ErrorResponse
function Backend.getCookieSyncStatus()
  return Backend.requestJson({
    path = "/cookie-sync/status",
    method = "GET",
  })
end

--- @class GenerateCodeResponse
--- @field pairing_code string

--- @return SuccessfulResponse<GenerateCodeResponse>|ErrorResponse
function Backend.generatePairingCode(server_url)
  return Backend.requestJson({
    path = "/cookie-sync/generate-code",
    method = "POST",
    body = { server_url = server_url },
    timeout = 15,
  })
end

--- @class PollPairingResponse
--- @field paired boolean
--- @field chat_id number|nil
--- @field device_name string|nil

--- @return SuccessfulResponse<PollPairingResponse>|ErrorResponse
function Backend.pollPairingStatus(server_url, pairing_code)
  return Backend.requestJson({
    path = "/cookie-sync/poll-pairing",
    method = "POST",
    body = {
      server_url = server_url,
      pairing_code = pairing_code,
    },
    timeout = 15,
  })
end

--- @class SyncResponse
--- @field status string
--- @field domains string[]

--- @return SuccessfulResponse<SyncResponse>|ErrorResponse
function Backend.syncCookies()
  return Backend.requestJson({
    path = "/cookie-sync/sync",
    method = "POST",
    timeout = 30,
  })
end

--- @return SuccessfulResponse<{status: string}>|ErrorResponse
function Backend.unpairDevice()
  return Backend.requestJson({
    path = "/cookie-sync/unpair",
    method = "POST",
  })
end

--- @class ListCookiesResponse
--- @field domains table

--- @return SuccessfulResponse<ListCookiesResponse>|ErrorResponse
function Backend.listCookies()
  return Backend.requestJson({
    path = "/cookie-sync/cookies",
    method = "GET",
  })
end

-- we can't really rely upon Koreader informing us it has terminated because
-- the plugin lifecycle is really obscure, so use the garbage collector to
-- detect we're done and cleanup
if _VERSION == "Lua 5.1" then
  logger.info("setting up __gc proxy")
  ---@diagnostic disable-next-line: deprecated
  local proxy = newproxy(true)
  local proxyMeta = getmetatable(proxy)

  proxyMeta.__gc = function()
    Backend.cleanup()
  end

  rawset(Backend, '__proxy', proxy)
else
  setmetatable(Backend, {
    __gc = function()
      Backend.cleanup()
    end
  })
end

return Backend
