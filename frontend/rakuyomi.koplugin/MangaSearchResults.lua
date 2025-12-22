local ButtonDialog = require("ui/widget/buttondialog")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Trapper = require("ui/trapper")
local InfoMessage = require("ui/widget/infomessage")
local _ = require("gettext")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")
local LoadingDialog = require("LoadingDialog")
local ChapterListing = require("ChapterListing")
local Testing = require("testing")
local Icons = require("Icons")
local MangaInfoWidget = require("MangaInfoWidget")
local calcLastReadText = require("utils/calcLastReadText")

--- @class MangaSearchResults: { [any]: any }
--- @field results Manga[]
--- @field on_return_callback fun(): nil
local MangaSearchResults = Menu:extend {
  name = "manga_search_results",
  is_enable_shortcut = false,
  is_popout = false,
  title = "Search results...",
  with_context_menu = true,

  -- list of mangas
  results = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
}

function MangaSearchResults:init()
  self.results = self.results or {}
  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  local page = self.page
  Menu.init(self)
  self.page = page

  -- see `ChapterListing` for an explanation on this
  -- FIXME we could refactor this into a single class
  self.paths = { 0 }
  self.on_return_callback = nil

  self:updateItems()
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

  Menu.updateItems(self)
end

--- Generates the item table for displaying the search results.
--- @private
--- @param results Manga[]
--- @return table
function MangaSearchResults:generateItemTableFromSearchResults(results)
  local item_table = {}
  for _, manga in ipairs(results) do
    local mandatory = (manga.last_read and calcLastReadText(manga.last_read) .. " " or "")

    if manga.unread_chapters_count ~= nil and manga.unread_chapters_count > 0 then
      mandatory = (mandatory or "") .. Icons.FA_BELL .. manga.unread_chapters_count
    end

    if manga.in_library then
      mandatory = (mandatory or "") .. Icons.COD_LIBRARY
    end

    table.insert(item_table, {
      manga = manga,
      text = manga.title,
      post_text = manga.source.name,
      mandatory = mandatory,
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
    return "No errors"
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
    table.insert(lines, string.format("… and %d more errors", #errors - max_items))
  end

  return table.concat(lines, "\n")
end
--- Searches for mangas and shows the results.
--- @param search_text string The text to be searched for.
--- @param onReturnCallback any
--- @return boolean
function MangaSearchResults:searchAndShow(search_text, onReturnCallback)
  local cancel_id = Backend.createCancelId()
  local response, cancelled = LoadingDialog:showAndRun(
    "Searching for \"" .. search_text .. "\"",
    function() return Backend.searchMangas(cancel_id, search_text) end,
    function()
      Backend.cancel(cancel_id)
      local InfoMessage = require("ui/widget/infomessage")

      local cancelledMessage = InfoMessage:new {
        text = "Search cancelled.",
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
        text = Icons.FA_BELL .. _(" Add to Library"),
        callback = function()
          UIManager:close(dialog)

          local _, err = Backend.addMangaToLibrary(manga.source.id, manga.id)

          if err ~= nil then
            ErrorDialog:show(err)

            return
          end

          manga.in_library = true
          self:updateItems()

          Testing:emitEvent("manga_added_to_library", {
            source_id = manga.source.id,
            manga_id = manga.id,
          })
        end
      },
    },
    {
      {
        text = Icons.INFO .. " " .. _("Details"),
        callback = function()
          UIManager:close(dialog)

          local onReturnCallback = function()
            local ui = MangaSearchResults:new {
              results = self.results,
              on_return_callback = self.onReturnCallback,
              covers_fullscreen = true, -- hint for UIManager:_repaint()
              page = self.page
            }
            ui.on_return_callback = self.onReturnCallback
            UIManager:show(ui)
          end
          MangaInfoWidget:fetchAndShow(manga, function()
            UIManager:close(self)
          end, onReturnCallback)
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
