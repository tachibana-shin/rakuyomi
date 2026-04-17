local ButtonDialog = require("ui/widget/buttondialog")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local InfoMessage = require("ui/widget/infomessage")
local _ = require("gettext+")
local addToPlaylist = require("handlers/addToPlaylist")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")
local MenuCustom = require("patch/MenuCustom")
local MenuItemCover = require("patch/MenuItemCover")
local MenuItemGrid = require("patch/MenuItemGrid")
local LoadingDialog = require("LoadingDialog")
local ChapterListing = require("ChapterListing")
local Testing = require("testing")
local Icons = require("Icons")
local MangaInfoWidget = require("MangaInfoWidget")
local calcLastReadText = require("utils/calcLastReadText")
local Trapper = require("ui/trapper")

--- @class MangaSearchResults: { [any]: any }
--- @field results Manga[]
--- @field on_return_callback fun(): nil
local MangaSearchResults = MenuCustom:extend {
  name = "manga_search_results",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Search results..."),
  with_context_menu = true,

  results = nil,
  on_return_callback = nil,
}

function MangaSearchResults:init()
  -- Stash results before Menu.init so its internal updateItems() call sees an
  -- empty list. Without this, covers are loaded once inside Menu.init and again
  -- in our explicit updateItems() below, exhausting the LRU image cache.
  local results = self.results or {}
  self.results = {}
  self.search_view_mode = G_reader_settings:readSetting("rakuyomi_search_view_mode", "base")

  self.title_bar_left_icon = "column.two"
  self.onLeftButtonTap = function()
    self:cycleViewMode()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  local page = self.page
  Menu.init(self)
  MenuCustom.init(self)
  self.page = page

  self.paths = { 0 }
  self.on_return_callback = nil
  self.results = results
  self:updateItems()
end

function MangaSearchResults:cycleViewMode()
  local modes = { "base", "cover", "grid" }
  local next_mode = "base"
  for i, mode in ipairs(modes) do
    if mode == self.search_view_mode then
      next_mode = modes[(i % #modes) + 1]
      break
    end
  end
  self.search_view_mode = next_mode
  G_reader_settings:saveSetting("rakuyomi_search_view_mode", next_mode)
  self:updateItems()
  Testing:emitEvent("search_view_mode_changed", { mode = next_mode })
end

function MangaSearchResults:_recalculateDimen(flag)
  if self.search_view_mode ~= "base" then
    MenuCustom._recalculateDimen(self, flag)
  else
    Menu._recalculateDimen(self, flag)
  end
end

function MangaSearchResults:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- Updates the menu item contents with the manga information
--- @private
function MangaSearchResults:updateItems()
  self.item_table = self:generateItemTableFromSearchResults(self.results)

  local mode = self.search_view_mode
  if mode == "grid" then
    self.grid_columns = G_reader_settings:readSetting("rakuyomi_grid_columns") or 3
    MenuCustom.updateItems(self, MenuItemGrid)
  elseif mode == "cover" then
    self.grid_columns = nil
    MenuCustom.updateItems(self, MenuItemCover)
  else
    self.grid_columns = nil
    Menu.updateItems(self)
  end
end

--- Generates the item table for displaying the search results.
--- @private
--- @param results Manga[]
--- @return table
function MangaSearchResults:generateItemTableFromSearchResults(results)
  local item_table = {}
  local is_cover = self.search_view_mode == "cover"

  for _, manga in ipairs(results) do
    local mandatory = (manga.last_read and calcLastReadText(manga.last_read) .. " " or "")

    if manga.unread_chapters_count ~= nil and manga.unread_chapters_count > 0 then
      mandatory = mandatory .. Icons.FA_BELL .. manga.unread_chapters_count
    end

    if manga.in_library then
      mandatory = mandatory .. Icons.COD_LIBRARY
    end

    table.insert(item_table, {
      manga = manga,
      text = manga.title,
      post_text = is_cover and mandatory or manga.source.name,
      manga_cover = self.search_view_mode ~= "base" and manga.manga_cover or nil,
      mandatory = not is_cover and mandatory or nil,
    })
  end

  return item_table
end

--- @private
function MangaSearchResults:onReturn()
  table.remove(self.paths)

  self:onClose()
end

--- @param errors SearchError[]
local function formatSearchErrors(errors)
  if not errors or #errors == 0 then
    return _("No errors")
  end

  local max_items = 5
  local lines = {}

  for i = 1, math.min(#errors, max_items) do
    local err = errors[i]
    table.insert(lines, string.format(
      "%s | %s",
      err.source_id,
      err.reason
    ))
  end

  if #errors > max_items then
    table.insert(lines, string.format(_("… and %d more errors"), #errors - max_items))
  end

  return table.concat(lines, "\n")
end
--- Searches for mangas and shows the results.
--- @param search_text string The text to be searched for.
--- @param exclude string[]
--- @param onReturnCallback any
--- @return boolean
function MangaSearchResults:searchAndShow(search_text, exclude, onReturnCallback)
  local cancel_id = Backend.createCancelId()
  local response, cancelled = LoadingDialog:showAndRun(
    _("Searching for") .. " \"" .. search_text .. "\"",
    function() return Backend.searchMangas(cancel_id, search_text, exclude) end,
    function()
      Backend.cancel(cancel_id)
      local InfoMessage = require("ui/widget/infomessage")

      local cancelledMessage = InfoMessage:new {
        text = _("Search cancelled."),
      }
      UIManager:show(cancelledMessage)
    end
  )

  if cancelled then
    return false
  end

  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return false
  end

  local results = response.body[1]

  local ui = MangaSearchResults:new {
    results = results,
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
    page = self.page
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)
  if #response.body[2] > 0 then
    UIManager:show(InfoMessage:new {
      text = formatSearchErrors(response.body[2])
    })
  end


  Testing:emitEvent("manga_search_results_shown")

  return true
end

--- @private
function MangaSearchResults:onPrimaryMenuChoice(item)
  Trapper:wrap(function()
    --- @type Manga
    local manga = item.manga

    local onReturnCallback = function()
      UIManager:show(self)
    end

    if ChapterListing:fetchAndShow(manga, onReturnCallback) then
      UIManager:close(self)
    end
  end)
end

--- @private
function MangaSearchResults:onContextMenuChoice(item)
  --- @type Manga
  local manga = item.manga

  local dialog
  local buttons = {
    {
      {
        text_func = function()
          if manga.in_library then
            return Icons.FA_BELL .. " " .. _("Remove from Library")
          end

          return Icons.FA_BELL .. " " .. _("Add to Library")
        end,
        callback = function()
          UIManager:close(dialog)

          Trapper:wrap(function()
            --- @type ErrorResponse
            local err = nil
            if manga.in_library then
              err = Backend.removeMangaFromLibrary(manga.source.id, manga.id)
            else
              err = Backend.addMangaToLibrary(manga.source.id, manga.id)
            end

            if err.type == 'ERROR' then
              ErrorDialog:show(err.message)

              return
            end

            local added = manga.in_library
            manga.in_library = not added

            if manga.in_library and self.search_view_mode ~= 'base' then
              local cancel_id = Backend.createCancelId()
              local response, cancelled = LoadingDialog:showAndRun(
                _("Refreshing details..."),
                function() return Backend.refreshMangaDetails(cancel_id, manga.source.id, manga.id) end,
                function()
                  Backend.cancel(cancel_id)
                  local InfoMessage = require("ui/widget/infomessage")

                  local cancelledMessage = InfoMessage:new {
                    text = _("Refresh details cancelled."),
                  }
                  UIManager:show(cancelledMessage)
                end
              )

              if cancelled then
                return false
              end

              if response.type == 'ERROR' then
                ErrorDialog:show(response.message)

                return false
              end
            end

            self:updateItems()

            Testing:emitEvent(added and "manga_removed_from_library" or "manga_added_to_library", {
              source_id = manga.source.id,
              manga_id = manga.id,
            })
          end)
        end
      },
    },
    {
      {
        text = Icons.FA_PLUS .. " " .. _("Add to Playlist"),
        callback = function()
          UIManager:close(dialog)
          addToPlaylist(manga)
        end
      }
    },
    {
      {
        text = Icons.INFO .. " " .. _("Details"),
        callback = function()
          UIManager:close(dialog)

          Trapper:wrap(function()
            local onReturnCallback = function()
              local ui = MangaSearchResults:new {
                results = self.results,
                on_return_callback = self.on_return_callback,
                covers_fullscreen = true, -- hint for UIManager:_repaint()
                page = self.page
              }
              ui.on_return_callback = self.on_return_callback
              UIManager:show(ui)
            end
            MangaInfoWidget:fetchAndShow(manga, onReturnCallback)
            UIManager:close(self)
          end)
        end
      }
    },
  }

  dialog = ButtonDialog:new {
    buttons = buttons,
  }

  UIManager:show(dialog)
end

return MangaSearchResults
