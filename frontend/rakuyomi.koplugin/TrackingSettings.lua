local Blitbuffer = require("ffi/blitbuffer")
local Button = require("ui/widget/button")
local FocusManager = require("widgets/FocusManagerWithTopZone")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local OverlapGroup = require("ui/widget/overlapgroup")
local Screen = require("device").screen
local Size = require("ui/size")
local TitleBar = require("ui/widget/titlebar")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local InfoMessage = require("ui/widget/infomessage")
local TrackingServices = require("TrackingServices")
local _ = require("gettext+")
local Font = require("ui/font")
local TextWidget = require("ui/widget/textwidget")
local ScrollableContainer = require("ui/widget/container/scrollablecontainer")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local SettingItem = require("widgets/SettingItem")
local Trapper = require("ui/trapper")
local LoadingDialog = require("LoadingDialog")

local TrackingSettings = FocusManager:extend {
  settings = nil,
  on_return_callback = nil,
  username_widgets = nil,
}

local function get_nested(t, path)
  local parts = {}
  for part in path:gmatch("[^%.]+") do
    table.insert(parts, part)
  end
  local current = t
  for _, part in ipairs(parts) do
    if type(current) ~= "table" then return nil end
    current = current[part]
  end
  return current
end

local function set_nested(t, path, value)
  local parts = {}
  for part in path:gmatch("[^%.]+") do
    table.insert(parts, part)
  end
  local current = t
  for i = 1, #parts - 1 do
    if type(current[parts[i]]) ~= "table" then
      current[parts[i]] = {}
    end
    current = current[parts[i]]
  end
  current[parts[#parts]] = value
end

--- Check if a nested settings path has a non-empty value.
local function has_value(settings, path)
  local v = get_nested(settings, path)
  return v ~= nil and v ~= ""
end

local function oauth_sign_in(self, service)
  return function()
    local OAuthFlowView = require("OAuthFlowView")
    OAuthFlowView:startFlow(service, function()
      UIManager:close(self)
      TrackingSettings:fetchAndShow()
    end)
  end
end

local function oauth_sign_out(self, service)
  return function()
    local settings = self.settings
    local svc = settings[service] or {}
    svc.access_token = nil
    svc.refresh_token = nil
    svc.username = nil
    settings[service] = svc
    Backend.setSettings(settings)
    UIManager:close(self)
    TrackingSettings:fetchAndShow()
  end
end

local service_configs = {
  {
    id = "anilist",
    oauth = true,
    fields = {
      { key = "access_token", placeholder = _("Paste AniList token") },
    },
  },
  {
    id = "myanimelist",
    oauth = true,
    fields = {
      { key = "client_id",     placeholder = _("Paste MAL client ID") },
      { key = "client_secret", placeholder = _("Paste MAL client secret") },
      { key = "refresh_token", placeholder = _("Paste MAL refresh token") },
    },
  },
  {
    id = "shikimori",
    oauth = true,
    fields = {
      { key = "client_id",     placeholder = _("Paste Shikimori client ID") },
      { key = "client_secret", placeholder = _("Paste Shikimori client secret") },
      { key = "refresh_token", placeholder = _("Paste Shikimori refresh token") },
    },
  },
  {
    id = "bangumi",
    oauth = true,
    fields = {
      { key = "refresh_token", placeholder = _("Paste Bangumi refresh token") },
    },
  },
  {
    id = "mangabaka",
    oauth = true,
    fields = {
      { key = "refresh_token", placeholder = _("Paste MangaBaka refresh token") },
      { key = "api_key",       placeholder = _("Paste MangaBaka API Key (mb-...)") },
    },
  },
  {
    id = "kavita",
    fields = {
      { key = "url",     placeholder = 'https://kavita.example.com' },
      { key = "api_key", placeholder = _("Paste Kavita API Key") },
    },
  },
  {
    id = "komga",
    fields = {
      { key = "url",     placeholder = 'http://localhost:25600' },
      { key = "api_key", placeholder = _("Komga API key or username:password") },
    },
  },
  {
    id = "suwayomi",
    fields = {
      { key = "url",     placeholder = 'http://localhost:4567' },
      { key = "api_key", placeholder = _("Suwayomi Basic Auth username:password") },
    },
  },
}

local function build_service_sign_in(self, service_id)
  return {
    title = function(settings)
      local key = service_id .. ".access_token"
      local key2 = service_id .. ".api_key"
      if has_value(settings, key) or has_value(settings, key2) then
        return _("Sign out")
      else
        return _("Sign in")
      end
    end,
    callback = function()
      local key = service_id .. ".access_token"
      local key2 = service_id .. ".api_key"
      if has_value(self.settings, key) or has_value(self.settings, key2) then
        oauth_sign_out(self, service_id)()
      else
        oauth_sign_in(self, service_id)()
      end
    end,
  }
end

local function build_validate_button(self, service_id)
  local sign_in = build_service_sign_in(self, service_id)
  return {
    type = 'button_pair',
    left = {
      title = _("Validate ") .. TrackingServices.getLabel(service_id),
      callback = function()
        Trapper:wrap(function()
          local response = LoadingDialog:showAndRun(
            _("Validating..."),
            function()
              return Backend.validateTrackingSettings(service_id)
            end
          )

          if response.type == 'ERROR' then
            ErrorDialog:show(response.message)
            return
          end

          local username = self:fetchAndShowUsername(service_id)
          if username then
            UIManager:show(InfoMessage:new { text = _("Credentials are valid.") .. " (" .. username .. ")" })
          else
            UIManager:show(InfoMessage:new { text = _("Credentials are valid.") })
          end
        end)
      end
    },
    right = sign_in,
  }
end

local function build_validate_button_plain(service_id)
  return {
    type = 'button',
    title = _("Validate ") .. TrackingServices.getLabel(service_id),
    callback = function()
      Trapper:wrap(function()
        local response = LoadingDialog:showAndRun(
          _("Validating..."),
          function()
            return Backend.validateTrackingSettings(service_id)
          end
        )

        if response.type == 'ERROR' then
          ErrorDialog:show(response.message)
          return
        end

        UIManager:show(InfoMessage:new { text = _("Credentials are valid.") })
      end)
    end
  }
end

TrackingSettings.tracking_value_definitions = {
  {
    'tracking_auto_sync',
    {
      type = 'boolean',
      title = _("Auto sync reading progress"),
      default = true,
    }
  },
  {
    nil,
    { type = 'divider', title = _("OAuth Bridge Server") }
  },
  {
    'oauth_server_url',
    {
      type = 'string',
      title = _("OAuth Bridge Server URL"),
      placeholder = 'https://your-bot.deno.dev',
      default = 'https://rakuyomi.tachibana-shin.deno.net/'
    }
  },
}

for _, svc in ipairs(service_configs) do
  table.insert(TrackingSettings.tracking_value_definitions, {
    nil,
    {
      type = 'divider',
      title = TrackingServices.getLabel(svc.id),
      service = svc.oauth and svc.id or nil,
    }
  })

  for _, field in ipairs(svc.fields) do
    table.insert(TrackingSettings.tracking_value_definitions, {
      svc.id .. '.' .. field.key,
      {
        type = 'string',
        title = TrackingServices.getLabel(svc.id) .. ' ' .. field.key:gsub('_', ' '),
        placeholder = field.placeholder,
      }
    })
  end

  if svc.oauth then
    table.insert(TrackingSettings.tracking_value_definitions, {
      'validate_' .. svc.id,
      build_validate_button(TrackingServices, svc.id),
    })
  else
    table.insert(TrackingSettings.tracking_value_definitions, {
      'validate_' .. svc.id,
      build_validate_button_plain(svc.id),
    })
  end
end

function TrackingSettings:init()
  self.username_widgets = {}

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

  local vertical_group = VerticalGroup:new {
    align = "left",
  }

  for _, tuple in ipairs(TrackingSettings.tracking_value_definitions) do
    local key = tuple[1]
    local definition = tuple[2]
    if definition.type == 'divider' then
      local divider = TextWidget:new {
        text = definition.title,
        face = Font:getFace("cfont"),
        bold = true,
      }
      table.insert(vertical_group, divider)

      if definition.service then
        -- Add "Signed in as ..." username label below the divider
        local username_text = TextWidget:new {
          text = "",
          face = Font:getFace("scfont"),
          fgcolor = Blitbuffer.COLOR_DARK_GRAY,
        }
        table.insert(vertical_group, username_text)
        self.username_widgets[definition.service] = username_text
      end
    elseif definition.type == 'button_pair' then
      local left_btn_title = definition.left.title
      if type(left_btn_title) == "function" then
        left_btn_title = left_btn_title(self.settings)
      end
      local left_btn = Button:new {
        text = left_btn_title,
        face = Font:getFace("cfont"),
        bordersize = 1,
        padding = Size.padding.small,
        callback = function()
          if definition.left.callback then
            definition.left.callback()
          end
        end,
      }

      local right = definition.right
      local right_btn_title = right.title
      if type(right_btn_title) == "function" then
        right_btn_title = right_btn_title(self.settings)
      end
      local right_btn = Button:new {
        text = right_btn_title,
        face = Font:getFace("cfont"),
        bordersize = 1,
        padding = Size.padding.small,
        callback = function()
          if right.callback then
            right.callback()
          end
        end,
      }

      local spacer_w = self.item_width - left_btn:getSize().w - right_btn:getSize().w
      table.insert(vertical_group, HorizontalGroup:new {
        left_btn,
        HorizontalSpan:new { width = spacer_w },
        right_btn,
      })
    else
      local value
      if key then
        value = get_nested(self.settings, key)
      end

      -- Resolve dynamic button title
      local btn_title = definition.title
      if type(btn_title) == "function" then
        btn_title = btn_title(self.settings)
      end

      local value_def = {
        type = definition.type,
        title = btn_title or definition.title,
        placeholder = definition.placeholder,
        default = definition.default,
        callback = definition.callback,
      }

      table.insert(vertical_group, SettingItem:new {
        show_parent = self,
        width = self.item_width,
        label = (definition.type == 'button') and "" or definition.title,
        value_definition = value_def,
        value = value,
        on_value_changed_callback = key and function(new_value)
          return self:updateSetting(key, new_value)
        end,
      })
    end
  end

  self.title_bar = TitleBar:new {
    title = _("Tracking"),
    fullscreen = true,
    width = self.dimen.w,
    with_bottom_line = true,
    bottom_line_color = Blitbuffer.COLOR_DARK_GRAY,
    bottom_line_h_padding = padding,
    left_icon = "chevron.left",
    left_icon_tap_callback = function()
      self:onReturn()
    end,
    close_callback = function()
      self:onClose()
    end,
  }

  local scrollable = ScrollableContainer:new {
    dimen = Geom:new {
      w = self.dimen.w,
      h = self.dimen.h - self.title_bar.dimen.h,
    },
    vertical_group,
  }
  local content = OverlapGroup:new {
    allow_mirroring = false,
    dimen = self.inner_dimen:copy(),
    VerticalGroup:new {
      align = "left",
      self.title_bar,
      HorizontalGroup:new {
        HorizontalSpan:new { width = padding },
        scrollable
      }
    }
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
    content
  }

  scrollable.show_parent = self

  UIManager:setDirty(self, "ui")

  self:fetchAllUsernames()
end

function TrackingSettings:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

function TrackingSettings:onReturn()
  self:onClose()
end

function TrackingSettings:updateSetting(key, value)
  set_nested(self.settings, key, value)
  local response = Backend.setSettings(self.settings)
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)
    return
  end
end

--- Update the username label below a service divider.
--- @param service string
function TrackingSettings:showUsername(service)
  local svc = self.settings[service]
  if not svc then return end
  local username = svc.username
  local label = self.username_widgets[service]
  if label then
    if username and username ~= "" then
      label:setText(_("Signed in as ") .. username)
    else
      label:setText(_("No sign"))
    end
    UIManager:setDirty(self, "ui")
  end
end

--- Fetch username from API, save to settings, and update the label.
--- @param service string
--- @return string|nil username
function TrackingSettings:fetchAndShowUsername(service)
  local response = Backend.getTrackingUser(service)
  if response.type == 'ERROR' then
    return nil
  end

  local username = response.body and response.body.username

  -- Update local settings cache
  local svc = self.settings[service] or {}
  if username then
    svc.username = username
  end
  self.settings[service] = svc

  -- not need update because getTrackingUser is function get data in database
  -- Backend.setSettings(self.settings)
  self:showUsername(service)
  return username
end

--- Show usernames from settings, and fetch from API for services with token but no username.
function TrackingSettings:fetchAllUsernames()
  local services = { "anilist", "myanimelist", "shikimori", "bangumi", "mangabaka" }
  for _, service in ipairs(services) do
    self:showUsername(service)
    local svc = self.settings[service] or {}
    local has_token = svc.access_token ~= nil and svc.access_token ~= ""
    local has_username = svc.username ~= nil and svc.username ~= ""
    if has_token and not has_username then
      self:fetchAndShowUsername(service)
    end
  end
end

function TrackingSettings:fetchAndShow(on_return_callback)
  local response = Backend.getSettings()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)
    return
  end

  local ui = TrackingSettings:new {
    settings = response.body,
    on_return_callback = on_return_callback,
  }
  UIManager:show(ui)
end

return TrackingSettings
