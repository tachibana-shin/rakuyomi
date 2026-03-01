local Menu = require("widgets/Menu")

local Device = require("device")
local UIManager = require("ui/uimanager")

local Size = require("ui/size")
local Math = require("optmath")
local Geom = require("ui/geometry")
local HorizontalGroup = require("ui/widget/horizontalgroup")

local Screen = Device.screen

local MenuCustom = Menu:extend {}

--- @param MenuItem any
--- @param select_number number|nil
--- @param no_recalculate_dimen boolean|nil
function MenuCustom:updateItems(MenuItem, select_number, no_recalculate_dimen)
  local old_dimen = self.dimen and self.dimen:copy()
  -- self.layout must be updated for focusmanager
  self.layout = {}
  self.item_group:clear()
  self.page_info:resetLayout()
  self.return_button:resetLayout()
  self.content_group:resetLayout()
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

  local columns = self.grid_columns or 1
  if columns > 1 then
    local rows = math.ceil(items_nb / columns)
    for r = 1, rows do
      local row_group = HorizontalGroup:new { align = "center" }
      for c = 1, columns do
        local idx_in_page = (r - 1) * columns + c
        if idx_in_page <= items_nb then
          local index = self.items_max_lines and self.page_items[self.page][idx_in_page] or idx_offset + idx_in_page
          local item = self.item_table[index]
          if item then
            item.idx = index
            if index == self.itemnumber then
              select_number = idx_in_page
            end

            local item_tmp = MenuItem:new {
              idx = index,
              show_parent = self.show_parent,
              state_w = self.state_w,
              text = Menu.getMenuText(item),
              manga_cover = item.manga_cover,
              text_bgcolor = item.text_bgcolor,
              bidi_wrap_func = item.bidi_wrap_func,
              post_text = item.post_text,
              mandatory = item.mandatory,
              mandatory_func = item.mandatory_func,
              mandatory_dim = item.mandatory_dim or item.dim,
              mandatory_dim_func = item.mandatory_dim_func,
              bold = self.item_table.current == index or item.bold == true,
              dim = item.dim,
              font_size = self.font_size,
              infont_size = self.items_mandatory_font_size or (self.font_size - 4),
              dimen = self.item_dimen:copy(),
              entry = item,
              menu = self,
              linesize = self.linesize,
              single_line = self.single_line,
              line_color = self.line_color,
              handle_hold_on_hold_release = self.handle_hold_on_hold_release,
            }
            table.insert(row_group, item_tmp)
          end
        end
      end
      table.insert(self.item_group, row_group)
      table.insert(self.layout, { row_group })
    end
  else
    for idx = 1, items_nb do
      local index = self.items_max_lines and self.page_items[self.page][idx] or idx_offset + idx
      local item = self.item_table[index]
      if item == nil then break end
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
        state_w = self.state_w,
        text = Menu.getMenuText(item),
        manga_cover = item.manga_cover,
        text_bgcolor = item.text_bgcolor,
        bidi_wrap_func = item.bidi_wrap_func,
        post_text = item.post_text,
        mandatory = item.mandatory,
        mandatory_func = item.mandatory_func,
        mandatory_dim = item.mandatory_dim or item.dim,
        mandatory_dim_func = item.mandatory_dim_func,
        bold = self.item_table.current == index or item.bold == true,
        dim = item.dim,
        font_size = self.font_size,
        infont_size = self.items_mandatory_font_size or (self.font_size - 4),
        dimen = self.item_dimen:copy(),
        shortcut = item_shortcut,
        shortcut_style = shortcut_style,
        entry = item,
        menu = self,
        linesize = self.linesize,
        single_line = self.single_line,
        multilines_forced = self.multilines_forced,
        multilines_show_more_text = multilines_show_more_text,
        items_max_lines = self.items_max_lines,
        truncate_left = self.truncate_left,
        align_baselines = self.align_baselines,
        with_dots = self.with_dots,
        line_color = self.line_color,
        items_padding = self.items_padding,
        handle_hold_on_hold_release = self.handle_hold_on_hold_release,
      }
      table.insert(self.item_group, item_tmp)
      -- this is for focus manager
      table.insert(self.layout, { item_tmp })
    end
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

local scale_by_size = Screen:scaleBySize(1000000) * (1 / 1000000)

---@param no_recalculate_dimen boolean|nil
function MenuCustom:_recalculateDimen(no_recalculate_dimen)
  self.portrait_mode = Screen:getWidth() <= Screen:getHeight()

  self.others_height = 0
  if self.title_bar then -- Menu:init() has been done
    if not self.is_borderless then
      self.others_height = self.others_height + 2
    end
    if not self.no_title then
      self.others_height = self.others_height + self.title_bar.dimen.h
    end
    if self.page_info then
      self.others_height = self.others_height + self.page_info:getSize().h
    end
  end

  local available_height = self.inner_dimen.h - self.others_height - Size.line.thin

  self.items_per_page = G_reader_settings:readSetting("rakuyomi_items_per_page")
  if self.items_per_page == nil or self.items_per_page < 1 then
    self.items_per_page = math.floor(available_height / scale_by_size / 88) -- 64
  end

  local item_dimen
  local columns = self.grid_columns or 1
  if columns > 1 then
    local rows = G_reader_settings:readSetting("rakuyomi_grid_rows")
    if rows == nil or rows < 1 then
      rows = math.floor(available_height / scale_by_size / 240)
      if rows < 2 then rows = 2 end
    end

    if not self.portrait_mode then
      local portrait_available_height = Screen:getWidth() - self.others_height - Size.line.thin
      local portrait_rows = math.floor(portrait_available_height / scale_by_size / 240)
      if portrait_rows < 2 then portrait_rows = 2 end
      rows = Math.round(available_height / (portrait_available_height / portrait_rows))
    end
    self.perpage = rows * columns
    item_dimen = Geom:new {
      w = math.floor(self.inner_dimen.w / columns),
      h = math.floor(available_height / rows)
    }
  else
    if not self.portrait_mode then
      local portrait_available_height = Screen:getWidth() - self.others_height - Size.line.thin
      local portrait_item_height = math.floor(portrait_available_height / self.items_per_page) - Size.line.thin
      self.items_per_page = Math.round(available_height / portrait_item_height)
    end
  end

  Menu._recalculateDimen(self, no_recalculate_dimen)

  if item_dimen ~= nil then
    self.item_dimen = item_dimen
  end
end

return MenuCustom
