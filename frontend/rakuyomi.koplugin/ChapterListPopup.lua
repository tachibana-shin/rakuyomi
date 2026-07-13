local FocusManager = require("ui/widget/focusmanager")
local FrameContainer = require("ui/widget/container/framecontainer")
local CenterContainer = require("ui/widget/container/centercontainer")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local Button = require("ui/widget/button")
local ChapterMenuItem = require("ChapterMenuItem")
local InputDialog = require("ui/widget/inputdialog")
local Menu = require("widgets/Menu")
local TextWidget = require("ui/widget/textwidget")

local GestureRange = require("ui/gesturerange")
local TitleBar = require("ui/widget/titlebar")
local Size = require("ui/size")
local Geom = require("ui/geometry")
local Blitbuffer = require("ffi/blitbuffer")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Device = require("device")
local Font = require("ui/font")
local _ = require("gettext+")
local Icons = require("Icons")

local SMALL_FONT_NAME = "smallinfofont"

--- Filter chapters by query string matching against chapter title, volume/chapter numbers.
--- Returns a new filtered list (reference-safe, does not mutate input).
--- @param chapters Chapter[]
--- @param query string
--- @return Chapter[]
local function filterChapters(chapters, query)
  if query == "" then return chapters end
  local q = query:lower()
  local result = {}
  for __, ch in ipairs(chapters) do
    local text = ""
    if ch.volume_num then text = text .. ch.volume_num end
    if ch.chapter_num then text = text .. ch.chapter_num end
    if ch.title then text = text .. ch.title end
    if ch.scanlator then text = text .. ch.scanlator end
    if text:lower():find(q, 1, true) then
      table.insert(result, ch)
    end
  end
  return result
end

--- Estimate the fixed heights used by the popup layout (excluding items).
--- Returns top_height (title bar + spans above content) and bottom_height (spans below content + nav bar).
local function estimateChromeHeights(title_bar)
  local top_height = title_bar:getHeight() + Size.span.vertical_large
  local nav_h = Button:new { icon = "chevron.left", bordersize = 0 }:getSize().h
  local bottom_height = Size.span.vertical_large + nav_h + Size.padding.large
  return top_height, bottom_height
end

--- Compute items_per_page from screen and chrome heights.
local function calcItemsPerPage(screen_h, top_height, bottom_height, font_size, item_width)
  local text_h = TextWidget:new {
    text = "Xg",
    face = Font:getFace(SMALL_FONT_NAME, font_size),
  }:getSize().h
  local sample = ChapterMenuItem:new {
    text = "Xg",
    mandatory = "X",
    font_size = font_size,
    infont_size = Menu.getItemMandatoryFontSize(math.floor(screen_h / 30)),
    dimen = Geom:new { w = item_width, h = text_h * 1.5 },
    single_line = true,
  }
  local item_h = sample:getSize().h
  local available = screen_h - top_height - bottom_height
  return math.max(1, math.floor(available * 0.9 / item_h))
end

--- @class ChapterListPopup : FocusManager
--- @field chapter Chapter
--- @field chapters Chapter[]
--- @field on_chapter_selected fun(chapter: Chapter): nil
--- @field show_langs boolean
--- @field show_scanlator boolean
--- @field new any
--- @field private current_page number
--- @field private total_pages number
--- @field private popup_width number
--- @field private content any
--- @field private nav_bar any
--- @field private dialog_frame any
--- @field private search_query string
--- @field private display_chapters Chapter[]
--- @field private item_dimen any
--- @field private font_size number
--- @field private infont_size number
local ChapterListPopup = FocusManager:extend {
  name = "chapter_list_popup",
  is_always_active = true,
}

--- @class ChapterListPopupOpenOptions
--- @field chapter Chapter
--- @field chapters Chapter[]
--- @field on_chapter_selected fun(chapter: Chapter): nil

--- @param opts ChapterListPopupOpenOptions
function ChapterListPopup:create(opts)
  return ChapterListPopup:new {
    chapter = opts.chapter,
    chapters = opts.chapters,
    on_chapter_selected = opts.on_chapter_selected,
  }
end

function ChapterListPopup:init()
  local screen_w = Screen:getWidth()
  local screen_h = Screen:getHeight()

  if Device:hasKeys() then
    self.key_events.Close = { { Device.input.group.Back } }
  end

  self.layout = {}
  self.popup_width = math.floor(screen_w * 0.85)
  self.search_query = ""
  self.display_chapters = self.chapters

  self.title_bar = TitleBar:new {
    title = _("Chapters"),
    width = self.popup_width,
    close_callback = function() self:onClose() end,
    show_parent = self,
  }

  local top_h, bottom_h = estimateChromeHeights(self.title_bar)
  self.items_per_page = calcItemsPerPage(screen_h, top_h, bottom_h, 20, self.popup_width)
  self.font_size = Menu.getItemFontSize(self.items_per_page)
  self.infont_size = Menu.getItemMandatoryFontSize(self.items_per_page)

  local item_text_h = TextWidget:new {
    text = "Xg",
    face = Font:getFace(SMALL_FONT_NAME, self.font_size),
  }:getSize().h
  local item_sample = ChapterMenuItem:new {
    text = "Xg",
    mandatory = "X",
    font_size = self.font_size,
    infont_size = self.infont_size,
    dimen = Geom:new { w = self.popup_width, h = item_text_h },
    single_line = true,
  }
  self.item_dimen = Geom:new { w = self.popup_width, h = item_sample:getSize().h }

  self.total_pages = math.max(1, math.ceil(#self.display_chapters / self.items_per_page))

  self.current_page = 1
  for i, ch in ipairs(self.display_chapters) do
    if ch.id == self.chapter.id then
      self.current_page = math.ceil(i / self.items_per_page)
      break
    end
  end

  self.page_indicator = Button:new {
    text = "",
    bordersize = 0,
    text_font_face = SMALL_FONT_NAME,
    callback = function() self:onPageIndicatorTap() end,
  }

  self.content = VerticalGroup:new { align = "left" }
  self.nav_bar = HorizontalGroup:new { align = "center" }

  self.dialog_frame = VerticalGroup:new {
    align = "center",
    self.title_bar,
    VerticalSpan:new { width = Size.span.vertical_large },
    self.content,
    VerticalSpan:new { width = Size.span.vertical_large },
    self.nav_bar,
    VerticalSpan:new { width = Size.padding.large },
  }

  local frame = FrameContainer:new {
    radius = Size.radius.window,
    bordersize = Size.border.window,
    padding = 0,
    background = Blitbuffer.COLOR_WHITE,
    self.dialog_frame,
  }

  self[1] = CenterContainer:new {
    dimen = Screen:getSize(),
    frame,
  }

  local screen_rect = Geom:new {
    x = 0, y = 0,
    w = screen_w, h = screen_h,
  }
  self.ges_events = {
    TapClose = {
      GestureRange:new { ges = "tap", range = screen_rect },
    },
    Swipe = {
      GestureRange:new { ges = "swipe", range = screen_rect },
    },
  }

  self.frame = frame
  self:renderPage()
  UIManager:show(self, "partial", self[1].dimen)
end

function ChapterListPopup:renderPage()
  self.layout = {}

  for i = #self.content, 1, -1 do
    self.content[i] = nil
  end
  for i = #self.nav_bar, 1, -1 do
    self.nav_bar[i] = nil
  end

  local start_idx = (self.current_page - 1) * self.items_per_page + 1
  local end_idx = math.min(self.current_page * self.items_per_page, #self.display_chapters)

  local ChapterListing = require("ChapterListing")
  for i = start_idx, end_idx do
    local chapter = self.display_chapters[i]
    local is_current = chapter.id == self.chapter.id
    local item = ChapterListing.renderChapterItem(chapter, self.show_langs, self.show_scanlator)

    local entry = {
      text = item.text,
      post_text = item.post_text,
      mandatory = (is_current and Icons.FA_CHECK .. " " or "") .. item.mandatory,
      mandatory_dim = item.dim,
      dim = item.dim,
      chapter = chapter,
    }

    local menu_item = ChapterMenuItem:new {
      text = entry.text,
      post_text = entry.post_text,
      mandatory = entry.mandatory,
      mandatory_dim = entry.mandatory_dim,
      dim = entry.dim,
      font_size = self.font_size,
      infont_size = self.infont_size,
      dimen = self.item_dimen:copy(),
      entry = entry,
      menu = self,
      single_line = true,
      show_parent = self,
      line_color = Blitbuffer.COLOR_GRAY,
      is_current = is_current,
      callback = function()
        self:onMenuSelect(chapter)
      end
    }
    table.insert(self.content, menu_item)
    table.insert(self.layout, { menu_item })
  end

  self.page_indicator:setText(self.current_page .. " / " .. self.total_pages)

  local search_btn = Button:new {
    icon = "appbar.search",
    bordersize = 0,
    callback = function() self:onSearch() end,
  }
  local prev_btn = Button:new {
    icon = "chevron.left",
    bordersize = 0,
    enabled = self.current_page > 1,
    callback = function() self:prevPage() end,
  }
  local next_btn = Button:new {
    icon = "chevron.right",
    bordersize = 0,
    enabled = self.current_page < self.total_pages,
    callback = function() self:nextPage() end,
  }

  table.insert(self.nav_bar, HorizontalSpan:new { width = Size.padding.large })
  table.insert(self.nav_bar, prev_btn)
  table.insert(self.nav_bar, HorizontalSpan:new { width = Size.padding.large })
  table.insert(self.nav_bar, self.page_indicator)
  table.insert(self.nav_bar, HorizontalSpan:new { width = Size.padding.large })
  table.insert(self.nav_bar, next_btn)
  table.insert(self.nav_bar, search_btn)
end

--- Adapter for MenuItem's menu reference.
--- @param chapter Chapter
function ChapterListPopup:onMenuSelect(chapter)
  if chapter.id ~= self.chapter.id then
    self.on_chapter_selected(chapter)
  end
end

function ChapterListPopup:onTapClose(_, ges)
  local pos = ges.pos
  for _, child in ipairs(self.content) do
    if child.frame and child.frame.dimen and child.frame.dimen:contains(pos) then
      return true
    end
  end
  if not self.frame.dimen:contains(pos) then
    self:onClose()
    return true
  end
  return true
end

function ChapterListPopup:onSwipe(_, ges)
  if ges.direction == "west" or ges.direction == "north" then
    self:nextPage()
  elseif ges.direction == "east" or ges.direction == "south" then
    self:prevPage()
  end
  return true
end

function ChapterListPopup:prevPage()
  if self.current_page > 1 then
    self.current_page = self.current_page - 1
    self:renderPage()
    UIManager:setDirty("all", "partial", self[1].dimen)
  end
end

function ChapterListPopup:nextPage()
  if self.current_page < self.total_pages then
    self.current_page = self.current_page + 1
    self:renderPage()
    UIManager:setDirty("all", "partial", self[1].dimen)
  end
end

function ChapterListPopup:goToPage(page)
  if page < 1 then page = 1 end
  if page > self.total_pages then page = self.total_pages end
  if page == self.current_page then return end
  self.current_page = page
  self:renderPage()
  UIManager:setDirty("all", "partial", self[1].dimen)
end

function ChapterListPopup:onSearch()
  local dialog
  dialog = InputDialog:new {
    title = _("Search chapters"),
    input = self.search_query,
    input_type = "text",
    buttons = {
      {
        { text = _("Cancel"), callback = function() UIManager:close(dialog) end },
        {
          text = _("Apply"),
          callback = function()
            local query = dialog:getInputText()
            UIManager:close(dialog)
            self.search_query = query
            self.display_chapters = filterChapters(self.chapters, query)
            self.total_pages = math.max(1, math.ceil(#self.display_chapters / self.items_per_page))
            self.current_page = 1
            self:renderPage()
            UIManager:setDirty("all", "partial", self[1].dimen)
          end,
        },
      },
    },
    show_parent = self,
  }
  UIManager:show(dialog)
  dialog:onShowKeyboard()
end

function ChapterListPopup:onPageIndicatorTap()
  local dialog
  dialog = InputDialog:new {
    title = _("Go to page"),
    input = tostring(self.current_page),
    input_type = "number",
    buttons = {
      {
        { text = _("Cancel"), callback = function() UIManager:close(dialog) end },
        {
          text = _("Go"),
          callback = function()
            local page = tonumber(dialog:getInputText())
            UIManager:close(dialog)
            if page then
              self:goToPage(page)
            end
          end,
        },
      },
    },
    show_parent = self,
  }
  UIManager:show(dialog)
end

function ChapterListPopup:onClose()
  UIManager:close(self)
end

return ChapterListPopup
