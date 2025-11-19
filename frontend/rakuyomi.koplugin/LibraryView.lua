-- FIXME make class names have _some_ kind of logic
local ConfirmBox = require("ui/widget/confirmbox")
local InputDialog = require("ui/widget/inputdialog")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Trapper = require("ui/trapper")
local _ = require("gettext")
local Icons = require("Icons")
local ButtonDialog = require("ui/widget/buttondialog")
local InstalledSourcesListing = require("InstalledSourcesListing")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local ChapterListing = require("ChapterListing")
local MangaSearchResults = require("MangaSearchResults")
local Menu = require("widgets/Menu")
local Settings = require("Settings")
local Testing = require("testing")
local UpdateChecker = require("UpdateChecker")
local ContinueReadingHandler = require("handlers/ContinueReadingHandler")
local calcLastReadText = require("utils/calcLastReadText")

local LoadingDialog = require("LoadingDialog")

local LibraryView = Menu:extend {
  name = "library_view",
  is_enable_shortcut = false,
  is_popout = false,
  title = "Library",
  with_context_menu = true,

  -- list of mangas in your library
  mangas = nil,
}

function LibraryView:init()
  self.mangas = self.mangas or {}
  self.title_bar_left_icon = "appbar.menu"
  self.onLeftButtonTap = function()
    self:openMenu()
  end
  self.width = Screen:getWidth()
  self.height = Screen:getHeight()

  local page = self.page
  Menu.init(self)
  self.page = page

  self.mangas_raw = self.mangas
  self.favorite_search_keyword = nil

  self:updateItems()
end

--- @private
function LibraryView:updateItems()
  if #self.mangas > 0 then
    self.item_table = self:generateItemTableFromMangas(self.mangas)
    self.multilines_show_more_text = false
    self.items_per_page = nil
  else
    self.item_table = self:generateEmptyViewItemTable()
    self.multilines_show_more_text = true
    self.items_per_page = 1
  end

  Menu.updateItems(self)
end

--- @private
--- @param mangas Manga[]
function LibraryView:generateItemTableFromMangas(mangas)
  local item_table = {}
  for _, manga in ipairs(mangas) do
    local mandatory = (manga.last_read and calcLastReadText(manga.last_read) .. " " or "")

    if manga.unread_chapters_count ~= nil and manga.unread_chapters_count > 0 then
      mandatory = (mandatory or "")
          .. Icons.FA_BELL .. manga.unread_chapters_count
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
function LibraryView:generateEmptyViewItemTable()
  return {
    {
      text = "No mangas found in library. Try adding some by holding their name on the search results!",
      dim = true,
      select_enabled = false,
    }
  }
end

function LibraryView:fetchAndShow()
  local response = Backend.getMangasInLibrary()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local mangas = response.body

  UIManager:show(LibraryView:new {
    mangas = mangas,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
    page = self.page
  })

  Testing:emitEvent('library_view_shown')
end

--- @private
function LibraryView:onPrimaryMenuChoice(item)
  Trapper:wrap(function()
    --- @type Manga
    local manga = item.manga

    local onReturnCallback = function()
      self:fetchAndShow()
    end

    ChapterListing:fetchAndShow(manga, onReturnCallback, true)
    self:onClose()
  end)
end

--- @private
function LibraryView:onContextMenuChoice(item)
  --- @type Manga
  local manga = item.manga
  local dialog_context_menu

  local context_menu_buttons = {
    {
      {
        text = Icons.REFRESHING .. " Refresh",
        callback = function()
          UIManager:close(dialog_context_menu)
          local response = self:_refreshManga(manga)


          local InfoMessage = require("ui/widget/infomessage")
          if response.type == 'ERROR' then
            UIManager:show(InfoMessage:new {
              text = response.message
            })
          else
            UIManager:show(InfoMessage:new {
              text = "Refreshed manga"
            })
          end

          UIManager:close(self)
          self:fetchAndShow()
        end
      }
    },
    {
      {
        text = "Continue Reading",
        callback = function()
          UIManager:close(dialog_context_menu)
          self:_handleContinueReading(manga)
        end,
      },
    },
    {
      {
        text = "Remove from Library",
        callback = function()
          UIManager:close(dialog_context_menu)
          self:_handleRemoveFromLibrary(manga)
        end,
      },
    },
  }
  dialog_context_menu = ButtonDialog:new {
    title = manga.title,
    buttons = context_menu_buttons,
  }
  UIManager:show(dialog_context_menu)
end

--- @private
function LibraryView:onSwipe(arg, ges_ev)
  local BD = require("ui/bidi")
  local direction = BD.flipDirectionIfMirroredUILayout(ges_ev.direction)
  if direction == "south" then
    self:refreshAllChapters()

    return
  end

  Menu.onSwipe(self, arg, ges_ev)
end

--- Handles "Continue Reading" action
--- @private
function LibraryView:_handleContinueReading(manga)
  local callbacks = {
    onReturn = function()
      self:fetchAndShow()
    end,
    onError = function(error_msg)
      ErrorDialog:show(error_msg)
    end,
    onChapterRead = function(chapter)
      Testing:emitEvent('chapter_read_from_library', {
        manga_id = manga.id,
        chapter_id = chapter.id
      })
    end
  }

  ContinueReadingHandler.handle(manga, self, callbacks)
end

--- @private
function LibraryView:_handleRemoveFromLibrary(manga)
  UIManager:show(ConfirmBox:new {
    text = "Do you want to remove \"" .. manga.title .. "\" from your library?",
    ok_text = "Remove",
    ok_callback = function()
      local response = Backend.removeMangaFromLibrary(manga.source.id, manga.id)

      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end
      self:fetchAndShow()
    end
  })
end

--- @private
function LibraryView:openMenu()
  local dialog

  local buttons = {
    {
      {
        text = Icons.FA_MAGNIFYING_GLASS .. " Search for mangas",
        callback = function()
          UIManager:close(dialog)

          self:openSearchMangasDialog()
        end
      },
    },
    {
      {
        text = Icons.REFRESHING .. " Refresh mangas",
        callback = function()
          UIManager:close(dialog)

          self:refreshAllChapters()
        end
      },
      {
        text = "\u{E644} Search favorites",
        callback = function()
          UIManager:close(dialog)

          self:openSearchFavoritesDialog()
        end
      }
    },
    {
      {
        text = "\u{e000} Cleaner chapters",
        callback = function()
          UIManager:close(dialog)

          self:openCleanerDialog()
        end
      },
      {
        text = Icons.FA_PLUG .. " Manage sources",
        callback = function()
          UIManager:close(dialog)

          self:openInstalledSourcesListing()
        end
      },
    },
    {
      {
        text = Icons.FA_GEAR .. " Settings",
        callback = function()
          UIManager:close(dialog)

          self:openSettings()
        end
      },
      {
        text = Icons.FA_ARROW_UP .. " Check for updates",
        callback = function()
          UIManager:close(dialog)

          UpdateChecker:checkForUpdates()
        end
      },
    },
    { {
      text = Icons.SYNC .. " Sync Database (Beta)",
      callback = function()
        Trapper:wrap(function()
          local response = LoadingDialog:showAndRun(
            "Sync to WebDAV...",
            function() return Backend.syncDatabase(false) end
          )

          if response.type == 'ERROR' then
            ErrorDialog:show(response.message)

            return
          end

          local InfoMessage = require("ui/widget/infomessage")

          if response.body == 'update_required' then
            UIManager:show(ConfirmBox:new {
              text = "The remote database is newer than the local one.\nDo you want to migrate your local database from the server?\n\nThis action cannot be undone.",
              ok_text = "Migrate",
              ok_callback = function()
                Trapper:wrap(function()
                  local response = LoadingDialog:showAndRun(
                    "Migrating database...",
                    function() return Backend.syncDatabase(true) end
                  )

                  if response.type == 'ERROR' then
                    ErrorDialog:show(response.message)

                    return
                  end

                  UIManager:show(InfoMessage:new {
                    text = "Local database has been migrated from the server!"
                  })

                  UIManager:close(self)
                  UIManager:close(dialog)
                  self:fetchAndShow()
                end)
              end,
              other_buttons = {
                {
                  {
                    text = "Replace Cloud",
                    callback = function()
                      Trapper:wrap(function()
                        local response = LoadingDialog:showAndRun(
                          "Replacing cloud...",
                          function() return Backend.syncDatabase(false, true) end
                        )
                        if response.type == 'ERROR' then
                          ErrorDialog:show(response.message)

                          return
                        end

                        UIManager:show(InfoMessage:new {
                          text = "Cloud database has been forcedly replaced with local one!"
                        })

                        UIManager:close(self)
                        UIManager:close(dialog)
                        self:fetchAndShow()
                      end)
                    end,
                  }
                }
              }
            })

            return
          end

          local msg = '';
          if response.body == 'up_to_date' then
            msg = "Database is already up to date!"
          elseif response.body == 'updated_to_server' then
            msg = "Database has been synced to the server!"
          elseif response.body == 'updated' then
            msg = "Local database has been migrated from the server!"
          else
            msg = "Sync completed!"
          end

          UIManager:show(InfoMessage:new {
            text = msg
          })
        end)
      end
    } }
  }

  dialog = ButtonDialog:new {
    buttons = buttons,
  }

  UIManager:show(dialog)

  Testing:emitEvent('library_view_menu_opened')
end

--- @private
function LibraryView:openSearchMangasDialog()
  local dialog
  dialog = InputDialog:new {
    title = _("Manga search..."),
    input_hint = _("Houseki no Kuni"),
    description = _("Type the manga name to search for"),
    buttons = {
      {
        {
          text = _("Cancel"),
          id = "close",
          callback = function()
            UIManager:close(dialog)
          end,
        },
        {
          text = _("Search"),
          is_enter_default = true,
          callback = function()
            UIManager:close(dialog)

            self:searchMangas(dialog:getInputText())
          end,
        },
      }
    }
  }

  UIManager:show(dialog)
  dialog:onShowKeyboard()
end

--- @private
function LibraryView:openSearchFavoritesDialog()
  local dialog
  dialog = InputDialog:new {
    title = _("Favorite search..."),
    input = self.favorite_search_keyword,
    input_hint = _("Tonikaku Kawaii"),
    description = _("Type the manga name to search for"),
    buttons = {
      {
        {
          text = _("Cancel"),
          id = "close",
          callback = function()
            UIManager:close(dialog)
          end,
        },
        {
          text = _("Search"),
          is_enter_default = true,
          callback = function()
            UIManager:close(dialog)

            local query = dialog:getInputText()

            query = query and query:match("^%s*(.-)%s*$"):lower()

            local mangas = {}

            if query and query ~= "" then
              for _, manga in ipairs(self.mangas_raw) do
                -- convert manga title to lowercase for comparison
                local title = (manga.title or ""):lower()
                if title:find(query, 1, true) then
                  table.insert(mangas, manga)
                end
              end
            else
              mangas = self.mangas_raw
            end

            self.mangas = mangas
            self.favorite_search_keyword = dialog:getInputText()

            self:updateItems()
          end,
        },
      }
    }
  }

  UIManager:show(dialog)
  dialog:onShowKeyboard()
end

function LibraryView:startCleaner(modeInvalid)
  Trapper:wrap(function()
    local response = LoadingDialog:showAndRun(
      "Scaning files...",
      function() return Backend.findOrphanOrReadFiles(modeInvalid) end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    local filenames = response.body.filenames or {}
    local total_size = response.body.total_text

    local confirm = ConfirmBox:new {
      text = string.format(
        "Found %d files.\n\nTotal size %s.\n\nOnly file .cbz and .epub scan.",
        #filenames,
        total_size
      ),
      ok_text = "Clean",
      ok_callback = function()
        local ProgressbarDialog = require("ui/widget/progressbardialog")
        local InfoMessage = require("ui/widget/infomessage")

        local progressbar_dialog = ProgressbarDialog:new {
          title = "Deleting...",
          progress_max = #filenames
        }
        UIManager:show(progressbar_dialog)

        for i, filename in ipairs(filenames) do
          local response = Backend.removeFile(filename)
          if response.type == 'ERROR' then
            ErrorDialog:show(response.message)
            return
          end
          progressbar_dialog:reportProgress(i + 1)
          progressbar_dialog:redrawProgressbarIfNeeded()
        end

        progressbar_dialog:close()

        UIManager:show(InfoMessage:new {
          text = string.format("Cleaned free %s storage", total_size)
        })
      end
    }

    UIManager:show(confirm)
  end
  )
end

--- @private
function LibraryView:_refreshManga(manga)
  local response = Backend.refreshChapters(manga.source.id, manga.id)
  return response
end

--- @private
function LibraryView:refreshAllChapters()
  local ProgressbarDialog = require("ui/widget/progressbardialog")
  local InfoMessage = require("ui/widget/infomessage")

  Trapper:wrap(function()
    local progressbar_dialog = ProgressbarDialog:new {
      title = "Refresh mangas...",
      progress_max = #self.mangas_raw
    }
    UIManager:show(progressbar_dialog)
    local errors = {}

    for i, manga in ipairs(self.mangas_raw) do
      local response = self:_refreshManga(manga)

      if response.type == 'ERROR' then
        table.insert(errors, {
          id = manga.id,
          title = manga.title,
          source = manga.source.id,
          message = response.message
        })
      end

      progressbar_dialog:reportProgress(i + 1)
      progressbar_dialog:redrawProgressbarIfNeeded()
    end

    UIManager:close(self)
    self:fetchAndShow()

    progressbar_dialog:close()

    if #errors > 0 then
      local msg = "Some manga updates fail:\n\n"
      for _, err in ipairs(errors) do
        msg = msg .. string.format("- [%s] (%s): %s\n", err.source, err.title, err.message)
      end
      ErrorDialog:show(msg)
    else
      UIManager:show(InfoMessage:new {
        text = "All chapters manga updated!"
      })
    end
  end)
end

--- @private
function LibraryView:openCleanerDialog()
  local dialog

  dialog = ConfirmBox:new {
    text = "Cleaner\n\n" ..
        "Normal: Find and delete invalid files including files from deleted sources\n\n" ..
        "Chapter read done: Find and delete chapters that have been read\n\n" ..
        "IMPORTANT: Meta files (bookmark, history) not keep!",
    ok_text = "Normal",
    ok_callback = function()
      self:startCleaner(true)
    end,
    other_buttons = { {
      {
        text = "Chapter read done",
        callback = function()
          self:startCleaner(false)
        end
      }
    }
    } }

  UIManager:show(dialog)
end

--- @private
function LibraryView:searchMangas(search_text)
  Trapper:wrap(function()
    local onReturnCallback = function()
      self:fetchAndShow()
    end

    MangaSearchResults:searchAndShow(search_text, onReturnCallback)

    self:onClose()
  end)
end

--- @private
function LibraryView:openInstalledSourcesListing()
  Trapper:wrap(function()
    local onReturnCallback = function()
      self:fetchAndShow()
    end

    InstalledSourcesListing:fetchAndShow(onReturnCallback)

    self:onClose()
  end)
end

--- @private
function LibraryView:openSettings()
  Trapper:wrap(function()
    local onReturnCallback = function()
      self:fetchAndShow()
    end

    Settings:fetchAndShow(onReturnCallback)

    self:onClose()
  end)
end

return LibraryView
