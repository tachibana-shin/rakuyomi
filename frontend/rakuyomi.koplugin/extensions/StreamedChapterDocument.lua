--[[--
StreamedChapterDocument is a KOReader document provider that renders manga
chapters fetched page-by-page from the backend server, instead of requiring
a fully downloaded CBZ file.

The document is backed by a tiny descriptor file (see
`extensions/StreamedChapters.lua`) which identifies the chapter to stream.
Pages are fetched through the backend (`GET .../stream/pages/{n}`), decoded
in RAM and drawn exactly like KOReader's own picture documents do, so all of
ReaderUI's paging machinery (zoom, pan, RTL, tile caching) works unchanged.

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

local Backend = require("Backend")
local ImageFetcher = require("utils/ImageFetcher")
local PicDocument = require("document/picdocument")
local RenderImage = require("ui/renderimage")
local StreamedChapters = require("extensions/StreamedChapters")

-- Generous timeout: the first fetch of a page may involve the backend
-- proxying the source website.
local PAGE_FETCH_TIMEOUT_SECONDS = 120

-- Size of the placeholder page shown when a page cannot be fetched or
-- decoded. Keeps the reading session alive instead of crashing mid-render.
local PLACEHOLDER_WIDTH = 600
local PLACEHOLDER_HEIGHT = 800

--- Creates an all-white BlitBuffer used as a fallback page.
---@return BlitBuffer
local function make_placeholder_bb()
  local bb = Blitbuffer.new(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT)
  bb:fill(0xFF) -- white
  return bb
end

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

--[[--
Engine page object. Mirrors the interface of `ffi/pic.lua`'s pages:
getSize/dc-aware dimensions plus a draw method scaling into the target BB.
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

--- Frees the underlying decoded bitmap.
function StreamedPage:close()
  if self.image_bb ~= nil then
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

--- Fetches and decodes a single page (1-based).
---@param pageno number
---@return StreamedPage
function StreamedEngine:openPage(pageno)
  local path = Backend.getChapterStreamPagePath(
    self.descriptor.source_id, self.descriptor.manga_id,
    self.descriptor.chapter_id, pageno)

  local opts = { timeout = PAGE_FETCH_TIMEOUT_SECONDS }
  local data = ImageFetcher.fetchPath(path, opts)
  local bb = data and decode_image_data(data)

  if not bb then
    -- Either the fetch failed or the cached bytes were corrupted: drop any
    -- cached entry and try once more with fresh data.
    logger.warn("StreamedChapterDocument: page", pageno,
      "failed to load, retrying without cache")
    ImageFetcher.dropFromCache(path)

    data = ImageFetcher.fetchPath(path, opts)
    bb = data and decode_image_data(data)
  end

  if not bb then
    logger.warn("StreamedChapterDocument: showing placeholder for page", pageno)
    bb = make_placeholder_bb()
  end

  return StreamedPage:new{
    width = bb:getWidth(),
    height = bb:getHeight(),
    image_bb = bb,
  }
end

function StreamedEngine:close()
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
    return first_page.image_bb:copy()
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
