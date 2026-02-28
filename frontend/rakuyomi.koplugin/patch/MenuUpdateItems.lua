local UIManager = require("ui/uimanager")
local Menu = require("ui/widget/menu")

--- @param menu any
--- @param MenuItem any
--- @param select_number number|nil
--- @param no_recalculate_dimen boolean|nil
local function MenuUpdateItems(menu, MenuItem, select_number, no_recalculate_dimen)
  local old_dimen = menu.dimen and menu.dimen:copy()
  -- self.layout must be updated for focusmanager
  menu.layout = {}
  menu.item_group:clear()
  menu.page_info:resetLayout()
  menu.return_button:resetLayout()
  menu.content_group:resetLayout()
  menu:_recalculateDimen(no_recalculate_dimen)

  local items_nb -- number of items in the visible page
  local idx_offset, multilines_show_more_text
  if menu.items_max_lines then
    items_nb = #menu.page_items[menu.page]
  else
    items_nb = menu.perpage
    idx_offset = (menu.page - 1) * items_nb
    multilines_show_more_text = menu.multilines_show_more_text
    if multilines_show_more_text == nil then
      multilines_show_more_text = G_reader_settings:isTrue("items_multilines_show_more_text")
    end
  end

  print(items_nb)

  for idx = 1, items_nb do
    local index = menu.items_max_lines and menu.page_items[menu.page][idx] or idx_offset + idx
    local item = menu.item_table[index]
    if item == nil then break end
    item.idx = index                 -- index is valid only for items that have been displayed
    if index == menu.itemnumber then -- focused item
      select_number = idx
    end
    local item_shortcut, shortcut_style
    if menu.is_enable_shortcut then
      item_shortcut = menu.item_shortcuts[idx]
      -- give different shortcut_style to keys in different lines of keyboard
      shortcut_style = (idx < 11 or idx > 20) and "square" or "grey_square"
    end

    local item_tmp = MenuItem:new {
      idx = index,
      show_parent = menu.show_parent,
      state_w = menu.state_w,
      text = Menu.getMenuText(item),
      text_bgcolor = item.text_bgcolor,
      bidi_wrap_func = item.bidi_wrap_func,
      post_text = item.post_text,
      mandatory = item.mandatory,
      mandatory_func = item.mandatory_func,
      mandatory_dim = item.mandatory_dim or item.dim,
      mandatory_dim_func = item.mandatory_dim_func,
      bold = menu.item_table.current == index or item.bold == true,
      dim = item.dim,
      font_size = menu.font_size,
      infont_size = menu.items_mandatory_font_size or (menu.font_size - 4),
      dimen = menu.item_dimen:copy(),
      shortcut = item_shortcut,
      shortcut_style = shortcut_style,
      entry = item,
      menu = menu,
      linesize = menu.linesize,
      single_line = menu.single_line,
      multilines_forced = menu.multilines_forced,
      multilines_show_more_text = multilines_show_more_text,
      items_max_lines = menu.items_max_lines,
      truncate_left = menu.truncate_left,
      align_baselines = menu.align_baselines,
      with_dots = menu.with_dots,
      line_color = menu.line_color,
      items_padding = menu.items_padding,
      handle_hold_on_hold_release = menu.handle_hold_on_hold_release,
    }
    table.insert(menu.item_group, item_tmp)
    -- this is for focus manager
    table.insert(menu.layout, { item_tmp })
  end

  ---@diagnostic disable-next-line: redundant-parameter
  menu:updatePageInfo(select_number)
  menu:mergeTitleBarIntoLayout()

  UIManager:setDirty(menu.show_parent, function()
    local refresh_dimen =
        old_dimen and old_dimen:combine(menu.dimen)
        or menu.dimen
    return "ui", refresh_dimen
  end)
end

return MenuUpdateItems
