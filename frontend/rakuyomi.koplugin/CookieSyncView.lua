local Blitbuffer = require("ffi/blitbuffer")
local Button = require("ui/widget/button")
local ConfirmBox = require("ui/widget/confirmbox")
local FocusManager = require("ui/widget/focusmanager")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local InfoMessage = require("ui/widget/infomessage")
local InputDialog = require("ui/widget/inputdialog")
local LineWidget = require("ui/widget/linewidget")
local OverlapGroup = require("ui/widget/overlapgroup")
local Screen = require("device").screen
local ScrollableContainer = require("ui/widget/container/scrollablecontainer")
local Size = require("ui/size")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TextWidget = require("ui/widget/textwidget")
local TitleBar = require("ui/widget/titlebar")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local _ = require("gettext+")

local Backend = require("Backend")

local CookieSyncView = FocusManager:extend {
  cookie_sync_status = nil,
  pairing_code = nil,
  server_url = nil,
  on_return_callback = nil,
}

function CookieSyncView:init()
  self.dimen = Geom:new {
    x = 0,
    y = 0,
    w = Screen:getWidth(),
    h = Screen:getHeight(),
  }

  if self.dimen.w == Screen:getWidth() and self.dimen.h == Screen:getHeight() then
    self.covers_fullscreen = true
  end

  local border_size = Size.border.window
  local padding = Size.padding.large

  self.inner_dimen = Geom:new {
    w = self.dimen.w - 2 * border_size,
    h = self.dimen.h - 2 * border_size,
  }

  self.item_width = self.inner_dimen.w - 2 * padding

  local title_bar = TitleBar:new {
    width = self.dimen.w,
    title = _("Cookie Sync"),
    left_icon = "chevron.left",
    left_icon_tap_callback = function()
      self:onReturn()
    end,
    close_callback = function()
      self:onClose()
    end,
  }

  local widgets = VerticalGroup:new {
    align = "left",
  }

  table.insert(widgets, self:buildStatusWidget())

  if self.cookie_sync_status and self.cookie_sync_status.paired then
    table.insert(widgets, self:buildPairedActions())
  else
    table.insert(widgets, self:buildUnpairedActions())
  end

  local scrollable = ScrollableContainer:new {
    dimen = Geom:new {
      w = self.dimen.w,
      h = self.dimen.h - title_bar.dimen.h,
    },
    ScrollThroughPages = true,
    padding = Size.padding.small,
    widgets,
  }

  local content = OverlapGroup:new {
    allow_mirroring = false,
    dimen = self.inner_dimen:copy(),
    VerticalGroup:new {
      align = "left",
      title_bar,
      HorizontalGroup:new {
        HorizontalSpan:new { width = padding },
        scrollable,
      },
    },
  }

  self[1] = FrameContainer:new {
    show_parent = self,
    width = self.dimen.w,
    height = self.dimen.h,
    padding = 0,
    margin = 0,
    bordersize = border_size,
    focusable = true,
    background = Blitbuffer.COLOR_WHITE,
    content,
  }

  scrollable.show_parent = self

  UIManager:setDirty(self, "ui")
end

function CookieSyncView:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

function CookieSyncView:onReturn()
  self:onClose()
end

function CookieSyncView:getStatus()
  local resp = Backend.getCookieSyncStatus()
  if resp.type == 'ERROR' then
    self.cookie_sync_status = nil
    return
  end
  self.cookie_sync_status = resp.body
end

function CookieSyncView:fetchAndShow()
  UIManager:close(self)

  local resp = Backend.getCookieSyncStatus()
  local status = resp.type ~= 'ERROR' and resp.body or nil

  local ui = CookieSyncView:new {
    cookie_sync_status = status,
    server_url = self.server_url,
    pairing_code = self.pairing_code,
    on_return_callback = self.on_return_callback,
  }
  UIManager:show(ui)
end

function CookieSyncView:buildStatusWidget()
  local items = VerticalGroup:new {}
  table.insert(items, VerticalSpan:new { width = Size.span.horizontal_large })

  local item_width = self.item_width

  local status = self.cookie_sync_status
  if not status then
    table.insert(items, TextWidget:new {
      text = _("Unable to fetch cookie sync status."),
    })
    return items
  end

  local item_face = Font:getFace("cfont", 20)

  if status.paired then
    local function makeRow(label_text, value_text)
      local label = TextWidget:new { text = label_text, face = item_face }
      local value = TextWidget:new { text = value_text, face = item_face }
      local remaining = item_width - label:getWidth() - value:getWidth()
      if remaining > 0 then
        return HorizontalGroup:new { label, HorizontalSpan:new { width = remaining }, value }
      else
        return HorizontalGroup:new { label, HorizontalSpan:new { width = 5 }, value }
      end
    end

    table.insert(items, makeRow(_("Device paired as: "), status.device_name or "?"))
    table.insert(items, VerticalSpan:new { width = Size.span.vertical_small })
    table.insert(items, makeRow(_("Chat ID: "), status.chat_id or "?"))
    table.insert(items, VerticalSpan:new { width = Size.span.vertical_small })
    table.insert(items, makeRow(_("Cached domains: "), tostring(status.cookie_count or 0)))
  else
    table.insert(items, TextBoxWidget:new {
      text = _("Not paired. Use 'Pair Device' to link this device to your Telegram bot."),
      face = item_face,
      width = item_width,
    })
  end

  table.insert(items, VerticalSpan:new { width = Size.span.vertical_large })
  table.insert(items, LineWidget:new {
    dimen = Geom:new {
      w = item_width,
      h = 1,
    },
  })

  return items
end

function CookieSyncView:buildPairedActions()
  local items = VerticalGroup:new {}
  table.insert(items, VerticalSpan:new { width = Size.span.vertical_large })

  local button_width = self.item_width

  table.insert(items, Button:new {
    text = _("Sync Cookies Now"),
    callback = function()
      self:doSync()
    end,
    margin = Size.margin.button,
    bordersize = Size.border.button,
    width = button_width,
  })
  table.insert(items, VerticalSpan:new { width = Size.span.vertical_large })

  table.insert(items, Button:new {
    text = _("View Cached Cookies"),
    callback = function()
      self:showCookies()
    end,
    margin = Size.margin.button,
    bordersize = Size.border.button,
    width = button_width,
  })
  table.insert(items, VerticalSpan:new { width = Size.span.vertical_large })

  table.insert(items, Button:new {
    text = _("Unpair Device"),
    callback = function()
      self:doUnpair()
    end,
    margin = Size.margin.button,
    bordersize = Size.border.button,
    width = button_width,
  })

  return items
end

function CookieSyncView:buildUnpairedActions()
  local items = VerticalGroup:new {}
  table.insert(items, VerticalSpan:new { width = Size.span.vertical_large })

  local button_width = self.item_width

  table.insert(items, Button:new {
    text = _("Pair Device"),
    callback = function()
      self:startPairing()
    end,
    margin = Size.margin.button,
    bordersize = Size.border.button,
    width = button_width,
  })

  return items
end

function CookieSyncView:startPairing()
  local dialog
  dialog = InputDialog:new {
    title = _("Enter Telegram Bot Server URL"),
    description = _("Enter the URL of your Telegram Bot server. "
      .. "You will get a pairing code to send to the bot."),
    input = self.server_url or "https://b8b5-149-88-103-35.ngrok-free.app",
    input_hint = _("https://your-bot.deno.dev"),
    buttons = {
      {
        {
          text = _("Cancel"),
          callback = function()
            UIManager:close(dialog)
          end,
        },
        {
          text = _("Generate Code"),
          is_enter_default = true,
          callback = function()
            local url = dialog:getInputText()
            if url == "" then
              return
            end
            UIManager:close(dialog)
            self:doGenerateCode(url)
          end,
        },
      },
    },
  }
  UIManager:show(dialog)
end

function CookieSyncView:doGenerateCode(server_url)
  self.server_url = server_url

  local loading = InfoMessage:new {
    text = _("Generating pairing code..."),
    dismissable = false,
  }
  UIManager:show(loading)
  UIManager:forceRePaint()

  local resp = Backend.generatePairingCode(server_url)

  UIManager:close(loading)

  if resp.type == 'ERROR' then
    UIManager:show(InfoMessage:new {
      text = _("Failed to generate pairing code: ") .. (resp.message or "unknown error"),
    })
    return
  end

  local pairing_code = resp.body.pairing_code
  self.pairing_code = pairing_code

  local text = _("Pairing Code: ") .. pairing_code .. "\n\n"
      .. _("1. Open Telegram on your Android phone") .. "\n"
      .. _("2. Send this command to the bot:") .. "\n\n  /link " .. pairing_code .. " "
      .. _("DEVICE_NAME") .. "\n\n"
      .. _("(Replace DEVICE_NAME with a name for this device, e.g. kindle_bedroom)") .. "\n\n"
      .. _("3. Come back here and tap 'Check Pairing'")

  UIManager:show(ConfirmBox:new {
    text = text,
    ok_text = _("Check Pairing"),
    ok_callback = function()
      self:pollPairing()
    end,
    cancel_text = _("Close"),
  })
end

function CookieSyncView:pollPairing()
  local loading = InfoMessage:new {
    text = _("Checking pairing status..."),
    dismissable = false,
  }
  UIManager:show(loading)
  UIManager:forceRePaint()

  local resp = Backend.pollPairingStatus(self.server_url, self.pairing_code)

  UIManager:close(loading)

  if resp.type == 'ERROR' then
    UIManager:show(InfoMessage:new {
      text = _("Failed to check pairing: ") .. (resp.message or "unknown error"),
    })
    return
  end

  if resp.body.paired then
    UIManager:show(InfoMessage:new {
      text = _("Device paired successfully as: ") .. (resp.body.device_name or "?"),
    })
    self:fetchAndShow()
  else
    UIManager:show(ConfirmBox:new {
      text = _("Not paired yet. "
        .. "Make sure you sent the /link command to the Telegram bot. "
        .. "Try again?"),
      ok_text = _("Check Again"),
      ok_callback = function()
        self:pollPairing()
      end,
      cancel_text = _("Close"),
    })
  end
end

function CookieSyncView:doSync()
  local loading = InfoMessage:new {
    text = _("Syncing cookies..."),
    dismissable = false,
  }
  UIManager:show(loading)
  UIManager:forceRePaint()

  local resp = Backend.syncCookies()

  UIManager:close(loading)

  if resp.type == 'ERROR' then
    UIManager:show(InfoMessage:new {
      text = _("Failed to sync cookies: ") .. (resp.message or "unknown error"),
    })
    return
  end

  local count = #(resp.body.domains or {})
  UIManager:show(InfoMessage:new {
    text = _("Synced cookies for ") .. count .. _(" domain(s)."),
  })
  self:fetchAndShow()
end

function CookieSyncView:doUnpair()
  UIManager:show(ConfirmBox:new {
    text = _("Unpair this device? This will clear all synced cookies."),
    ok_text = _("Unpair"),
    ok_callback = function()
      local resp = Backend.unpairDevice()
      if resp.type == 'ERROR' then
        UIManager:show(InfoMessage:new {
          text = _("Failed to unpair: ") .. (resp.message or "unknown error"),
        })
        return
      end
      UIManager:show(InfoMessage:new {
        text = _("Device unpaired."),
      })
      self:fetchAndShow()
    end,
    cancel_text = _("Cancel"),
  })
end

function CookieSyncView:showCookies()
  local resp = Backend.listCookies()

  if resp.type == 'ERROR' then
    UIManager:show(InfoMessage:new {
      text = _("Failed to list cookies: ") .. (resp.message or "unknown error"),
    })
    return
  end

  local domains = resp.body.domains or {}
  local lines = {}
  for _, entry in ipairs(domains) do
    local domain = entry[1]
    local info = entry[2]
    table.insert(lines, domain .. " (" .. #info.cookies .. " cookies)")
  end

  local text = #lines > 0 and table.concat(lines, "\n") or _("No cookies cached.")
  UIManager:show(InfoMessage:new {
    text = text,
  })
end

return CookieSyncView
