local Blitbuffer = require("ffi/blitbuffer")
local CenterContainer = require("ui/widget/container/centercontainer")
local Device = require("device")
local Font = require("ui/font")
local FocusManager = require("ui/widget/focusmanager")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local ImageWidget = require("ui/widget/imagewidget")
local ScrollTextWidget = require("ui/widget/scrolltextwidget")
local InputText = require("ui/widget/inputtext")
local TextViewer = require("ui/widget/textviewer")
local LeftContainer = require("ui/widget/container/leftcontainer")
local LineWidget = require("ui/widget/linewidget")
local ProgressWidget = require("ui/widget/progresswidget")
local Size = require("ui/size")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TextWidget = require("ui/widget/textwidget")
local TitleBar = require("ui/widget/titlebar")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local InfoMessage = require("ui/widget/infomessage")
local Trapper = require("ui/trapper")
local _ = require("gettext")
local Screen = Device.screen
local T = require("ffi/util").template

local LoadingDialog = require("LoadingDialog")
local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local calcLastReadText = require("utils/calcLastReadText")

local function parse_iso8601(str)
  local year, month, day, hour, min, sec =
      str:match("(%d+)%-(%d+)%-(%d+)T(%d+):(%d+):(%d+)Z")

  return os.time({
    year = year,
    month = month,
    day = day,
    hour = hour,
    min = min,
    sec = sec,
    isdst = false,
  })
end

--- @class FocusManager
--- @field key_events table<string, any>
--- @field ges_events table<string, any>
--- @field dimen boolean
--- @field close_callback fun() | nil
--- @field new fun(self: FocusManager): FocusManager

--- @class MangaInfoWidget : FocusManager
--- @field padding any
--- @field raw_manga Manga
--- @field manga MManga|nil
--- @field per_read number|nil
--- @field on_return_callback fun()|nil
local MangaInfoWidget = FocusManager:extend {
  padding = Size.padding.fullscreen,
  raw_manga = nil,
  manga = nil,
  per_read = nil,
  on_return_callback = nil,
}

function MangaInfoWidget:init()
  self.updated = nil
  self.layout = {}

  self.small_font_face = Font:getFace("smallffont")
  self.medium_font_face = Font:getFace("ffont")
  self.large_font_face = Font:getFace("largeffont")

  if Device:hasKeys() then
    self.key_events.Close = { { Device.input.group.Back } }
  end
  if Device:isTouchDevice() then
    self.ges_events.Swipe = {
      GestureRange:new {
        ges = "swipe",
        range = function() return self.dimen end,
      }
    }
    self.ges_events.MultiSwipe = {
      GestureRange:new {
        ges = "multiswipe",
        range = function() return self.dimen end,
      }
    }
  end

  local screen_size = Screen:getSize()
  self.covers_fullscreen = true -- hint for UIManager:_repaint()
  self[1] = FrameContainer:new {
    width = screen_size.w,
    height = screen_size.h,
    background = Blitbuffer.COLOR_WHITE,
    bordersize = 0,
    padding = 0,
    self:getStatusContent(screen_size.w, self.manga),
  }

  self.dithered = true
end

--- @param manga MManga
function MangaInfoWidget:getStatusContent(width, manga)
  local title_bar = TitleBar:new {
    width = width,
    bottom_v_padding = 0,
    close_callback = function() self:onClose() end,
    left_icon = "appbar.menu",
    left_icon_tap_callback = function()
      local raw_manga = self.raw_manga
      local on_return_callback = self.on_return_callback

      local onReturnCallback = function()
        self:fetchAndShow(raw_manga, on_return_callback)
      end

      local ChapterListing = require("ChapterListing")
      ChapterListing:fetchAndShow(raw_manga, onReturnCallback, true, true)

      self:onClose(false)
    end,
    show_parent = self,
  }
  local content = VerticalGroup:new {
    align = "left",
    title_bar,
    self:genBookInfoGroup(manga),
    self:genHeader(_("Statistics")),
    self:genStatisticsGroup(width, manga),
    self:genHeader(_("Description")),
    self:genSummaryGroup(width, manga),
    -- self:genHeader(_("Book")),
  }
  return content
end

function MangaInfoWidget:genHeader(title)
  local width, height = Screen:getWidth(), Size.item.height_default

  local header_title = TextWidget:new {
    text = title,
    face = self.medium_font_face,
    fgcolor = Blitbuffer.COLOR_GRAY_9,
  }

  local padding_span = HorizontalSpan:new { width = self.padding }
  local line_width = (width - header_title:getSize().w) / 2 - self.padding * 2
  local line_container = LeftContainer:new {
    dimen = Geom:new { w = line_width, h = height },
    LineWidget:new {
      background = Blitbuffer.COLOR_LIGHT_GRAY,
      dimen = Geom:new {
        w = line_width,
        h = Size.line.thick,
      }
    }
  }
  local span_top, span_bottom
  if Screen:getScreenMode() == "landscape" then
    span_top = VerticalSpan:new { width = Size.span.horizontal_default }
    span_bottom = VerticalSpan:new { width = Size.span.horizontal_default }
  else
    span_top = VerticalSpan:new { width = Size.item.height_default }
    span_bottom = VerticalSpan:new { width = Size.span.vertical_large }
  end

  return VerticalGroup:new {
    span_top,
    HorizontalGroup:new {
      align = "center",
      padding_span,
      line_container,
      padding_span,
      header_title,
      padding_span,
      line_container,
      padding_span,
    },
    span_bottom,
  }
end

--- @param manga MManga
function MangaInfoWidget:genBookInfoGroup(manga)
  local screen_width = Screen:getWidth()
  local split_span_width = math.floor(screen_width * 0.05)

  local img_width, img_height
  if Screen:getScreenMode() == "landscape" then
    img_width = Screen:scaleBySize(132)
    img_height = Screen:scaleBySize(184)
  else
    img_width = Screen:scaleBySize(132 * 1.5)
    img_height = Screen:scaleBySize(184 * 1.5)
  end

  local height = img_height
  local width = screen_width - split_span_width - img_width

  -- Get a chance to have title and authors rendered with alternate
  -- title
  local book_meta_info_group = VerticalGroup:new {
    align = "center",
    VerticalSpan:new { width = height * 0.2 },
    TextBoxWidget:new {
      text = manga.title,
      -- lang = lang,
      width = width,
      face = self.medium_font_face,
      alignment = "center",
    },

  }
  -- author
  if manga.author ~= nil then
    local text_author = TextBoxWidget:new {
      text = manga.author,
      -- lang = lang,
      face = self.small_font_face,
      width = width,
      alignment = "center",
    }
    table.insert(book_meta_info_group,
      CenterContainer:new {
        dimen = Geom:new { w = width, h = text_author:getSize().h },
        text_author
      }
    )
  end
  -- artist
  if manga.artist ~= nil then
    local text_artist = TextBoxWidget:new {
      text = manga.artist,
      -- lang = lang,
      face = self.small_font_face,
      width = width,
      alignment = "center",
    }
    table.insert(book_meta_info_group,
      CenterContainer:new {
        dimen = Geom:new { w = width, h = text_artist:getSize().h },
        text_artist
      }
    )
  end
  -- progress bar
  local read_percentage = self.per_read
  local progress_bar = ProgressWidget:new {
    width = math.floor(width * 0.7),
    height = Screen:scaleBySize(10),
    percentage = read_percentage,
    ticks = nil,
    last = nil,
  }
  table.insert(book_meta_info_group,
    CenterContainer:new {
      dimen = Geom:new { w = width, h = progress_bar:getSize().h },
      progress_bar
    }
  )
  -- complete text
  local text_complete = TextWidget:new {
    text = T(_("%1\xE2\x80\xAF% Completed"), string.format("%1.f", read_percentage * 100)),
    face = self.small_font_face,
  }
  table.insert(book_meta_info_group,
    CenterContainer:new {
      dimen = Geom:new { w = width, h = text_complete:getSize().h },
      text_complete
    }
  )

  -- tags text
  if manga.tags ~= nil and #manga.tags > 0 then
    local tags_text = table.concat(manga.tags, ", ")
    local text_tags = TextBoxWidget:new {
      text = "\n" .. tags_text,
      -- lang = lang,
      face = self.small_font_face,
      width = width,
      alignment = "center",
    }
    table.insert(book_meta_info_group,
      CenterContainer:new {
        dimen = Geom:new { w = width, h = text_tags:getSize().h },
        text_tags
      }
    )
  end

  -- last read text
  if manga.last_read ~= nil then
    local last_read_str = calcLastReadText(parse_iso8601(manga.last_read))
    local text_last_read = TextWidget:new {
      text = T(_("Last read: %1"), last_read_str),
      face = self.small_font_face,
      width = width,
      alignment = "right",
    }
    table.insert(book_meta_info_group,
      CenterContainer:new {
        dimen = Geom:new { w = width, h = text_last_read:getSize().h },
        text_last_read
      }
    )
  end

  -- build the final group
  local book_info_group = HorizontalGroup:new {
    align = "top",
    HorizontalSpan:new { width = split_span_width }
  }
  -- thumbnail
  local thumbnail = manga.url
  -- local cc = ImageLoader:new {
  --   callback = function(content)
  --     thumbnail = RenderImage:fromData(content)
  --     UIManager:setDirty(nil, "ui", nil, true)
  --   end
  -- }
  -- cc.loadImage(manga.cover_url)

  if thumbnail then
    -- Much like BookInfoManager, honor AR here
    -- local cbb_w, cbb_h = thumbnail:getWidth(), thumbnail:getHeight()
    -- if cbb_w > img_width or cbb_h > img_height then
    --   local scale_factor = math.min(img_width / cbb_w, img_height / cbb_h)
    --   cbb_w = math.min(math.floor(cbb_w * scale_factor) + 1, img_width)
    --   cbb_h = math.min(math.floor(cbb_h * scale_factor) + 1, img_height)
    --   thumbnail = RenderImage:scaleBlitBuffer(thumbnail, cbb_w, cbb_h, true)
    -- end
    table.insert(book_info_group, ImageWidget:new {
      file = thumbnail:gsub("^file://", ""),
      width = img_width,
      height = img_height,
    })
  end

  table.insert(book_info_group, CenterContainer:new {
    dimen = Geom:new { w = width, h = height },
    book_meta_info_group,
  })

  return CenterContainer:new {
    dimen = Geom:new { w = screen_width, h = img_height },
    book_info_group,
  }
end

--- @param manga MManga
function MangaInfoWidget:genStatisticsGroup(width, manga)
  local height = Screen:scaleBySize(60)
  local statistics_container = CenterContainer:new {
    dimen = Geom:new { w = width, h = height },
  }

  local statistics_group = VerticalGroup:new { align = "left" }

  local tile_width = width * (1 / 3)
  local tile_height = height * (1 / 2)

  local titles_group = HorizontalGroup:new {
    align = "center",
    CenterContainer:new {
      dimen = Geom:new { w = tile_width, h = tile_height },
      TextWidget:new {
        text = _("Status"),
        face = self.small_font_face,
      },
    },
    CenterContainer:new {
      dimen = Geom:new { w = tile_width, h = tile_height },
      TextWidget:new {
        text = _("NSFW"),
        face = self.small_font_face,
      },
    },
    CenterContainer:new {
      dimen = Geom:new { w = tile_width, h = tile_height },
      TextWidget:new {
        text = _("Last Updated"),
        face = self.small_font_face,
      }
    }
  }

  local data_group = HorizontalGroup:new {
    align = "center",
    CenterContainer:new {
      dimen = Geom:new { w = tile_width, h = tile_height },
      TextWidget:new {
        text = self:getStatus(manga),
        face = self.medium_font_face,
      },
    },
    CenterContainer:new {
      dimen = Geom:new { w = tile_width, h = tile_height },
      TextWidget:new {
        text = self:getNSFW(manga),
        face = self.medium_font_face,
      },
    },
    CenterContainer:new {
      dimen = Geom:new { w = tile_width, h = tile_height },
      TextWidget:new {
        text = manga.last_updated and calcLastReadText(parse_iso8601(manga.last_updated)) or _("N/A"),
        face = self.medium_font_face,
      }
    }
  }

  table.insert(statistics_group, titles_group)
  table.insert(statistics_group, data_group)

  table.insert(statistics_container, statistics_group)
  return statistics_container
end

--- @param manga MManga
function MangaInfoWidget:getStatus(manga)
  if manga.status == PublishingStatus.Ongoing then
    return _("Ongoing")
  elseif manga.status == PublishingStatus.Completed then
    return _("Completed")
  elseif manga.status == PublishingStatus.Cancelled then
    return _("Cancelled")
  elseif manga.status == PublishingStatus.Hiatus then
    return _("Hiatus")
  elseif manga.status == PublishingStatus.NotPublished then
    return _("Not published")
  end
  return _("Unknown")
end

--- @param manga MManga
function MangaInfoWidget:getNSFW(manga)
  if manga.nsfw == MangaContentRating.Safe then
    return _("Safe")
  elseif manga.nsfw == MangaContentRating.Suggestive then
    return _("Suggestive")
  elseif manga.nsfw == MangaContentRating.Nsfw then
    return _("NSFW")
  end

  return "N/A"
end

--- @param manga MManga
function MangaInfoWidget:genSummaryGroup(width, manga)
  local height
  if Screen:getScreenMode() == "landscape" then
    height = Screen:scaleBySize(80)
  else
    height = Screen:scaleBySize(160)
  end

  local text_padding = Size.padding.default
  self.input_note = ScrollTextWidget:new {
    text = manga.description or "N/A",
    face = self.medium_font_face,
    width = width - self.padding * 3,
    height = math.floor(height),
    dialog = TextViewer:new {
      title = _("Description"),
      text = manga.description
    },
    scroll = true,
    bordersize = Size.border.default,
    focused = false,
    padding = text_padding,
    parent = self,
  }
  table.insert(self.layout, { self.input_note })

  return VerticalGroup:new {
    VerticalSpan:new { width = Size.span.vertical_large },
    CenterContainer:new {
      dimen = Geom:new { w = width, h = height },
      self.input_note
    }
  }
end

function MangaInfoWidget:onSwipe(arg, ges_ev)
  if ges_ev.direction == "south" then
    -- Allow easier closing with swipe down
    self:onClose()
  elseif ges_ev.direction == "west" or ges_ev.direction == "north" then
    UIManager:show(TextViewer:new {
      title = _("Description"),
      text = self.manga.description
    })
  elseif ges_ev.direction == "east" or ges_ev.direction == "west" or ges_ev.direction == "north" then
    -- no use for now
    do end -- luacheck: ignore 541
  else     -- diagonal swipe
    -- trigger full refresh
    UIManager:setDirty(nil, "full", nil, true)
    -- a long diagonal swipe may also be used for taking a screenshot,
    -- so let it propagate
    return false
  end
end

function MangaInfoWidget:onMultiSwipe(arg, ges_ev)
  -- For consistency with other fullscreen widgets where swipe south can't be
  -- used to close and where we then allow any multiswipe to close, allow any
  -- multiswipe to close this widget too.
  self:onClose()
  return true
end

function MangaInfoWidget:onClose(run_return_callback)
  -- NOTE: Flash on close to avoid ghosting, since we show an image.
  UIManager:close(self, "flashpartial")
  if self.close_callback then
    self.close_callback()
  end
  if self.on_return_callback and run_return_callback ~= false then
    self.on_return_callback()
  end
  return true
end

--- @param source_id string
--- @param manga_id string
function MangaInfoWidget:refreshDetails(source_id, manga_id)
  Trapper:wrap(function()
    local refresh_details_response = LoadingDialog:showAndRun(
      "Refreshing details...",
      function()
        return Backend.refreshMangaDetails(source_id, manga_id)
      end,
      function()
        local cancelledMessage = InfoMessage:new {
          text = "Cancelled.",
        }
        UIManager:show(cancelledMessage)
      end
    )

    if refresh_details_response.type == 'ERROR' then
      ErrorDialog:show(refresh_details_response.message)

      return
    end
  end)
end

--- @param raw_manga Manga
--- @param on_return_callback fun()|nil
--- @param no_refresh boolean|nil
function MangaInfoWidget:fetchAndShow(raw_manga, on_return_callback, no_refresh)
  -- Trapper:wrap(function()
  local response = LoadingDialog:showAndRun(
    "Loading details...",
    function() return Backend.cachedMangaDetails(raw_manga.source.id, raw_manga.id) end,
    nil,
    true
  )

  if response.type == 'ERROR' and response.status == 404 and no_refresh ~= true then
    MangaInfoWidget:refreshDetails(raw_manga.source.id, raw_manga.id)
    MangaInfoWidget:fetchAndShow(raw_manga, on_return_callback, true)

    return
  end

  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  Trapper:wrap(function()
    Backend.refreshMangaDetails(raw_manga.source.id, raw_manga.id)
  end)

  ---@diagnostic disable-next-line: redundant-parameter
  local widget = MangaInfoWidget:new {
    raw_manga = raw_manga,
    manga = response.body[1],
    per_read = response.body[2],
    on_return_callback = on_return_callback,
  }
  UIManager:show(widget)
  -- end)
end

return MangaInfoWidget
