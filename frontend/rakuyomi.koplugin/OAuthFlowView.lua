local Blitbuffer = require("ffi/blitbuffer")
local Button = require("ui/widget/button")
local CenterContainer = require("ui/widget/container/centercontainer")
local FocusManager = require("widgets/FocusManagerWithTopZone")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local InfoMessage = require("ui/widget/infomessage")
local OverlapGroup = require("ui/widget/overlapgroup")
local QRWidget = require("ui/widget/qrwidget")
local Screen = require("device").screen
local ScrollableContainer = require("ui/widget/container/scrollablecontainer")
local Size = require("ui/size")
local TextWidget = require("ui/widget/textwidget")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TitleBar = require("ui/widget/titlebar")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local Trapper = require("ui/trapper")
local _ = require("gettext+")

local Backend = require("Backend")

local SERVICE_NAMES = {
  anilist = "AniList",
  myanimelist = "MyAnimeList",
  shikimori = "Shikimori",
  bangumi = "Bangumi",
  mangabaka = "MangaBaka",
}

local OAuthFlowView = FocusManager:extend {
  service = nil,
  session_id = nil,
  bridge_url = nil,
  on_return_callback = nil,
  poll_timer = nil,
}

function OAuthFlowView:init()
  self.dimen = Geom:new {
    x = 0, y = 0,
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

  local service_name = SERVICE_NAMES[self.service] or self.service

  local title_bar = TitleBar:new {
    width = self.dimen.w,
    title = _("Sign in with ") .. service_name,
    left_icon = "chevron.left",
    left_icon_tap_callback = function()
      self:onReturn()
    end,
    close_callback = function()
      self:onClose()
    end,
  }

  local widgets = VerticalGroup:new { align = "left" }

  -- Instruction text
  table.insert(widgets, TextBoxWidget:new {
    text = _("Scan this QR code on your phone to sign in."),
    face = Font:getFace("cfont", 24),
    width = self.dimen.w - 2 * padding,
    alignment = "left",
  })
  table.insert(widgets, VerticalSpan:new { width = Size.padding.large })

  -- QR code — use QRWidget directly (QRMessage is modal and captures all taps)
  if self.bridge_url then
    local frame_padding = Size.padding.large
    local max_qr = Screen:scaleBySize(200)
    local qr_size = math.min(self.dimen.w - 2 * padding - 2 * frame_padding, max_qr)
    table.insert(widgets, CenterContainer:new {
      dimen = Geom:new { w = self.dimen.w - 2 * padding, h = qr_size + 2 * frame_padding },
      FrameContainer:new {
        background = Blitbuffer.COLOR_WHITE,
        padding = frame_padding,
        QRWidget:new {
          text = self.bridge_url,
          width = qr_size,
          height = qr_size,
        },
      },
    })
  else
    table.insert(widgets, TextWidget:new {
      text = _("Error: No bridge URL available."),
      face = Font:getFace("cfont", 20),
      width = self.dimen.w - 2 * padding,
    })
  end

  table.insert(widgets, VerticalSpan:new { width = Size.padding.large })

  -- Status text
  self.status_widget = TextBoxWidget:new {
    text = _("Scan the QR code, then check sign-in status."),
    face = Font:getFace("cfont", 20),
    width = self.dimen.w - 2 * padding,
    alignment = "left",
  }
  table.insert(widgets, self.status_widget)

  table.insert(widgets, VerticalSpan:new { width = Size.span.vertical_small })

  -- Manual check button
  table.insert(widgets, Button:new {
    text = _("Check sign-in status"),
    callback = function()
      self:doPoll()
    end,
    margin = Size.margin.button,
    bordersize = Size.border.button,
    width = self.dimen.w - 2 * padding - Size.border.button,
  })

  local scrollable = ScrollableContainer:new {
    dimen = Geom:new {
      w = self.dimen.w,
      h = self.dimen.h - title_bar.dimen.h,
    },
    ScrollThroughPages = true,
    padding = padding,
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

  -- self:startPoll()
end

function OAuthFlowView:startPoll()
  self:stopPoll()
  self.poll_timer = UIManager:scheduleIn(3, function()
    if not self.session_id then return end
    self:doPoll()
    if self.session_id then
      self:startPoll()
    end
  end)
end

function OAuthFlowView:stopPoll()
  if self.poll_timer then
    UIManager:unschedule(self.poll_timer)
    self.poll_timer = nil
  end
end

function OAuthFlowView:onClose()
  self:stopPoll()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

function OAuthFlowView:onReturn()
  self:onClose()
end

function OAuthFlowView:doPoll()
  if not self.session_id then return end

  self:updateStatus(_("Checking..."))

  Trapper:wrap(function()
    local resp = Backend.pollOAuthStatus(self.session_id)

    if resp.type == "ERROR" then
      self:updateStatus(_("Network error. Please try again."))
      return
    end

    local body = resp.body

    if body.status == "pending" then
      self:updateStatus(_("Waiting for sign-in. Scan the QR code, then tap Check."))
    elseif body.status == "completed" then
      self:stopPoll()
      self:updateStatus(_("Sign-in successful!"))
      self:saveTokens(body)
      UIManager:scheduleIn(1.5, function()
        self:onClose()
      end)
    elseif body.status == "error" then
      self:updateStatus(_("Error: ") .. (body.message or "unknown"))
    end
  end)
end

function OAuthFlowView:updateStatus(text)
  if self.status_widget then
    self.status_widget:setText(text)
    UIManager:setDirty(self, "ui")
  end
end

function OAuthFlowView:saveTokens(body)
  local settings = Backend.getSettings()
  if settings.type == "ERROR" then return end

  local s = settings.body

  if self.service == "anilist" and body.tokens then
    s.anilist = s.anilist or {}
    s.anilist.access_token = body.tokens.access_token
  elseif self.service == "myanimelist" and body.tokens then
    s.myanimelist = s.myanimelist or {}
    s.myanimelist.access_token = body.tokens.access_token
    if body.tokens.refresh_token then
      s.myanimelist.refresh_token = body.tokens.refresh_token
    end
    if body.tokens.client_id then
      s.myanimelist.client_id = body.tokens.client_id
    end
  elseif self.service == "shikimori" and body.tokens then
    s.shikimori = s.shikimori or {}
    s.shikimori.access_token = body.tokens.access_token
    if body.tokens.refresh_token then
      s.shikimori.refresh_token = body.tokens.refresh_token
    end
  elseif self.service == "bangumi" and body.tokens then
    s.bangumi = s.bangumi or {}
    s.bangumi.access_token = body.tokens.access_token
    if body.tokens.refresh_token then
      s.bangumi.refresh_token = body.tokens.refresh_token
    end
  elseif self.service == "mangabaka" and body.tokens then
    s.mangabaka = s.mangabaka or {}
    s.mangabaka.access_token = body.tokens.access_token
    if body.tokens.refresh_token then
      s.mangabaka.refresh_token = body.tokens.refresh_token
    end
  end

  Backend.setSettings(s)
end

--- Start the OAuth flow for a given service.
--- @param service string "anilist" | "myanimelist" | "shikimori" | "bangumi" | "mangabaka"
--- @param on_return_callback function|nil
function OAuthFlowView:startFlow(service, on_return_callback)
  local loading = InfoMessage:new {
    text = _("Creating sign-in session..."),
    dismissable = false,
  }
  UIManager:show(loading)
  UIManager:forceRePaint()

  local resp = Backend.startOAuthSession(service)

  UIManager:close(loading)

  if resp.type == "ERROR" then
    UIManager:show(InfoMessage:new {
      text = _("Failed to start sign-in: ") .. (resp.message or "unknown error"),
    })
    return
  end

  local ui = OAuthFlowView:new {
    service = service,
    session_id = resp.body.session_id,
    bridge_url = resp.body.bridge_url,
    on_return_callback = on_return_callback,
  }
  UIManager:show(ui)
end

return OAuthFlowView
