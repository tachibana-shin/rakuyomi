-- FIXME make class names have _some_ kind of logic
local ConfirmBox = require("ui/widget/confirmbox")
local InputDialog = require("ui/widget/inputdialog")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Trapper = require("ui/trapper")
local _ = require("gettext+")
local Icons = require("Icons")
local ButtonDialog = require("ui/widget/buttondialog")
local InstalledSourcesListing = require("InstalledSourcesListing")
local IconButton = require("ui/widget/iconbutton")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local Button = require("ui/widget/button")
local Font = require("ui/font")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local InfoMessage = require("ui/widget/infomessage")

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
local findEntries = require("utils/findEntries")
local NotificationView = require("NotificationView")
local RadioButtonWidget = require("ui/widget/radiobuttonwidget")

local LoadingDialog = require("LoadingDialog")
local MangaInfoWidget = require("MangaInfoWidget")
local CheckboxDialog = require("CheckboxDialog")

local DGENERIC_ICON_SIZE = G_defaults:readSetting("DGENERIC_ICON_SIZE")
local SMALL_FONT_FACE = Font:getFace("smallffont")
local LibraryView = Menu:extend {
  name = "library_view",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Library"),
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

  self:patchTitleBar(0)
  self:fetchCountNotification()

  self:updateItems()
end

--- @private
function LibraryView:fetchCountNotification()
  local response = Backend.getCountNotification()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local count_notify = response.body
  self:patchTitleBar(count_notify)

  UIManager:setDirty(self.show_parent, "ui", self.dimen)
end

--- @private
--- @param count_notify number
function LibraryView:patchTitleBar(count_notify)
  -- custom
  local left_icon_size_ratio = self.title_bar.left_icon_size_ratio
  local right_icon_size_ratio = self.title_bar.right_icon_size_ratio
  local right_icon_rotation_angle = self.title_bar.right_icon_rotation_angle

  local left_icon_size = Screen:scaleBySize(DGENERIC_ICON_SIZE * left_icon_size_ratio)
  local right_icon_size = Screen:scaleBySize(DGENERIC_ICON_SIZE * right_icon_size_ratio)
  local button_padding = Screen:scaleBySize(11)

  self.title_bar.left_button = HorizontalGroup:new {
    IconButton:new {
      icon = "appbar.settings",
      icon_rotation_angle = self.left_icon_rotation_angle,
      width = left_icon_size,
      height = left_icon_size,
      padding = button_padding,
      padding_bottom = left_icon_size,
      callback = self.title_bar.left_icon_tap_callback,
      hold_callback = self.title_bar.left_icon_hold_callback,
      allow_flash = self.title_bar.left_icon_allow_flash,
      show_parent = self.title_bar.show_parent,
    },
    IconButton:new {
      icon = "align.center",
      width = left_icon_size,
      height = left_icon_size,
      padding = button_padding,
      padding_bottom = right_icon_size,
      padding_right = 2 * left_icon_size,
      callback = function()
        Trapper:wrap(function()
          local response = Backend.getSettings()
          if response.type == 'ERROR' then
            ErrorDialog:show(response.message)
          end

          local settings = response.body

          local key = "library_sorting_mode"
          local tuple = findEntries(Settings.setting_value_definitions, key)

          local radio_buttons = {}
          for _, option in ipairs(tuple.options) do
            table.insert(radio_buttons, {
              {
                text = option.label,
                provider = option.value,
                checked = settings[key] == option.value,
              },
            })
          end

          local dialog
          dialog = RadioButtonWidget:new {
            title_text = tuple.title,
            radio_buttons = radio_buttons,
            callback = function(radio)
              UIManager:close(dialog)

              settings[key] = radio.provider

              local response = Backend.setSettings(settings)
              if response.type == 'ERROR' then
                ErrorDialog:show(response.message)
                return
              end

              local response = Backend.getMangasInLibrary()
              if response.type == 'ERROR' then
                ErrorDialog:show(response.message)

                return
              end

              local mangas = response.body

              self.mangas_raw = mangas
              self.favorite_search_keyword = nil
              self.mangas = mangas

              self:updateItems()

              UIManager:show(dialog)
            end
          }

          UIManager:show(dialog)
        end)
      end
    },
  }

  self.title_bar.right_button = HorizontalGroup:new {
    HorizontalSpan:new {
      width = Screen:getWidth() - button_padding - right_icon_size - button_padding * 2 - right_icon_size - button_padding * 2 - right_icon_size - button_padding, -- extend button tap zone
    },
    VerticalGroup:new {
      Button:new {
        text = Icons.FA_BELL .. count_notify,
        face = SMALL_FONT_FACE,
        bordersize = 0,
        enabled = true,
        text_font_size = 16,
        text_font_bold = false,
        callback = function()
          Trapper:wrap(function()
            local onReturnCallback = function()
              self:fetchAndShow()
            end

            NotificationView:fetchAndShow(onReturnCallback)

            self:onClose()
          end)
        end
      },
      VerticalSpan:new {
        width = right_icon_size / 2
      }
    },
    IconButton:new {
      icon = "appbar.search",
      width = right_icon_size,
      height = right_icon_size,
      padding = button_padding,
      padding_bottom = right_icon_size,
      callback = function()
        self:openSearchMangasDialog()
      end,
    },
    IconButton:new {
      icon = "close",
      icon_rotation_angle = right_icon_rotation_angle,
      width = right_icon_size,
      height = right_icon_size,
      padding = button_padding,
      padding_bottom = right_icon_size,
      callback = self.title_bar.right_icon_tap_callback,
      hold_callback = self.title_bar.right_icon_hold_callback,
    },
  }
  --- [1] title
  --- [2] left button
  --- [3] right button
  if self.title_bar[2] ~= nil then
    self.title_bar[2] = self.title_bar.left_button
  end
  if self.title_bar[3] ~= nil then
    self.title_bar[3] = self.title_bar.right_button
  end
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
      text = _("No mangas found in library") .. ". " .. _("Try adding some by holding their name on the search results!"),
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

    if ChapterListing:fetchAndShow(manga, onReturnCallback, true) then
      self:onClose()
    end
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
        text = Icons.REFRESHING .. " " .. _("Refresh"),
        callback = function()
          UIManager:close(dialog_context_menu)
          local response = self:_refreshManga(Backend.createCancelId(), manga)

          if response.type == 'ERROR' then
            UIManager:show(InfoMessage:new {
              text = response.message
            })
          else
            UIManager:show(InfoMessage:new {
              text = _("Refreshed manga")
            })
          end

          self:fetchAndShow()
          UIManager:close(self)
        end
      },
      {
        text = Icons.INFO .. " " .. _("Details"),
        callback = function()
          UIManager:close(dialog_context_menu)

          Trapper:wrap(function()
            local onReturnCallback = function()
              self:fetchAndShow()
            end
            MangaInfoWidget:fetchAndShow(manga, onReturnCallback)
            UIManager:close(self)
          end)
        end
      }
    },
    {
      {
        text = Icons.CHECK_ALL .. " " .. _("Mark read"),
        callback = function()
          UIManager:close(dialog_context_menu)

          ChapterListing:openMarkDialog(manga, true, function(count)
            manga.unread_chapters_count = count
            self:updateItems()
          end)
        end
      },
      {
        text = Icons.CHECK_ALL .. " " .. _("Mark unread"),
        callback = function()
          UIManager:close(dialog_context_menu)

          ChapterListing:openMarkDialog(manga, false, function(count)
            manga.unread_chapters_count = count
            self:updateItems()
          end)
        end
      }
    },
    {
      {
        text = _("Continue Reading"),
        callback = function()
          UIManager:close(dialog_context_menu)
          self:_handleContinueReading(manga)
        end,
      },
    },
    {
      {
        text = _("Remove from Library"),
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
  self:onClose()
end

--- @private
function LibraryView:_handleRemoveFromLibrary(manga)
  UIManager:show(ConfirmBox:new {
    text = _("Do you want to remove") .. "\" " .. manga.title .. "\" " .. _("from your library?"),
    ok_text = _("Remove"),
    ok_callback = function()
      local response = Backend.removeMangaFromLibrary(manga.source.id, manga.id)

      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end
      self:fetchAndShow()
      self:onClose()
    end
  })
end

--- @private
function LibraryView:openMenu()
  local dialog

  local buttons = {
    {
      {
        text = Icons.FA_MAGNIFYING_GLASS .. " " .. _("Search for mangas"),
        callback = function()
          UIManager:close(dialog)

          self:openSearchMangasDialog()
        end
      },
    },
    {
      {
        text = Icons.REFRESHING .. " " .. _("Refresh mangas"),
        callback = function()
          UIManager:close(dialog)

          self:refreshAllChapters()
        end
      },
      {
        text = "\u{E644}" .. " " .. _("Search favorites"),
        callback = function()
          UIManager:close(dialog)

          self:openSearchFavoritesDialog()
        end
      }
    },
    {
      {
        text = "\u{e000}" .. " " .. _("Cleaner chapters"),
        callback = function()
          UIManager:close(dialog)

          self:openCleanerDialog()
        end
      },
      {
        text = Icons.FA_PLUG .. " " .. _("Manage sources"),
        callback = function()
          UIManager:close(dialog)

          self:openInstalledSourcesListing()
        end
      },
    },
    {
      {
        text = Icons.FA_GEAR .. " " .. _("Settings"),
        callback = function()
          UIManager:close(dialog)

          self:openSettings()
        end
      },
      {
        text = Icons.FA_ARROW_UP .. " " .. _("Check for updates"),
        callback = function()
          UIManager:close(dialog)

          UpdateChecker:checkForUpdates()
        end
      },
    },
    { {
      text = Icons.SYNC .. " " .. _("Sync Database (Beta)"),
      callback = function()
        Trapper:wrap(function()
          local response = LoadingDialog:showAndRun(
            _("Sync to WebDAV..."),
            function() return Backend.syncDatabase(false, false) end
          )

          if response.type == 'ERROR' then
            ErrorDialog:show(response.message)

            return
          end

          if response.body == 'update_required' then
            UIManager:show(ConfirmBox:new {
              text = _("The remote database is newer than the local one.") .. "\n" .. _("Do you want to migrate your local database from the server?") .. "\n\n" .. _("This action cannot be undone."),
              ok_text = _("Migrate"),
              ok_callback = function()
                Trapper:wrap(function()
                  local response = LoadingDialog:showAndRun(
                    _("Migrating database..."),
                    function() return Backend.syncDatabase(true, false) end
                  )

                  if response.type == 'ERROR' then
                    ErrorDialog:show(response.message)

                    return
                  end

                  UIManager:show(InfoMessage:new {
                    text = _("Local database has been migrated from the server!")
                  })

                  UIManager:close(self)
                  UIManager:close(dialog)
                  self:fetchAndShow()
                end)
              end,
              other_buttons = {
                {
                  {
                    text = _("Replace Cloud"),
                    callback = function()
                      Trapper:wrap(function()
                        local response = LoadingDialog:showAndRun(
                          _("Replacing cloud..."),
                          function() return Backend.syncDatabase(false, true) end
                        )
                        if response.type == 'ERROR' then
                          ErrorDialog:show(response.message)

                          return
                        end

                        UIManager:show(InfoMessage:new {
                          text = _("Cloud database has been forcedly replaced with local one!")
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
            msg = _("Database is already up to date!")
          elseif response.body == 'updated_to_server' then
            msg = _("Database has been synced to the server!")
          elseif response.body == 'updated' then
            msg = _("Local database has been migrated from the server!")
          else
            msg = _("Sync completed!")
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
          text = _("Search"),
          is_enter_default = false,
          callback = function()
            UIManager:close(dialog)

            self:searchMangas(dialog:getInputText())
          end,
        },
        {
          text = _("Search") .. "*",
          is_enter_default = true,
          callback = function()
            UIManager:close(dialog)

            self:searchMangas(dialog:getInputText(), G_reader_settings:readSetting(
              "exlucde_source_ids_select_search", {}
            ))
          end,
        },
      },
      {
        {
          text = _("Settings"),
          callback = function()
            self:openSettingsSearchDialog()
          end
        },
        {
          text = _("Cancel"),
          id = "close",
          callback = function()
            UIManager:close(dialog)
          end,
        },
      }
    }
  }

  UIManager:show(dialog)
  dialog:onShowKeyboard()
end

--- @private
function LibraryView:openSettingsSearchDialog()
  local response = Backend.listInstalledSources()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local key = "exlucde_source_ids_select_search"
  ---@diagnostic disable-next-line: redundant-parameter
  local dialog = CheckboxDialog:new {
    title = _("Exclude source search for") .. " \"" .. _("Search") .. "*\"",
    current = G_reader_settings:readSetting(key, {}),
    options = response.body,
    update_callback = function(value)
      G_reader_settings:saveSetting(key, value)
    end
  }

  UIManager:show(dialog)
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
              for __,manga in ipairs(self.mangas_raw) do
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
      _("Scaning files..."),
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
        _("Found %d files.") .. "\n\n" ..
        _("Total size %s.") .. "\n\n" ..
        _("RendOnly file .cbz and .epub scan."),
        #filenames,
        total_size
      ),
      ok_text = _("Clean"),
      ok_callback = function()
        local ProgressbarDialog = require("ui/widget/progressbardialog")

        local progressbar_dialog = ProgressbarDialog:new {
          title = _("Deleting..."),
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
          text = string.format(_("Cleaned free %s storage"), total_size)
        })
      end
    }

    UIManager:show(confirm)
  end
  )
end

--- @private
--- @param cancel_id number
--- @param manga Manga
function LibraryView:_refreshManga(cancel_id, manga)
  local response = Backend.refreshChapters(cancel_id, manga.source.id, manga.id)
  return response
end

--- @private
function LibraryView:refreshAllChapters()
  local ProgressbarDialog = require("ui/widget/progressbardialog")

  Trapper:wrap(function()
    local progressbar_dialog = ProgressbarDialog:new {
      title = _("Refresh mangas..."),
      progress_max = #self.mangas_raw
    }
    UIManager:show(progressbar_dialog)
    local errors = {}

    for i, manga in ipairs(self.mangas_raw) do
      local response = self:_refreshManga(Backend.createCancelId(), manga)

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
      local msg = _("Some manga updates fail:") .. "\n\n"
      for __,err in ipairs(errors) do
        msg = msg .. string.format("- [%s] (%s): %s\n", err.source, err.title, err.message)
      end
      ErrorDialog:show(msg)
    else
      UIManager:show(InfoMessage:new {
        text = _("All chapters manga updated!")
      })
    end
  end)
end

--- @private
function LibraryView:openCleanerDialog()
  local dialog

  dialog = ConfirmBox:new {
    text = _("Cleaner") .. "\n\n" ..
        _("Normal") .. ": " .. _("Find and delete invalid files including files from deleted sources") .. "\n\n" ..
        ("Chapter read done: Find and delete chapters that have been read") .. "\n\n" ..
        _("IMPORTANT: Meta files (bookmark, history) not keep!"),
    ok_text = _("Normal"),
    ok_callback = function()
      self:startCleaner(true)
    end,
    other_buttons = { {
      {
        text = _("Chapter read done"),
        callback = function()
          self:startCleaner(false)
        end
      }
    }
    } }

  UIManager:show(dialog)
end

--- @private
function LibraryView:searchMangas(search_text, exclude)
  Trapper:wrap(function()
    local onReturnCallback = function()
      self:fetchAndShow()
    end

    if MangaSearchResults:searchAndShow(search_text, exclude, onReturnCallback) then
      self:onClose()
    end
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
