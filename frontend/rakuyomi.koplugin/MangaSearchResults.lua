local ConfirmBox = require("ui/widget/confirmbox")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Trapper = require("ui/trapper")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")
local LoadingDialog = require("LoadingDialog")
local ChapterListing = require("ChapterListing")
local Testing = require("testing")
local Icons = require("Icons")
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
  self.paths = {
    { callback = self.on_return_callback },
  }
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
  local path = table.remove(self.paths)

  self:onClose()
  path.callback()
end

--- Searches for mangas and shows the results.
--- @param search_text string The text to be searched for.
--- @param onReturnCallback any
function MangaSearchResults:searchAndShow(search_text, onReturnCallback)
  local cancel = false
  local response = LoadingDialog:showAndRun(
    "Searching for \"" .. search_text .. "\"",
    function() return Backend.searchMangas(search_text) end,
    function()
      local InfoMessage = require("ui/widget/infomessage")

      local cancelledMessage = InfoMessage:new {
        text = "Search cancelled.",
      }
      UIManager:show(cancelledMessage)

      cancel = true
    end
  )

  if cancel then
    return
  end

  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local results = response.body
  print(require('json').encode(results))

  local ui = MangaSearchResults:new {
    results = results,
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
    page = self.page
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("manga_search_results_shown")
end

--- @private
function MangaSearchResults:onPrimaryMenuChoice(item)
  Trapper:wrap(function()
    --- @type Manga
    local manga = item.manga

    local onReturnCallback = function()
      UIManager:show(self)
    end

    ChapterListing:fetchAndShow(manga, onReturnCallback)

    UIManager:close(self)
  end)
end

--- @private
function MangaSearchResults:onContextMenuChoice(item)
  --- @type Manga
  local manga = item.manga
  UIManager:show(ConfirmBox:new {
    text = "Do you want to add \"" .. manga.title .. "\" to your library?",
    ok_text = "Add",
    ok_callback = function()
      local _, err = Backend.addMangaToLibrary(manga.source.id, manga.id)

      if err ~= nil then
        ErrorDialog:show(err)

        return
      end

      Testing:emitEvent("manga_added_to_library", {
        source_id = manga.source.id,
        manga_id = manga.id,
      })
    end
  })
end

return MangaSearchResults
