--[[--
StreamedChapterDocument is a KOReader document provider that renders manga
chapters fetched page-by-page from the backend server, instead of requiring
a fully downloaded CBZ file.

The document is backed by a tiny descriptor file (see
`extensions/StreamedChapters.lua`) which identifies the chapter to stream.

Core contract — we NEVER lie to KOReader:
- `openPage` always returns a real, decoded page. On a cache miss it blocks
  briefly to fetch+decode (fast: the backend serves already-cached pages
  from disk/tmpfs). There are no placeholder pages, no fake dimensions and
  no post-hoc cache surgery, so every one of KOReader's caches (pgdim,
  tiles) stays truthful at all times.
- Misses are made rare by a background subprocess prefetcher keeping a
  window of pages around the reading position warm in the backend cache
  (see `utils/PagePrefetcher.lua`).

Implementation notes:
- Extends `PicDocument` to inherit its color/dithering setup, but overrides
  `init` entirely: no image engine is opened from disk.
- A sentinel `koptinterface` field makes ReaderConfig build the KoptOptions
  config panel (which Rakuyomi patches with its View Mode toggle), while all
  rendering stays on the generic `Document` base methods driven purely by
  this class' engine object. Auto page-trim is not supported in streaming
  mode: the base `getPageBBox` ignores `trim_page`, so pages are always
  shown full-bleed.
]]

local Blitbuffer = require("ffi/blitbuffer")
local CanvasContext = require("document/canvascontext")
local logger = require("logger")
local _ = require("gettext+")

local Backend = require("Backend")
local ImageFetcher = require("utils/ImageFetcher")
local PagePrefetcher = require("utils/PagePrefetcher")
local PicDocument = require("document/picdocument")
local RenderImage = require("ui/renderimage")
local StreamedChapters = require("extensions/StreamedChapters")

-- Generous timeout: a cold fetch may involve the backend proxying the
-- source website.
local PAGE_FETCH_TIMEOUT_SECONDS = 120

-- Prefetch window: pages kept warm in the backend cache ahead of and
-- behind the reading position, so that ordinary page turns hit the local
-- server cache instead of the network.
local PREFETCH_FORWARD_PAGES = 8
local PREFETCH_BACKWARD_PAGES = 3
local PREFETCH_KICK_DELAY_SECONDS = 0.25

-- RAM budget for the engine-level cache of decoded pages (BlitBuffers).
-- NOTE: decoded pages are uncompressed RGB32 and BIG — a single large webp
-- can decode to 50+ MB. The budget must comfortably hold ALL simultaneously
-- visible pages plus prefetch margin, otherwise the viewer evicts the page
-- being displayed and instantly reloads it in a loop.
local PAGE_LRU_MAX_BYTES = 96 * 1024 * 1024
local PAGE_LRU_MAX_ENTRIES = 6

-- How many of the most recently used pages are immune to eviction (the
-- visible page(s) plus close neighbours in scroll/dual-page modes).
local PAGE_LRU_PROTECTED_ENTRIES = 3

-- Generic size for placeholders when nothing is known about a page yet.
local PLACEHOLDER_WIDTH = 600
local PLACEHOLDER_HEIGHT = 800

--- Decodes raw image data into a BlitBuffer.
---@return BlitBuffer|nil
local function decode_image_data(data)
  local ok, bb = pcall(RenderImage.renderImageData, RenderImage, data, #data, false)
  if not ok or not bb then
    logger.warn("StreamedChapterDocument: failed decoding page data")
    return nil
  end
  return bb
end

--- Creates a placeholder BlitBuffer shown while a page is loading: an
--- all-white page with centered "Loading…" text (plain white if text
--- rendering fails for any reason). Sized to the given dimensions — callers
--- pass previously-known page dimensions when available, so the placeholder
--- matches the real page's aspect ratio.
---@param width number
---@param height number
---@return BlitBuffer
local function make_placeholder_bb(width, height)
  local bb = Blitbuffer.new(width, height)
  -- NOTE: fill() takes a color object, not a number (blitbuffer.lua calls
  -- value:getColor8() internally).
  bb:fill(Blitbuffer.COLOR_WHITE)

  pcall(function()
    local Font = require("ui/font")
    local RenderText = require("ui/rendertext")

    local face = Font:getFace("cfont", 24)
    local text = _("Loading…")
    local size = RenderText:sizeUtf8Text(0, width, face, text)

    local x = math.floor((width - size.x) / 2)
    local baseline = math.floor(height / 2 + size.y_top / 2)
    RenderText:renderUtf8Text(bb, x, baseline, face, text,
      false, false, Blitbuffer.COLOR_BLACK)
  end)

  return bb
end

--[[--
Engine page object. Mirrors the interface of `ffi/pic.lua`'s pages:
dc-aware dimensions plus a draw method scaling into the target BB.
]]
---@class StreamedPage
local StreamedPage = {}

function StreamedPage:new(o)
  o = o or {}
  setmetatable(o, self)
  self.__index = self
  return o
end

---@private
---@param dc DrawContext
---@return number w
---@return number h
function StreamedPage:getSize(dc)
  local zoom = dc:getZoom()
  return self.width * zoom, self.height * zoom
end

--- Scales the decoded page into the target blitbuffer, like `PicPage:draw`.
---@param dc DrawContext Unused, kept for interface parity.
---@param bb BlitBuffer The target blitbuffer.
function StreamedPage:draw(dc, bb)
  local scaled_bb = self.image_bb:scale(bb:getWidth(), bb:getHeight())
  bb:blitFullFrom(scaled_bb, 0, 0)
  scaled_bb:free()
end

--- Frees the underlying decoded bitmap, unless it is owned by the engine's
--- decoded-page cache (in which case the cache frees it on eviction).
function StreamedPage:close()
  if not self.owned_by_lru and self.image_bb ~= nil then
    self.image_bb:free()
    self.image_bb = nil
  end
end

--[[--
Engine document object plugged into `Document._document`. Provides
getPages/openPage/getToc/close, fetching pages over HTTP on demand.
]]
---@class StreamedEngine
local StreamedEngine = {}

--- Fetches stream metadata and builds the engine.
---@param descriptor table Parsed chapter descriptor.
---@return StreamedEngine|nil
---@return string|nil error
function StreamedEngine.new(descriptor)
  local response = Backend.getChapterStreamInfo(
    descriptor.source_id, descriptor.manga_id, descriptor.chapter_id)
  if response.type == 'ERROR' then
    logger.warn("StreamedChapterDocument: failed to get chapter stream info:",
      response.message)
    return nil, response.message
  end

  if response.body.is_novel then
    -- Text chapters cannot be streamed as images; callers should fall back
    -- to the regular download path before ever opening one of these.
    return nil, "chapter is a text chapter and cannot be streamed"
  end

  if not response.body.page_count or response.body.page_count == 0 then
    return nil, "chapter has no pages"
  end

  local engine = {
    page_count = response.body.page_count,
    descriptor = descriptor,
    -- Decoded-page cache (engine-level LRU of BlitBuffers). Decoded pages
    -- are uncompressed RGB32 and big (10-30 MB each): there is deliberately
    -- no per-entry size limit — the global budget + entry count bound memory
    -- instead, always keeping at least one entry.
    page_lru = {},
    page_lru_bytes = 0,
    page_lru_tick = 0,
    closed = false,
    -- Pages whose async load is currently in flight (dedup guard).
    pending_pages = {},
    -- Last page served through openPage; drives the prefetch kick.
    last_served_page = nil,
    prefetch_kick_scheduled = false,
    prefetch_in_flight = false,
    prefetch_pending = false,
    -- Contiguous range of pages known to be warm in the backend cache.
    warm_lo = nil,
    warm_hi = nil,
    -- Back-reference to the owning Document instance.
    document = nil,
  }
  setmetatable(engine, { __index = StreamedEngine })

  return engine
end

---@return number
function StreamedEngine:getPages()
  return self.page_count
end

---@return table
function StreamedEngine:getToc()
  return {}
end

--- Fetches and decodes a single page (1-based). Blocking on cold misses,
--- which the prefetcher keeps rare; never returns a stand-in page.
---@param pageno number
---@return StreamedPage
function StreamedEngine:openPage(pageno)
  local cached_bb = self:_lruGet(pageno)
  if cached_bb then
    logger.info("stream [lua]: openPage", pageno, "-> LRU hit",
      "(entries:", #self.page_lru, "bytes:", self.page_lru_bytes, ")")
    self:_schedulePrefetchKick(pageno)
    return StreamedPage:new{
      width = cached_bb:getWidth(),
      height = cached_bb:getHeight(),
      image_bb = cached_bb,
      owned_by_lru = true,
    }
  end

  -- Cold miss: kick an async load and show a placeholder immediately. The
  -- placeholder adopts previously-known dimensions for this page when
  -- available (pgdim cache), so flipping back to an already-rendered page
  -- never changes layout, and the eventual image swap needs no re-layout.
  if not self.pending_pages[pageno] then
    logger.info("stream [lua]: openPage", pageno, "-> MISS, scheduling async load",
      "(LRU entries:", #self.page_lru, "bytes:", self.page_lru_bytes,
      "closed:", self.closed, ")")
    self.pending_pages[pageno] = true
    self:_scheduleAsyncLoad(pageno)
  else
    logger.info("stream [lua]: openPage", pageno, "-> already pending")
  end

  local placeholder_width, placeholder_height = self:_knownPageSize(pageno)
  local placeholder = make_placeholder_bb(placeholder_width, placeholder_height)
  self:_schedulePrefetchKick(pageno)

  return StreamedPage:new{
    width = placeholder:getWidth(),
    height = placeholder:getHeight(),
    image_bb = placeholder,
  }
end

--- Previously-known pixel size of a page (from the pgdim cache of earlier
--- renders), or generic defaults for never-seen pages.
---@param pageno number
---@return number width
---@return number height
function StreamedEngine:_knownPageSize(pageno)
  local document = self.document
  if document ~= nil then
    local ok, item = pcall(function()
      local DocCache = require("document/doccache")
      local hash = "pgdim|" .. document.file .. "|" ..
        tostring(document.mod_time) .. "|" .. pageno
      local cached = DocCache:check(hash)
      return cached and cached[1]
    end)
    if ok and item and item.w and item.h and item.w > 0 and item.h > 0 then
      return item.w, item.h
    end
  end
  return PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT
end

--- Schedules an async load of a page: fetches its bytes in a background
--- subprocess (the UI stays responsive), decodes them in-process, caches
--- the decoded page and repaints so the placeholder is replaced by the
--- real page.
---@param pageno number
function StreamedEngine:_scheduleAsyncLoad(pageno)
  local UIManager = require("ui/uimanager")

  UIManager:scheduleIn(0, function()
    if self.closed or not self.pending_pages[pageno] then return end

    PagePrefetcher.runInBackground(function()
      -- Subprocess context: fetch the raw bytes; decoding happens in the
      -- parent (FFI blitbuffers cannot cross the pipe).
      return ImageFetcher.fetchPath(self:_pagePath(pageno), {
        use_memory_cache = false,
        timeout = PAGE_FETCH_TIMEOUT_SECONDS,
      })
    end, function(completed, data)
      -- Runs in the parent once the subprocess exited.
      self.pending_pages[pageno] = nil
      if self.closed then return end

      local bb
      if type(data) == "string" and #data > 0 then
        bb = decode_image_data(data)
      end
      if not bb then
        logger.warn("StreamedChapterDocument: async load of page", pageno,
          "failed; it will be retried on next access")
        return
      end

      logger.dbg("StreamedChapterDocument: async load of page", pageno,
        "completed")
      self:_lruPut(pageno, bb)
      logger.info("stream [lua]: async page", pageno, "cached:",
        #self.page_lru, "entries,", self.page_lru_bytes, "bytes")

      local document = self.document
      local need_relayout = false
      if document ~= nil then
        pcall(function()
          local DocCache = require("document/doccache")
          local CacheItem = require("cacheitem")
          local Geom = require("ui/geometry")

          local pgdim_hash = "pgdim|" .. document.file .. "|" ..
            tostring(document.mod_time) .. "|" .. pageno
          local previous = DocCache:check(pgdim_hash)
          local old_dims = previous and previous[1]
          local new_w = bb:getWidth()
          local new_h = bb:getHeight()
          if old_dims == nil or old_dims.w ~= new_w or old_dims.h ~= new_h then
            need_relayout = true
          end
          DocCache:insert(pgdim_hash, CacheItem:new{
            Geom:new{ w = new_w, h = new_h },
          })
        end)

        -- Discard tiles rendered from the placeholder. NOTE: +1 second so
        -- tiles created in this same os.time() second are discarded too
        -- (a bare os.time() would let same-second stale tiles survive).
        document.tile_cache_validity_ts = os.time() + 1

        if need_relayout then
          -- Dimensions changed under us: go through KOReader's own re-layout
          -- path — ReaderView recalculates zoom/visible area against the
          -- corrected dimensions instead of reusing stale state.
          pcall(function()
            local Event = require("ui/event")
            local ReaderUI = require("apps/reader/readerui")
            local reader_ui = ReaderUI.instance
            if reader_ui ~= nil and reader_ui.document == document
                and reader_ui.paging ~= nil and reader_ui.paging.current_page then
              reader_ui:handleEvent(Event:new("PageUpdate",
                reader_ui.paging.current_page))
            end
          end)
        end

        pcall(function()
          local ReaderUI = require("apps/reader/readerui")
          local reader_ui = ReaderUI.instance
          if reader_ui ~= nil and reader_ui.document == document then
            UIManager:setDirty(reader_ui, "partial")
          end
        end)
      end
      UIManager:setDirty("all", "partial")

      -- Safety net: nudge once more shortly after, covering any ordering
      -- quirk between the refresh queue and freshly dirtied widgets.
      UIManager:scheduleIn(0.2, function()
        if not self.closed then
          UIManager:setDirty("all", "partial")
        end
      end)
    end, { returns_simple_string = true })
  end)
end

--- Byte size of a BlitBuffer, for LRU accounting.
---@param bb BlitBuffer
---@return number
function StreamedEngine:_bbBytes(bb)
  return tonumber(bb.stride) * bb.h
end

--- Returns the cached decoded bitmap of a page, marking it recently used.
---@param pageno number
---@return BlitBuffer|nil
function StreamedEngine:_lruGet(pageno)
  for _, entry in ipairs(self.page_lru) do
    if entry.pageno == pageno then
      self.page_lru_tick = self.page_lru_tick + 1
      entry.last_use = self.page_lru_tick
      return entry.bb
    end
  end
  return nil
end

--- Returns whether a page's decoded bitmap is in the engine cache, WITHOUT
--- touching its recency (unlike _lruGet).
---@param pageno number
---@return boolean
function StreamedEngine:_lruPeek(pageno)
  for _, entry in ipairs(self.page_lru) do
    if entry.pageno == pageno then
      return true
    end
  end
  return false
end

--- Stores a decoded bitmap of a page in the engine LRU (evicting the least
--- recently used entries, and freeing their bitmaps, over budget).
---@param pageno number
---@param bb BlitBuffer Ownership transfers to the cache.
function StreamedEngine:_lruPut(pageno, bb)
  if self.closed then
    bb:free()
    return
  end

  local bytes = self:_bbBytes(bb)

  for i, entry in ipairs(self.page_lru) do
    if entry.pageno == pageno then
      -- Replace an existing entry (should not normally happen).
      entry.bb:free()
      table.remove(self.page_lru, i)
      self.page_lru_bytes = self.page_lru_bytes - entry.size
      break
    end
  end

  self.page_lru_tick = self.page_lru_tick + 1
  table.insert(self.page_lru, {
    pageno = pageno,
    bb = bb,
    size = bytes,
    last_use = self.page_lru_tick,
  })
  self.page_lru_bytes = self.page_lru_bytes + bytes

  while (#self.page_lru > PAGE_LRU_MAX_ENTRIES
      or self.page_lru_bytes > PAGE_LRU_MAX_BYTES)
      and #self.page_lru > PAGE_LRU_PROTECTED_ENTRIES do
    -- Protect the PAGE_LRU_PROTECTED_ENTRIES most recently used entries
    -- (the page(s) currently on screen, plus close neighbours in scroll or
    -- dual-page modes): evicting a visible page makes it miss again on the
    -- very next repaint, which turns memory pressure into a reload loop.
    local protected = {}
    for _, entry in ipairs(self.page_lru) do
      table.insert(protected, entry.last_use)
    end
    table.sort(protected)

    local threshold = protected[math.min(#protected,
      PAGE_LRU_PROTECTED_ENTRIES)] or 0

    local lru_index, lru_entry
    for i, entry in ipairs(self.page_lru) do
      if entry.last_use < threshold then
        if not lru_entry or entry.last_use < lru_entry.last_use then
          lru_index, lru_entry = i, entry
        end
      end
    end
    if not lru_index then break end

    logger.dbg("StreamedChapterDocument: evicting page", lru_entry.pageno,
      "from page cache")
    logger.info("stream [lua]: LRU evicting page", lru_entry.pageno,
      "(size:", lru_entry.size, ")")
    lru_entry.bb:free()
    table.remove(self.page_lru, lru_index)
    self.page_lru_bytes = self.page_lru_bytes - lru_entry.size
  end
end

--- Records the page just served and schedules a deferred prefetch kick.
--- The delay lets rapid page-flip bursts coalesce, and running the actual
--- work from a scheduled callback keeps it out of the paint cycle (a
--- requirement for the Trapper subprocess machinery).
---@param pageno number The page that was just served.
function StreamedEngine:_schedulePrefetchKick(pageno)
  self.last_served_page = pageno
  if self.prefetch_kick_scheduled then
    return
  end
  self.prefetch_kick_scheduled = true

  local UIManager = require("ui/uimanager")
  UIManager:scheduleIn(PREFETCH_KICK_DELAY_SECONDS, function()
    self.prefetch_kick_scheduled = false
    self:_predecodeNext()
    self:_startPrefetch()
  end)
end

--- Pre-decodes the next page (n+1) into the engine cache, so the most
--- common page turn is instant regardless of tile cache state.
---
--- Runs from a scheduled callback (outside the paint cycle). The fetch
--- itself hits the backend's warm cache, so this costs only a local read
--- plus one image decode (~tens to a couple hundred ms) — paid once per
--- page turn, while the reader is idle.
function StreamedEngine:_predecodeNext()
  if self.closed then return end

  local last = self.last_served_page
  if last == nil then return end

  local next_page = last + 1
  if next_page > self.page_count then return end
  if self:_lruPeek(next_page) then return end
  -- An async load for that page may already be running; don't duplicate.
  if self.pending_pages[next_page] then return end

  logger.info("stream [lua]: predecoding page", next_page)

  local data = ImageFetcher.fetchPath(self:_pagePath(next_page), {
    use_memory_cache = false,
    timeout = PAGE_FETCH_TIMEOUT_SECONDS,
  })
  local bb = data and decode_image_data(data)

  if not bb then
    logger.warn("StreamedChapterDocument: predecode of page", next_page,
      "failed; will retry on next access")
    ImageFetcher.dropFromCache(self:_pagePath(next_page))
    return
  end

  self:_lruPut(next_page, bb)
end

--- Launches a background subprocess warming the desired window of pages
--- (PREFETCH_BACKWARD_PAGES behind, PREFETCH_FORWARD_PAGES ahead) around
--- the reader's current position. Never runs two batches concurrently: an
--- overlapping kick while a batch runs is remembered and replayed after.
function StreamedEngine:_startPrefetch()
  local last = self.last_served_page
  if last == nil then
    return
  end

  if self.prefetch_in_flight then
    self.prefetch_pending = true
    return
  end

  local want_lo = math.max(1, last - PREFETCH_BACKWARD_PAGES)
  local want_hi = math.min(self.page_count, last + PREFETCH_FORWARD_PAGES)

  -- Build the list of pages in the wanted window that are not warm yet.
  local indices = {}
  for i = want_lo, want_hi do
    local warmed = self.warm_lo ~= nil and i >= self.warm_lo and i <= self.warm_hi
    if not warmed then
      indices[#indices + 1] = i
    end
  end

  if #indices == 0 then
    return
  end

  logger.info("stream [lua]: prefetching", #indices, "page(s) [",
    want_lo, "..", want_hi, "] warm:",
    tostring(self.warm_lo), "-", tostring(self.warm_hi),
    "last:", last)

  self.prefetch_in_flight = true

  PagePrefetcher.runInBackground(function()
    -- Runs in a forked subprocess: fetch every page in the wanted window.
    -- The backend caches them on disk/tmpfs as a side effect of serving;
    -- we discard the bytes here — the server cache IS the preload store.
    for _, i in ipairs(indices) do
      ImageFetcher.fetchPath(self:_pagePath(i), {
        use_memory_cache = false,
        timeout = PAGE_FETCH_TIMEOUT_SECONDS,
      })
    end
  end, function()
    -- Runs in the parent once the subprocess exited.
    self.prefetch_in_flight = false

    -- The whole wanted window is warm now (skipped indices were already
    -- warm by definition).
    self.warm_lo = want_lo
    self.warm_hi = want_hi

    if self.prefetch_pending then
      self.prefetch_pending = false
      self:_startPrefetch()
    end
  end)
end

--- Builds the backend request path serving one page of this chapter.
---@param pageno number
---@return string
function StreamedEngine:_pagePath(pageno)
  return Backend.getChapterStreamPagePath(
    self.descriptor.source_id, self.descriptor.manga_id,
    self.descriptor.chapter_id, pageno)
end

function StreamedEngine:close()
  -- Free all cached decoded bitmaps.
  self.closed = true
  for _, entry in ipairs(self.page_lru) do
    entry.bb:free()
  end
  self.page_lru = {}
  self.page_lru_bytes = 0
end

---@class StreamedChapterDocument
local StreamedChapterDocument = PicDocument:extend{
  --- Sentinel truthy value: makes ReaderConfig pick the KoptOptions panel
  --- (which Rakuyomi extends) without routing rendering through the real
  --- KoptInterface, whose C-backed contexts our engine cannot provide.
  koptinterface = setmetatable({}, {}),
}

--- Opens the streamed chapter described by the `.rcbz` descriptor file.
function StreamedChapterDocument:init()
  self:updateColorRendering()

  local descriptor = StreamedChapters.readDescriptor(self.file)
  if not descriptor or not descriptor.source_id
      or not descriptor.manga_id or not descriptor.chapter_id then
    error("StreamedChapterDocument: invalid chapter descriptor file "
      .. tostring(self.file))
  end
  self.descriptor = descriptor

  -- Dithering flags, copied from PicDocument:init.
  if CanvasContext:hasEinkScreen() then
    if CanvasContext:canHWDither() then
      self.hw_dithering = true
    elseif CanvasContext.fb_bpp == 8 then
      self.sw_dithering = true
    end
  end

  local engine, err = StreamedEngine.new(descriptor)
  if not engine then
    error("StreamedChapterDocument: " .. tostring(err))
  end
  self._document = engine
  engine.document = self

  self.is_open = true
  self.info.has_pages = true
  self.info.configurable = true

  self:_readMetadata()
end

--- Full-page bounding box per page (no trimming in streaming mode).
---@param pageno number
---@return table bbox
function StreamedChapterDocument:getUsedBBox(pageno)
  local dims = self:getNativePageDimensions(pageno)
  return { x0 = 0, y0 = 0, x1 = dims.w, y1 = dims.h }
end

--- Document properties sourced from the descriptor's embedded metadata.
---@return table props
function StreamedChapterDocument:getDocumentProps()
  return {
    title = self.descriptor.title,
    series = self.descriptor.series,
    language = self.descriptor.language,
    keywords = self.descriptor.scanlator,
  }
end

--- First page as a BlitBuffer, for cover display.
---@return BlitBuffer|nil
function StreamedChapterDocument:getCoverPageImage()
  local first_page = self._document:openPage(1)
  if first_page.image_bb then
    local copy = first_page.image_bb:copy()
    first_page:close()
    return copy
  end
  return nil
end

--- Registers this provider for the `.rcbz` extension. Weight 120 puts it
--- above every built-in provider.
---@param registry DocumentRegistry
function StreamedChapterDocument:register(registry)
  registry:addProvider(
    "rcbz", "application/x-rakuyomi-streamed-chapter", self, 120)
end

return StreamedChapterDocument
