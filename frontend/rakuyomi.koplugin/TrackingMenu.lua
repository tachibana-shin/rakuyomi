local ButtonDialog = require("ui/widget/buttondialog")
local InputDialog = require("ui/widget/inputdialog")
local InfoMessage = require("ui/widget/infomessage")
local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")
local _ = require("gettext+")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Icons = require("Icons")
local LoadingDialog = require("LoadingDialog")
local TrackingServices = require("TrackingServices")

local TrackingMenu = {}

--- Format a Unix timestamp as DD/MM/YYYY, or nil if nil.
local function formatDate(timestamp)
  if timestamp == nil or timestamp == 0 then return nil end
  return os.date("%d/%m/%Y", timestamp)
end

--- Parse a DD/MM/YYYY string to Unix timestamp, or nil on failure/empty.
local function parseDate(str)
  if str == nil or str == "" then return nil end
  local day, month, year = str:match("^(%d+)/(%d+)/(%d+)$")
  if not day then return nil end
  day, month, year = tonumber(day), tonumber(month), tonumber(year)
  if not day or not month or not year then return nil end
  if year < 1970 or year > 2100 then return nil end
  if month < 1 or month > 12 then return nil end
  if day < 1 or day > 31 then return nil end
  local ts = os.time({ year = year, month = month, day = day, hour = 0, min = 0, sec = 0 })
  return ts
end

local function formatTrackingCandidate(candidate)
  local suffix = {}
  if candidate.total_chapters ~= nil then
    table.insert(suffix, _("Chapters") .. ": " .. candidate.total_chapters)
  end
  if candidate.total_volumes ~= nil then
    table.insert(suffix, _("Volumes") .. ": " .. candidate.total_volumes)
  end

  if #suffix == 0 then
    return candidate.title
  end

  return candidate.title .. " (" .. table.concat(suffix, ", ") .. ")"
end

local function findTrackingBinding(bindings, service)
  for _, binding in ipairs(bindings or {}) do
    if binding.service == service then
      return binding
    end
  end

  return nil
end

local function isTrackingServiceEnabled(settings, service)
  if settings == nil then return false end
  local s = settings[service]
  if s == nil then return false end
  if s.access_token and s.access_token ~= "" then return true end
  if s.api_key and s.api_key ~= "" then return true end
  return false
end

--- Main entry point: shows tracking actions if bound, or service picker if not.
---@param manga Manga
---@param on_pull_completed? fun() called after a successful pull to refresh the chapter list
function TrackingMenu.openTrackingMenu(manga, on_pull_completed)
  Trapper:wrap(function()
    local bindings_response = LoadingDialog:showAndRun(
      _("Loading tracking..."),
      function()
        return Backend.getTrackingBindings(manga.source.id, manga.id)
      end
    )

    if bindings_response.type == "ERROR" then
      ErrorDialog:show(bindings_response.message)
      return
    end

    local bindings = bindings_response.body or {}

    if #bindings > 0 then
      TrackingMenu.openTrackingServiceActions(manga, bindings[1].service, on_pull_completed)
    else
      TrackingMenu.openTrackingServicePicker(manga, on_pull_completed)
    end
  end)
end

function TrackingMenu.openTrackingServicePicker(manga, on_pull_completed)
  local services = TrackingServices.getKeys()
  local buttons = {}
  local dialog

  local settings_response = Backend.getSettings()
  local settings = (settings_response.type == "SUCCESS") and settings_response.body or nil

  for _, s in ipairs(services) do
    table.insert(buttons, {
      {
        text = TrackingServices.getLabel(s),
        enabled = isTrackingServiceEnabled(settings, s),
        callback = function()
          UIManager:close(dialog)
          TrackingMenu.openTrackingServiceActions(manga, s, on_pull_completed)
        end
      }
    })
  end

  dialog = ButtonDialog:new {
    title = _("Select Tracking Service"),
    buttons = buttons,
  }

  UIManager:show(dialog)
end

function TrackingMenu.openTrackingServiceActions(manga, service, on_pull_completed)
  Trapper:wrap(function()
    local response = LoadingDialog:showAndRun(
      _("Loading tracking..."),
      function()
        return Backend.getTrackingBindings(manga.source.id, manga.id)
      end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)
      return
    end

    local bindings = response.body or {}
    local binding = findTrackingBinding(bindings, service)
    local dialog
    local buttons = {}

    -- Link / Relink
    table.insert(buttons, {
      {
        text = Icons.SYNC .. " " .. (binding and _("Relink") or _("Link")),
        callback = function()
          UIManager:close(dialog)
          TrackingMenu.openTrackingSearch(manga, service, on_pull_completed)
        end
      }
    })

    if binding then
      -- Pull & Push
      table.insert(buttons, {
        {
          text = Icons.INFO .. " " .. _("Pull progress"),
          callback = function()
            UIManager:close(dialog)
            TrackingMenu.syncTrackingService(manga, service, "pull", on_pull_completed)
          end
        },
        {
          text = Icons.SYNC .. " " .. _("Push progress"),
          callback = function()
            UIManager:close(dialog)
            TrackingMenu.syncTrackingService(manga, service, "push", on_pull_completed)
          end
        }
      })

      -- Start / End dates
      table.insert(buttons, {
        {
          text = Icons.INFO .. " " .. _("Start") .. ": " .. (formatDate(binding.started_at) or _("Not set")),
          callback = function()
            UIManager:close(dialog)
            TrackingMenu.openTrackingDateInput(manga, service, binding, "started_at")
          end
        },
        {
          text = Icons.INFO .. " " .. _("End") .. ": " .. (formatDate(binding.completed_at) or _("Not set")),
          callback = function()
            UIManager:close(dialog)
            TrackingMenu.openTrackingDateInput(manga, service, binding, "completed_at")
          end
        }
      })

      -- Unlink
      table.insert(buttons, {
        {
          text = Icons.FA_TRASH .. " " .. _("Unlink") .. " (" .. binding.remote_title .. ")",
          callback = function()
            UIManager:close(dialog)
            TrackingMenu.unlinkTrackingService(manga, service)
          end
        }
      })
    end

    -- Other services
    table.insert(buttons, {
      {
        text = Icons.SYNC .. " " .. _("Other services"),
        callback = function()
          UIManager:close(dialog)
          TrackingMenu.openTrackingServicePicker(manga, on_pull_completed)
        end
      }
    })

    dialog = ButtonDialog:new {
      title = TrackingServices.getLabel(service),
      buttons = buttons,
    }

    UIManager:show(dialog)
  end)
end

function TrackingMenu.openTrackingSearch(manga, service, on_pull_completed)
  local input_dialog
  input_dialog = InputDialog:new {
    title = _("Search tracker title"),
    input = manga.title,
    input_hint = TrackingServices.getLabel(service),
    description = _("Search for a matching manga entry to link with this title."),
    buttons = {
      {
        {
          text = _("Cancel"),
          id = "close",
          callback = function()
            UIManager:close(input_dialog)
          end
        },
        {
          text = _("Search"),
          is_enter_default = true,
          callback = function()
            local query = input_dialog:getInputText()
            UIManager:close(input_dialog)

            Trapper:wrap(function()
              local response = LoadingDialog:showAndRun(
                _("Searching tracker..."),
                function()
                  return Backend.searchTrackingCandidates(manga.source.id, manga.id, service, query)
                end
              )

              if response.type == 'ERROR' then
                ErrorDialog:show(response.message)
                return
              end

              if #response.body == 0 then
                UIManager:show(InfoMessage:new { text = _("No tracking results found.") })
                return
              end

              TrackingMenu.showTrackingCandidates(manga, service, response.body, on_pull_completed)
            end)
          end
        }
      }
    }
  }

  UIManager:show(input_dialog)
  input_dialog:onShowKeyboard()
end

function TrackingMenu.showTrackingCandidates(manga, service, candidates, on_pull_completed)
  local dialog
  local buttons = {}

  for __, candidate in ipairs(candidates) do
    table.insert(buttons, {
      {
        text = formatTrackingCandidate(candidate),
        callback = function()
          UIManager:close(dialog)

          Trapper:wrap(function()
            local response = LoadingDialog:showAndRun(
              _("Linking tracker entry..."),
              function()
                return Backend.linkTrackingBinding(manga.source.id, manga.id, candidate)
              end
            )

            if response.type == 'ERROR' then
              ErrorDialog:show(response.message)
              return
            end

            UIManager:show(InfoMessage:new {
              text = _("Linked with") .. " " .. TrackingServices.getLabel(service) .. ": " .. candidate.title,
            })
          end)
        end
      }
    })
  end

  dialog = ButtonDialog:new {
    title = TrackingServices.getLabel(service),
    buttons = buttons,
  }

  UIManager:show(dialog)
end

function TrackingMenu.syncTrackingService(manga, service, direction, on_pull_completed)
  Trapper:wrap(function()
    local response = LoadingDialog:showAndRun(
      direction == "pull" and _("Pulling tracking progress...") or _("Pushing tracking progress..."),
      function()
        return Backend.syncTrackingBindings(manga.source.id, manga.id, service, direction)
      end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)
      return
    end

    if #response.body == 0 then
      UIManager:show(InfoMessage:new { text = _("No tracking binding found for this service.") })
      return
    end

    local messages = {}
    for _, result in ipairs(response.body) do
      table.insert(messages, result.message)
    end

    if direction == "pull" and on_pull_completed then
      on_pull_completed()
    end

    UIManager:show(InfoMessage:new {
      text = table.concat(messages, "\n"),
    })
  end)
end

function TrackingMenu.openTrackingDateInput(manga, service, binding, field)
  local current_value = binding[field]
  local current_str = formatDate(current_value) or ""
  local label = field == "started_at" and _("Start date") or _("End date")

  local input_dialog
  input_dialog = InputDialog:new {
    title = label,
    input = current_str,
    input_hint = "DD/MM/YYYY",
    description = _("Enter date in DD/MM/YYYY format, or leave empty to clear."),
    buttons = {
      {
        {
          text = _("Cancel"),
          id = "close",
          callback = function()
            UIManager:close(input_dialog)
          end
        },
        {
          text = _("Save"),
          is_enter_default = true,
          callback = function()
            local text = input_dialog:getInputText()
            UIManager:close(input_dialog)

            local ts = parseDate(text)
            local other_field = field == "started_at" and "completed_at" or "started_at"
            local other_value = binding[other_field]

            Trapper:wrap(function()
              local response = LoadingDialog:showAndRun(
                _("Saving date..."),
                function()
                  return Backend.setTrackingDates(
                    manga.source.id,
                    manga.id,
                    service,
                    field == "started_at" and ts or other_value,
                    field == "completed_at" and ts or other_value
                  )
                end
              )

              if response.type == "ERROR" then
                ErrorDialog:show(response.message)
                return
              end

              UIManager:show(InfoMessage:new {
                text = ts and (label .. ": " .. formatDate(ts)) or (label .. ": " .. _("cleared")),
              })
            end)
          end
        }
      }
    }
  }

  UIManager:show(input_dialog)
  input_dialog:onShowKeyboard()
end

function TrackingMenu.unlinkTrackingService(manga, service)
  Trapper:wrap(function()
    local response = LoadingDialog:showAndRun(
      _("Removing tracking binding..."),
      function()
        return Backend.unlinkTrackingBinding(manga.source.id, manga.id, service)
      end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)
      return
    end

    UIManager:show(InfoMessage:new {
      text = _("Unlinked") .. " " .. TrackingServices.getLabel(service),
    })
  end)
end

return TrackingMenu
