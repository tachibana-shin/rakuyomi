local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local UIManager = require("ui/uimanager")
local InfoMessage = require("ui/widget/infomessage")
local _ = require("gettext+")
local PlaylistDialog = require("PlaylistDialog")

--- @param manga Manga
local function addToPlaylist(manga)
  PlaylistDialog:fetchAndShow(function(playlist)
    local r = Backend.addMangaToPlaylist(playlist.id, manga.source.id, manga.id)
    if r.type == 'ERROR' then
      ErrorDialog:show(r.message)
      return
    end
    UIManager:show(InfoMessage:new {
      text = _("Added to") .. " \"" .. playlist.name .. "\""
    })
  end)
end

return addToPlaylist
