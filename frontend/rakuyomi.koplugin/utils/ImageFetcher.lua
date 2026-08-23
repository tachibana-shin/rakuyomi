--[[--
ImageFetcher fetches binary data (images) from the backend server.

Unlike `ui/widget/urlimagewidget`, this module is not tied to a rendering
widget and does not depend on `socket.http`: all requests go through the
backend server transport (`Backend.server:request`), which works on every
platform (Android TCP, generic Unix UDS). The backend serves the actual
bytes, proxying sources when needed (e.g. streamed chapter pages).

A small LRU memory cache keeps recently downloaded data around in RAM only,
so re-fetching the same path does not hit the server again. Nothing touches
the disk from here: disk caching (when it exists) is done server-side.
]]

local logger = require("logger")

local Backend = require("Backend")

local CACHE_MAX_SIZE = 32 * 1024 * 1024 -- 32 MiB of compressed image data
local mem_cache = {}          -- path -> {data=string, size=number, last_use=number}
local mem_cache_size = 0      -- sum of cached sizes
local mem_cache_tick = 0      -- increasing counter, to track least-recently-used

local function cacheDrop(path)
  local entry = mem_cache[path]
  if entry then
    mem_cache_size = mem_cache_size - entry.size
    mem_cache[path] = nil
  end
end

local function cacheGet(path)
  local entry = mem_cache[path]
  if entry then
    mem_cache_tick = mem_cache_tick + 1
    entry.last_use = mem_cache_tick
    return entry.data
  end
end

local function cachePut(path, data)
  local size = #data
  if size > CACHE_MAX_SIZE then return end -- too big to be worth caching
  cacheDrop(path) -- in case we're replacing an existing entry
  mem_cache_tick = mem_cache_tick + 1
  mem_cache[path] = { data = data, size = size, last_use = mem_cache_tick }
  mem_cache_size = mem_cache_size + size
  while mem_cache_size > CACHE_MAX_SIZE do
    -- Evict the least recently used entry
    local lru_path, lru_entry
    for p, e in pairs(mem_cache) do
      if not lru_entry or e.last_use < lru_entry.last_use then
        lru_path, lru_entry = p, e
      end
    end
    if not lru_path then break end
    cacheDrop(lru_path)
  end
end

local ImageFetcher = {}

--- Removes an entry from the memory cache (e.g. after decoding failed,
--- meaning the cached data was corrupted).
--- @param path string The request path the data was fetched for.
function ImageFetcher.dropFromCache(path)
  cacheDrop(path)
end

--- Fetches binary data from the given backend path, going through the RAM
--- cache. This is a blocking call.
--- @param path string The request path (e.g. "/mangas/.../stream/pages/1").
--- @param opts table|nil Additional options:
---   - use_memory_cache (boolean, default true)
---   - timeout (number, seconds; defaults to the platform's value)
--- @nodiscard
--- @return string|nil data The raw bytes, or nil on failure.
--- @return ErrorResponse|nil error The error response, on failure.
function ImageFetcher.fetchPath(path, opts)
  opts = opts or {}

  if opts.use_memory_cache ~= false then
    local cached = cacheGet(path)
    if cached then
      return cached
    end
  end

  local response = Backend.requestRaw({
    path = path,
    method = "GET",
    timeout = opts.timeout,
  })

  if response.type == 'ERROR' then
    logger.dbg("ImageFetcher: request failed for", path, ":", response.message)
    return nil, response
  end

  if response.body == nil or response.body == "" then
    logger.dbg("ImageFetcher: empty body for", path)
    return nil, { type = 'ERROR', message = "empty response" }
  end

  if opts.use_memory_cache ~= false then
    cachePut(path, response.body)
  end

  return response.body
end

return ImageFetcher
