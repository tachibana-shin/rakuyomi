local _ = require("gettext")
local ImageWidget = require("ui/widget/imagewidget")
local MenuItemRaw = require("MenuItem")
local Blitbuffer = require("ffi/blitbuffer")
local Trapper = require("ui/trapper")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")

local OverlapGroup = require("ui/widget/overlapgroup")
local ConfirmBox = require("ui/widget/confirmbox")
local LeftContainer = require("ui/widget/container/leftcontainer")
local CenterContainer = require("ui/widget/container/centercontainer")
local Device = require("device")
local UnderlineContainer = require("ui/widget/container/underlinecontainer")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local RightContainer = require("ui/widget/container/rightcontainer")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local Size = require("ui/size")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TextWidget = require("ui/widget/textwidget")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local ffiUtil = require("ffi/util")
local _ = require("gettext")
local ChapterListing = require("ChapterListing")
local InfoMessage = require("ui/widget/infomessage")
local calcLastReadText = require("utils/calcLastReadText")

local Screen = Device.screen
local T = ffiUtil.template

local MenuItem = MenuItemRaw:extend {

}

function MenuItem:init()
  self.ges_events = {
    TapSelect = {
      GestureRange:new {
        ges = "tap",
        range = self.dimen,
      },
    },
    HoldSelect = {
      GestureRange:new {
        ges = self.handle_hold_on_hold_release and "hold_release" or "hold",
        range = self.dimen,
      },
    },
  }

  local max_item_height = self.dimen.h - 2 * self.linesize

  local img_width, img_height
  if Screen:getScreenMode() == "landscape" then
    img_height = math.min(Screen:scaleBySize(184), max_item_height)
  else
    img_height = math.min(Screen:scaleBySize(184 * 1.5), max_item_height)
  end
  img_width = 132 / 184 * img_height

  self.content_width = self.dimen.w - 2 * Size.padding.fullscreen

  -- We want to show at least one line, so cap the provided font sizes
  local max_font_size = TextBoxWidget:getFontSizeToFitHeight(max_item_height, 1)
  if self.font_size > max_font_size then
    self.font_size = max_font_size
  end
  if self.infont_size > max_font_size then
    self.infont_size = max_font_size
  end

  self.face = Font:getFace(self.font, self.font_size)
  self.info_face = Font:getFace(self.infont, self.infont_size)
  self.post_text_face = Font:getFace(self.font, self.infont_size)

  local screen_width = Screen:getWidth()
  local split_span_width = math.floor(screen_width * 0.05)

  if self.entry.chapter_id == nil then
    self[1] = FrameContainer:new {
      bordersize = 0,
      padding = 0,
      HorizontalGroup:new {
        align = "center",
        TextBoxWidget:new {
          text = self.entry.text,
          -- lang = lang,
          width = screen_width - split_span_width - img_width,
          face = self.face,
          alignment = "center",
          fgcolor = Blitbuffer.COLOR_DARK_GRAY,
        },
      }
    }

    return
  end

  --- @type Notification
  local notify = self.entry

  local text_container = LeftContainer:new {
    dimen = Geom:new { w = self.content_width, h = self.dimen.h },
    HorizontalGroup:new {
      self:genCover(img_width, img_height),
      HorizontalSpan:new {
        width = 8
      },
      VerticalGroup:new {
        TextBoxWidget:new {
          text = notify.manga_title,
          -- lang = lang,
          width = screen_width - split_span_width - img_width,
          face = self.face,
          alignment = "left",
        },
        TextBoxWidget:new {
          text = "Ch." .. (notify.chapter_number or "unknown") .. ": " .. notify.chapter_title,
          width = screen_width - split_span_width - img_width,
          face = self.info_face,
          bold = self.bold,
        }
      },
      HorizontalSpan:new {
        width = 8
      },
    }
  }

  self._underline_container = UnderlineContainer:new {
    color = self.line_color,
    linesize = 0,
    vertical_align = "center",
    padding = 0,
    dimen = Geom:new {
      x = 0, y = 0,
      w = self.content_width,
      h = self.dimen.h
    },
    HorizontalGroup:new {
      align = "center",
      OverlapGroup:new {
        dimen = Geom:new { w = self.content_width, h = self.dimen.h },

        text_container,
        RightContainer:new {
          dimen = Geom:new { w = self.content_width, h = self.dimen.h },
          HorizontalGroup:new {
            TextWidget:new {
              text = calcLastReadText(notify.created_at),
              face = self.info_face,
              bold = self.bold,
              fgcolor = Blitbuffer.COLOR_DARK_GRAY or nil,
            },

            -- IconButton:new {
            --   icon = "close",
            --   width = self.font_size,
            --   height = self.font_size,
            --   padding = Screen:scaleBySize(11),
            --   callback = function()
            --     -- self:openSearchMangasDialog()
            --   end,
            -- },
          }
        }
      },
    }
  }
  local hgroup = HorizontalGroup:new {
    align = "center",
    HorizontalSpan:new { width = self.items_padding or Size.padding.fullscreen },
  }
  table.insert(hgroup, self._underline_container)
  table.insert(hgroup, HorizontalSpan:new { width = Size.padding.fullscreen })

  self[1] = FrameContainer:new {
    bordersize = 0,
    padding = 0,
    hgroup,
  }

  -- self[1] = FrameContainer:new {
  --   bordersize = 0,
  --   padding = Size.padding.fullscreen,
  --   HorizontalGroup:new {
  --     self:genCover(img_width, img_height),
  --   },
  -- }
end

function MenuItem:onFocus()
  return true
end

function MenuItem:onUnfocus()
  return true
end

local function starts_with(s, p)
  return s:sub(1, #p) == p
end

local function getCachedCoverSize(img_w, img_h, max_img_w, max_img_h)
  local scale_factor
  local width = math.floor(max_img_h * img_w / img_h + 0.5)
  if max_img_w >= width then
    max_img_w = width
    scale_factor = max_img_w / img_w
  else
    max_img_h = math.floor(max_img_w * img_h / img_w + 0.5)
    scale_factor = max_img_h / img_h
  end
  return max_img_w, max_img_h, scale_factor
end


local scale_by_size = Screen:scaleBySize(1000000) * (1 / 1000000)
function MenuItem:genCover(wleft_width, wleft_height)
  local border_size = Size.border.thin

  local wleft
  if self.entry.manga_cover and starts_with(self.entry.manga_cover, "file://") then
    local wimage = ImageWidget:new {
      file = self.entry.manga_cover:gsub("^file://", ""),
      -- scale_factor = 0.5
    }
    wimage:_loadfile()
    local image_size = wimage:getSize() -- get final widget size
    local _, _, scale_factor = getCachedCoverSize(image_size.w, image_size.h, wleft_width, wleft_height)

    wimage = ImageWidget:new {
      file = self.entry.manga_cover:gsub("^file://", ""),
      scale_factor = scale_factor
    }

    wimage:_render()
    image_size = wimage:getSize()

    wleft = CenterContainer:new {
      dimen = Geom:new { w = wleft_width, h = wleft_height },
      FrameContainer:new {
        width = image_size.w + 2 * border_size,
        height = image_size.h + 2 * border_size,
        margin = 0,
        padding = 0,
        bordersize = border_size,
        dim = self.file_deleted,
        wimage,
      }
    }
    -- Let menu know it has some item with images
    self.menu._has_cover_images = true
    self._has_cover_image = true
  else
    local function _fontSize(nominal, max)
      -- The nominal font size is based on 64px ListMenuItem height.
      -- Keep ratio of font size to item height
      local font_size = math.floor(nominal * self.dimen.h * (1 / 64) / scale_by_size)
      -- But limit it to the provided max, to avoid huge font size when
      -- only 4-6 items per page
      if max and font_size >= max then
        return max
      end
      return font_size
    end

    local fake_cover_w = wleft_width - border_size * 2
    local fake_cover_h = wleft_height - border_size * 2
    wleft = CenterContainer:new {
      dimen = Geom:new { w = wleft_width, h = wleft_height },
      FrameContainer:new {
        width = fake_cover_w + 2 * border_size,
        height = fake_cover_h + 2 * border_size,
        margin = 0,
        padding = 0,
        bordersize = border_size,
        dim = self.file_deleted,
        CenterContainer:new {
          dimen = Geom:new { w = fake_cover_w, h = fake_cover_h },
          TextWidget:new {
            text = "⛶", -- U+26F6 Square four corners
            face = Font:getFace("cfont", _fontSize(20)),
          },
        },
      },
    }
  end

  return wleft
end

--- @class Menu
--- @field new fun(self: Menu): Menu
--- @field dimen any
--- @field item_group any
--- @field page_info any
--- @field return_button any
--- @field content_group any
--- @field _recalculateDimen fun(bool)
--- @field items_max_lines number
--- @field page_items any[]
--- @field perpage number
--- @field items_font_size number|nil
--- @field title_bar any
--- @field no_title boolean|nil
--- @field page_return_arrow any
--- @field page_info_text string|nil
--- @field inner_dimen any
--- @field setupItemHeights fun()
--- @field getPageNumber fun()
--- @field itemnumber number
--- @field is_enable_shortcut boolean
--- @field item_shortcuts any[]
--- @field item_dimen any
--- @field updatePageInfo fun(any)
--- @field mergeTitleBarIntoLayout fun()
--- @field show_parent any
--- @field onClose fun()
--- @field openMenu fun()

--- @class NotificationView : Menu
--- @field notifications Notification[]
--- @field on_return_callback fun()|nil
local NotificationView = Menu:extend {
  name = "notification_view",
  is_enable_shortcut = false,
  is_popout = false,
  title = "Notification",
  with_context_menu = true,

  items_per_page = 10,
  notifications = nil,
  on_return_callback = nil
}

function NotificationView:init()
  self.mangas = self.mangas or {}

  self.title_bar_left_icon = "appbar.pokeball"
  self.onLeftButtonTap = function()
    local response = Backend.clearNotifications()
    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    UIManager:show(InfoMessage:new {
      text = "Cleared all notifications!"
    })

    self.notifications = {}
    self:updateItems()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()

  local page = self.page
  Menu.init(self)
  self.page = page

  self:updateItems()
end

--- @param select_number number|nil
---@param no_recalculate_dimen boolean|nil
function NotificationView:updateItems(select_number, no_recalculate_dimen)
  if #self.notifications > 0 then
    self.item_table = self.notifications
    self.multilines_show_more_text = false
    self.items_per_page = nil
  else
    self.item_table = self:generateEmptyViewItemTable()
    self.multilines_show_more_text = true
    self.items_per_page = 1
  end

  local old_dimen = self.dimen and self.dimen:copy()
  -- self.layout must be updated for focusmanager
  self.layout = {}
  self.item_group:clear()
  self.page_info:resetLayout()
  self.return_button:resetLayout()
  self.content_group:resetLayout()
  ---@diagnostic disable-next-line: redundant-parameter
  self:_recalculateDimen(no_recalculate_dimen)

  local items_nb -- number of items in the visible page
  local idx_offset, multilines_show_more_text
  if self.items_max_lines then
    items_nb = #self.page_items[self.page]
  else
    items_nb = self.perpage
    idx_offset = (self.page - 1) * items_nb
    multilines_show_more_text = self.multilines_show_more_text
    if multilines_show_more_text == nil then
      multilines_show_more_text = G_reader_settings:isTrue("items_multilines_show_more_text")
    end
  end

  for idx = 1, items_nb do
    local index = self.items_max_lines and self.page_items[self.page][idx] or idx_offset + idx
    local item = self.item_table[index]
    if item == nil then break end
    ---@diagnostic disable-next-line: inject-field
    item.idx = index                 -- index is valid only for items that have been displayed
    if index == self.itemnumber then -- focused item
      select_number = idx
    end
    local item_shortcut, shortcut_style
    if self.is_enable_shortcut then
      item_shortcut = self.item_shortcuts[idx]
      -- give different shortcut_style to keys in different lines of keyboard
      shortcut_style = (idx < 11 or idx > 20) and "square" or "grey_square"
    end

    local item_tmp = MenuItem:new {
      idx = index,
      show_parent = self.show_parent,

      ---@diagnostic disable-next-line: undefined-field
      state_w = self.state_w,
      ---@diagnostic disable-next-line: undefined-field
      bold = self.item_table.current == index,
      ---@diagnostic disable-next-line: undefined-field
      font_size = self.font_size,
      ---@diagnostic disable-next-line: undefined-field
      infont_size = self.items_mandatory_font_size or (self.font_size - 4),
      dimen = self.item_dimen:copy(),
      shortcut = item_shortcut,
      shortcut_style = shortcut_style,
      entry = item,
      menu = self,
      ---@diagnostic disable-next-line: undefined-field
      linesize = self.linesize,
      ---@diagnostic disable-next-line: undefined-field
      single_line = self.single_line,
      ---@diagnostic disable-next-line: undefined-field
      multilines_forced = self.multilines_forced,
      multilines_show_more_text = multilines_show_more_text,
      items_max_lines = self.items_max_lines,
      ---@diagnostic disable-next-line: undefined-field
      truncate_left = self.truncate_left,
      ---@diagnostic disable-next-line: undefined-field
      align_baselines = self.align_baselines,
      ---@diagnostic disable-next-line: undefined-field
      with_dots = self.with_dots,
      ---@diagnostic disable-next-line: undefined-field
      line_color = self.line_color,
      ---@diagnostic disable-next-line: undefined-field
      items_padding = self.items_padding,
      ---@diagnostic disable-next-line: undefined-field
      handle_hold_on_hold_release = self.handle_hold_on_hold_release,
    }
    table.insert(self.item_group, item_tmp)
    -- this is for focus manager
    table.insert(self.layout, { item_tmp })
  end

  ---@diagnostic disable-next-line: redundant-parameter
  self:updatePageInfo(select_number)
  self:mergeTitleBarIntoLayout()

  UIManager:setDirty(self.show_parent, function()
    local refresh_dimen =
        old_dimen and old_dimen:combine(self.dimen)
        or self.dimen
    return "ui", refresh_dimen
  end)
end

--- @private
function NotificationView:generateEmptyViewItemTable()
  return {
    {
      text = "No notifications",
      dim = true,
      select_enabled = false,
    }
  }
end

--- @param onReturnCallback fun()
function NotificationView:fetchAndShow(onReturnCallback)
  local response = Backend.getNotifications()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local notifications = response.body

  ---@diagnostic disable-next-line: redundant-parameter
  local widget = NotificationView:new {
    notifications = notifications,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
    page = self.page,
    on_return_callback = onReturnCallback
  }
  ---@diagnostic disable-next-line: inject-field
  widget.on_return_callback = onReturnCallback
  UIManager:show(widget)
end

--- @private
function NotificationView:onMenuSelect(item)
  local onReturnCallback = function()
    self:fetchAndShow(self.on_return_callback)
  end

  Trapper:wrap(function()
    --- @type Notification
    local notify = item
    local manga = {
      id = notify.chapter_id.manga_id.manga_id,
      source = {
        id = notify.chapter_id.manga_id.source_id
      },
      title = notify.manga_title
    }

    if ChapterListing:fetchAndShow(manga, onReturnCallback, true) then
      self:onClose(false)
    end
  end)
end

function NotificationView:onMenuHold(item)
  local confirm_dialog
  confirm_dialog = ConfirmBox:new {
    text = "Delete this notification?",
    ok_text = _("Delete"),
    cancel_text = _("Cancel"),
    ok_callback = function()
      UIManager:close(confirm_dialog)

      local response = Backend.removeNotification(item.id)
      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end

      local response = Backend.getNotifications()
      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end

      self.notifications = response.body
      self:updateItems()
    end,
    cancel_callback = function()
      UIManager:close(confirm_dialog)
    end
  }

  UIManager:show(confirm_dialog)
  return true
end

function NotificationView:onClose(call_return)
  UIManager:close(self)
  if self.on_return_callback and call_return ~= false then
    self.on_return_callback()
  end
end

return NotificationView
