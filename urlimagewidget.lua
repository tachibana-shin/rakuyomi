--[[--
UrlImageWidget shows an image downloaded from an http(s) URL.

The image data is kept fully in RAM (it is never written to disk), decoded
via RenderImage, and displayed through an inner ImageWidget.

Show image from URL example:

    local Trapper = require("ui/trapper")
    Trapper:wrap(function()
        UIManager:show(UrlImageWidget:new{
            url = "https://example.com/image.png",
            -- Optional, to fit the image into a specific area:
            -- width = 400, height = 300,
        })
    end)

Or, shorter (does the Trapper wrapping for you):

    local UrlImageWidget = require("ui/widget/urlimagewidget")
    UrlImageWidget.view{ url = "https://example.com/image.png" }

When wrapped in Trapper, a dismissible loading message is shown while
downloading, and tapping it cancels the download.
Without Trapper, the download is simply blocking, with no progress UI.

A small LRU memory cache keeps recently downloaded (still compressed) image
data around, so re-showing the same URL does not hit the network again.
This cache lives in RAM only, and dies with the app.

Notes:
- Animated GIF/WebP are displayed as their first frame only
  (use ImageViewer for animations).
- Checking network availability (NetworkMgr) is the caller's job.
]]

local Geom = require("ui/geometry")
local ImageWidget = require("ui/widget/imagewidget")
local InfoMessage = require("ui/widget/infomessage")
local RenderImage = require("ui/renderimage")
local Screen = require("device").screen
local Trapper = require("ui/trapper")
local UIManager = require("ui/uimanager")
local Widget = require("ui/widget/widget")
local logger = require("logger")
local _ = require("gettext")
local T = require("ffi/util").template

local http = require("socket.http")
local https = require("ssl.https")
local socket = require("socket")
local socketutil = require("socketutil")

local CANCELLED_CODE = "cancelled"

-- In-memory LRU cache of downloaded (still compressed) image data, keyed by URL.
-- RAM-only by design: nothing ever touches the disk.
local MEM_CACHE_MAX_SIZE = 32 * 1024 * 1024 -- 32 MiB of compressed image data
local mem_cache = {}          -- url -> {data=string, size=number, last_use=number}
local mem_cache_size = 0      -- sum of cached sizes
local mem_cache_tick = 0      -- increasing counter, to track least-recently-used

local function cacheDrop(url)
    local entry = mem_cache[url]
    if entry then
        mem_cache_size = mem_cache_size - entry.size
        mem_cache[url] = nil
    end
end

local function cacheGet(url)
    local entry = mem_cache[url]
    if entry then
        mem_cache_tick = mem_cache_tick + 1
        entry.last_use = mem_cache_tick
        return entry.data
    end
end

local function cachePut(url, data)
    if not data then return end
    local size = #data
    if size > MEM_CACHE_MAX_SIZE then return end -- too big to be worth caching
    cacheDrop(url) -- in case we're replacing an existing entry
    mem_cache_tick = mem_cache_tick + 1
    mem_cache[url] = { data = data, size = size, last_use = mem_cache_tick }
    mem_cache_size = mem_cache_size + size
    while mem_cache_size > MEM_CACHE_MAX_SIZE do
        -- Evict the least recently used entry
        local lru_url, lru_entry
        for u, e in pairs(mem_cache) do
            if not lru_entry or e.last_use < lru_entry.last_use then
                lru_url, lru_entry = u, e
            end
        end
        if not lru_url then break end
        cacheDrop(lru_url)
    end
end

--- Downloads url into a string, fully in RAM.
-- on_progress(), when provided, is called at most once per second;
-- returning false aborts the download (used to implement tap-to-cancel).
-- @treturn data string, or nil,err
local function fetchImageData(url, on_progress)
    if not url:find("^https?://") then
        return nil, _("unsupported protocol")
    end
    local requester = url:find("^https://") and https or http
    local chunks = {}
    local cancelled = false
    local last_report_ts = os.time()
    -- Custom LTN12 sink: accumulates chunks, honors cancellation.
    -- Returning nil aborts the whole transfer (same trick as socketutil.table_sink).
    local sink = function(chunk, err)
        if chunk then
            table.insert(chunks, chunk)
            if on_progress and os.time() - last_report_ts >= 1 then
                last_report_ts = os.time()
                if not on_progress() then
                    cancelled = true
                end
            end
        end
        if cancelled then
            return nil, CANCELLED_CODE
        end
        return 1
    end
    socketutil:set_timeout(socketutil.FILE_BLOCK_TIMEOUT, socketutil.FILE_TOTAL_TIMEOUT)
    local code, headers, status = socket.skip(1, requester.request{
        url     = url,
        headers = { ["Accept-Encoding"] = "identity" },
        sink    = sink,
    })
    socketutil:reset_timeout()
    if cancelled then
        return nil, CANCELLED_CODE
    end
    if code == socketutil.TIMEOUT_CODE
        or code == socketutil.SSL_HANDSHAKE_CODE
        or code == socketutil.SINK_TIMEOUT_CODE then
        return nil, code
    end
    if code ~= 200 then
        logger.dbg("UrlImageWidget: request failed:", status or code)
        logger.dbg("UrlImageWidget: response headers:", headers)
        return nil, tostring(status or code)
    end
    return table.concat(chunks)
end

--- Decodes image data (as fetched above) into a BlitBuffer.
-- @treturn BlitBuffer, or nil on failure
local function decodeImageData(data)
    local ok, bb = pcall(RenderImage.renderImageData, RenderImage, data, #data, false)
    if not ok or not bb then
        logger.warn("UrlImageWidget: failed decoding image data")
        return nil
    end
    return bb
end

local UrlImageWidget = Widget:extend{
    -- URL of the image to download and display (required)
    url = nil,

    -- Sizing hints, passed to the inner ImageWidget:
    -- with the default scale_factor=0, the image is scaled to best fit
    -- the width/height box, keeping aspect ratio.
    -- Defaults to fitting the screen when none are provided.
    width = nil,
    height = nil,
    scale_factor = 0,
    alpha = false, -- set to true if the image has a transparent background
    scale_for_dpi = false,

    -- Local file to display instead if fetching or decoding failed
    fallback_file = nil,

    -- Whether to use the in-memory cache of downloaded data
    use_memory_cache = true,

    -- Text shown while downloading (as a dismissible Trapper info message)
    loading_text = _("Downloading image…"),

    -- Callbacks:
    -- on_success(): called once the image is ready to be painted
    on_success = nil,
    -- on_error(err): called on failure. When neither on_error nor
    -- fallback_file is provided, an InfoMessage shows the error.
    on_error = nil,

    -- Inner widget actually doing the painting (ImageWidget)
    _content = nil,
}

function UrlImageWidget:init()
    if not self.url then
        error("UrlImageWidget: 'url' is required")
    end
    if not self.width and not self.height then
        -- Default to fitting the screen (meaningful with default scale_factor=0)
        self.width = Screen:getWidth()
        self.height = Screen:getHeight()
    end

    local data = self.use_memory_cache and cacheGet(self.url) or nil
    local err
    if not data then
        -- Show a dismissible loading message (no-op UI-wise when not wrapped
        -- in Trapper). It returns false if the user tapped it to cancel.
        if Trapper:info(self.loading_text) then
            data, err = fetchImageData(self.url, function()
                -- Keep the message displayed, catch dismissal (tap to cancel),
                -- allow the UI to breathe (~100ms) between progress updates.
                return Trapper:info(nil, true)
            end)
        else
            err = CANCELLED_CODE
        end
        Trapper:clear()
    end

    if data then
        local bb = decodeImageData(data)
        if bb then
            if self.use_memory_cache then
                cachePut(self.url, data)
            end
            self._content = ImageWidget:new{
                image = bb,
                image_disposable = true,
                width = self.width,
                height = self.height,
                scale_factor = self.scale_factor,
                alpha = self.alpha,
                scale_for_dpi = self.scale_for_dpi,
            }
            if self.on_success then
                self.on_success(self)
            end
            return
        else
            -- Don't keep corrupted data around
            cacheDrop(self.url)
            err = _("decoding failed")
        end
    end

    -- Failure paths
    if err == CANCELLED_CODE then
        logger.dbg("UrlImageWidget: cancelled by user")
        return -- cancelled on purpose: no error message
    end
    err = err or _("unknown error")
    logger.warn("UrlImageWidget: failed to load", self.url, ":", err)
    if self.fallback_file then
        self._content = ImageWidget:new{
            file = self.fallback_file,
            width = self.width,
            height = self.height,
            scale_factor = self.scale_factor,
            alpha = self.alpha,
            scale_for_dpi = self.scale_for_dpi,
        }
    elseif self.on_error then
        self.on_error(self, err)
    else
        UIManager:show(InfoMessage:new{
            text = T(_("Could not load image:\n%1"), tostring(err)),
            timeout = 5,
        })
    end
end

function UrlImageWidget:getSize()
    if self._content then
        return self._content:getSize()
    end
    return Geom:new{ w = 0, h = 0 }
end

function UrlImageWidget:paintTo(bb, x, y)
    if not self._content then return end
    local size = self:getSize()
    self.dimen = Geom:new{ x = x, y = y, w = size.w, h = size.h }
    self._content:paintTo(bb, x, y)
end

function UrlImageWidget:free()
    if self._content then
        self._content:free()
    end
end

--- Fetches image data for url, going through the memory cache.
-- Class method, usable without instantiating the widget.
-- @treturn data string, or nil,err
function UrlImageWidget:fetchData(url)
    local data = cacheGet(url)
    if data then
        return data
    end
    local err
    data, err = fetchImageData(url)
    if data then
        cachePut(url, data)
    end
    return data, err
end

--- Convenience helper: wraps everything in Trapper and shows the widget.
-- Usage: UrlImageWidget.view{ url = "https://..." }
function UrlImageWidget.view(opts)
    local widget
    Trapper:wrap(function()
        widget = UrlImageWidget:new(opts)
        UIManager:show(widget)
    end)
    return widget
end

return UrlImageWidget
