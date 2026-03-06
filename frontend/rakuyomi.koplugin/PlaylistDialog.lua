local CustomDialog = require("CustomDialog")
local MenuItem = require("MenuItem")
local ButtonWidget = require("ui/widget/button")
local ButtonDialog = require("ui/widget/buttondialog")
local ConfirmBox = require("ui/widget/confirmbox")
local InputDialog = require("ui/widget/inputdialog")
local UnderlineContainer = require("ui/widget/container/underlinecontainer")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local OverlapGroup = require("ui/widget/overlapgroup")
local TextWidget = require("ui/widget/textwidget")
local LeftContainer = require("ui/widget/container/leftcontainer")
local RightContainer = require("ui/widget/container/rightcontainer")
local GestureRange = require("ui/gesturerange")
local Font = require("ui/font")
local Size = require("ui/size")
local Geom = require("ui/geometry")
local Blitbuffer = require("ffi/blitbuffer")
local Device = require("device")
local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")
local _ = require("gettext+")
local Icons = require("Icons")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")

local Screen = Device.screen
--- A MenuItem subclass that dispatches tap/hold to plain callbacks
--- instead of requiring a Menu parent object.
local PlaylistItem = MenuItem:extend {
  on_tap = nil,   -- fun(): nil
  on_hold = nil,  -- fun(): nil
  playlist = nil, -- Playlist
}

local BUTTON_FONT = Font:getFace("smallffont")
function PlaylistItem:init()
  self.ges_events = {
    TapSelect = {
      GestureRange:new {
        ges = "tap",
        range = self.dimen,
      },
    },
    HoldSelect = {
      GestureRange:new {
        ges = "hold",
        range = self.dimen,
      },
    },
    Pan = { -- (for mousewheel scrolling support)
      GestureRange:new {
        ges = "pan",
        range = self.dimen,
      }
    },
    -- Close = self.on_close_ges
  }

  local face = Font:getFace(self.font)
  self.content_width = self.dimen.w - 2 * Size.padding.fullscreen
  local button_w = Screen:scaleBySize(30)
  local button_p = Screen:scaleBySize(4)

  ---@type Playlist
  local playlist = self.playlist
  self._underline_container = UnderlineContainer:new {
    color = self.line_color,
    linesize = self.linesize,
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
        LeftContainer:new {
          dimen = Geom:new { w = self.content_width, h = self.dimen.h },
          TextWidget:new {
            text = playlist.name,
            face = face,
            max_width = self.content_width - button_w - button_p * 2,
          },
        },

        RightContainer:new {
          dimen = Geom:new { w = self.content_width, h = self.dimen.h },
          ButtonWidget:new {
            text = Icons.FA_ELLIPSIS_VERTICAL,
            face = BUTTON_FONT,
            radius = 0,
            bordersize = 0,
            padding = button_p,
            width = button_w,
            callback = function()
              self.on_hold(playlist)
            end,
          },
        }
      }
    },
  }

  self[1] = self._underline_container
end

function PlaylistItem:onTapSelect()
  print("hello item")
  if self.on_tap then self.on_tap() end
  return true
end

function PlaylistItem:onHoldSelect()
  print("hello item")
  if self.on_hold then self.on_hold() end
  return true
end

--- @class PlaylistDialog: CustomDialog
---@diagnostic disable-next-line: redundant-parameter
local PlaylistDialog = CustomDialog:extend {}

--- @private rename dialog
local function openRenameDialog(playlist, on_done)
  local dialog
  dialog = InputDialog:new {
    title = _("Rename Playlist"),
    input = playlist.name,
    buttons = {
      {
        { text = _("Cancel"), id = "close", callback = function() UIManager:close(dialog) end },
        { text = _("Save"), is_enter_default = true, callback = function()
          local name = dialog:getInputText()
          UIManager:close(dialog)
          if not name or name:match("^%s*$") then return end
          local r = Backend.renamePlaylist(playlist.id, name)
          if r.type == 'ERROR' then
            ErrorDialog:show(r.message)
            return
          end
          on_done()
        end
        },
      }
    }
  }
  UIManager:show(dialog)
  dialog:onShowKeyboard()
end

--- @private delete confirm
local function openDeleteConfirm(playlist, on_done)
  UIManager:show(ConfirmBox:new {
    text = _("Delete playlist") .. " \"" .. playlist.name .. "\"?",
    ok_text = _("Delete"),
    ok_callback = function()
      local r = Backend.deletePlaylist(playlist.id)
      if r.type == 'ERROR' then
        ErrorDialog:show(r.message)
        return
      end
      on_done()
    end
  })
end

--- Shows the playlist dialog.
--- @param on_select fun(playlist: Playlist)|nil Optional. If provided, tapping a playlist calls this instead of opening LibraryView.
--- @param on_return_callback fun()|nil
function PlaylistDialog:fetchAndShow(on_select, on_return_callback)
  Trapper:wrap(function()
    local response = Backend.getPlaylists()
    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)
      return
    end
    PlaylistDialog:_buildAndShow(response.body, on_select, on_return_callback)
  end)
end

--- @private
function PlaylistDialog:_buildAndShow(playlists, on_select_override, on_return_callback)
  local current_dialog

  local function refresh()
    if current_dialog then UIManager:close(current_dialog) end
    Trapper:wrap(function()
      local r = Backend.getPlaylists()
      if r.type == 'ERROR' then
        ErrorDialog:show(r.message)
        return
      end
      PlaylistDialog:_buildAndShow(r.body, on_select_override, on_return_callback)
    end)
  end

  local function on_select(playlist)
    if current_dialog then UIManager:close(current_dialog) end
    if on_select_override then
      on_select_override(playlist)
    else
      Trapper:wrap(function()
        local LibraryView = require("LibraryView")
        LibraryView:fetchAndShow(playlist)
      end)
    end
  end

  local function on_context(playlist)
    if current_dialog then UIManager:close(current_dialog) end
    local ctx
    ctx = ButtonDialog:new {
      title = playlist.name,
      buttons = {
        {
          { text = Icons.FA_BOOK .. " " .. _("Open"), callback = function()
            UIManager:close(ctx)
            on_select(playlist)
          end
          },
        },
        {
          { text = Icons.FA_PEN .. " " .. _("Rename"), callback = function()
            UIManager:close(ctx)
            openRenameDialog(playlist, refresh)
          end
          },
          { text = Icons.FA_TRASH .. " " .. _("Delete"), callback = function()
            UIManager:close(ctx)
            openDeleteConfirm(playlist, refresh)
          end
          },
        },
      },
      tap_close_callback = function()
        refresh()
      end
    }
    UIManager:show(ctx)
  end

  local function on_create()
    if current_dialog then UIManager:close(current_dialog) end
    local input
    input = InputDialog:new {
      title = _("New Playlist"),
      input_hint = _("Playlist name"),
      buttons = {
        {
          {
            text = _("Cancel"),
            id = "close",
            callback = function()
              UIManager:close(input)

              refresh()
            end
          },
          {
            text = _("Create"),
            is_enter_default = true,
            callback = function()
              local name = input:getInputText()
              UIManager:close(input)

              if not name or name:match("^%s*$") then return end

              local r = Backend.createPlaylist(name)
              if r.type == 'ERROR' then
                ErrorDialog:show(r.message)
                return
              end
              refresh()
            end
          },
        }
      },
      close_callback = function()
        refresh()
      end,
    }
    UIManager:show(input)
    input:onShowKeyboard()
  end

  -- Build options list
  local options = {}
  table.insert(options, { _type = "new_playlist" })

  if #playlists == 0 then
    table.insert(options, { _type = "empty" })
  else
    for _, p in ipairs(playlists) do
      table.insert(options, { _type = "playlist", playlist = p })
    end
  end

  local item_height = Screen:scaleBySize(50)

  ---@diagnostic disable-next-line: undefined-field
  current_dialog = PlaylistDialog:new {
    title = _("Playlists"),
    options = options,
    generate = function(option, max_width, _index)
      if option._type == "new_playlist" then
        -- "New playlist" create button at the top
        local btn = ButtonWidget:new {
          text = Icons.FA_PLUS .. "  " .. _("New Playlist"),
          face = Font:getFace("smallffont"),
          radius = Size.radius.button,
          bordersize = Size.border.button,
          padding = Size.padding.button,
          width = max_width - Size.padding.button * 2,
          callback = on_create,
        }
        btn.dimen = btn:getSize()
        return btn
      elseif option._type == "empty" then
        local tw = TextWidget:new {
          text = _("No playlists yet."),
          face = Font:getFace("smallffont"),
          fgcolor = Blitbuffer.COLOR_DARK_GRAY,
        }
        tw.dimen = tw:getSize()
        return tw
      else
        -- Proper MenuItem-style row with tap (open) and hold (context menu)
        local p = option.playlist
        local item = PlaylistItem:new {
          playlist = p,
          width = max_width,
          dimen = Geom:new {
            x = 0, y = 0,
            w = max_width,
            h = item_height,
          },
          on_tap = function() on_select(p) end,
          on_hold = function() on_context(p) end,
        }
        return item
      end
    end,
  }

  UIManager:show(current_dialog)
end

return PlaylistDialog
