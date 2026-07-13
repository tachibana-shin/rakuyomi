local ReaderUI = require("apps/reader/readerui")
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local ConfirmBox = require("ui/widget/confirmbox")
local Event = require("ui/event")
local Trapper = require("ui/trapper")
local logger = require("logger")
local _ = require("gettext+")
local Backend = require("Backend")
local shallow_clone = require("utils/shallowClone")

local Testing = require('testing')

--- @class MangaReader
--- @field chapter Chapter
--- @field viewer MangaViewer
--- @field state_viewer boolean
--- @field on_rtl_changed fun(viewer: MangaViewer): nil
--- @field on_end_of_book_callback? fun(no_as_read: boolean): nil
--- This is a singleton that contains a simpler interface with ReaderUI.
local MangaReader = {
  on_return_callback = nil,
  on_end_of_book_callback = nil,
  on_beginning_of_book_callback = nil,
  on_close_book_callback = nil,
  is_showing = false,
  is_switching_document = false,
}

--- @class MangaReaderOptions
--- @field path string Path to the file to be displayed.
--- @field on_return_callback fun(): nil Function to be called when the user selects "Go back to Rakuyomi".
--- @field on_end_of_book_callback fun(no_as_read: boolean): nil Function to be called when the user reaches the end of the file.
--- @field on_beginning_of_book_callback? fun(): nil Function to be called when the user navigates before the first page.
--- @field on_rtl_changed fun(viewer: MangaViewer): nil Function to be called when the RTL setting is toggled.
--- @field chapter Chapter The chapter being read.
--- @field viewer MangaViewer The preferred viewer mode from the source ("DefaultViewer", "Rtl", "Ltr", "Vertical", "Scroll").
--- @field state_viewer boolean The viewer set by user?
--- @field on_close_book_callback? fun(Chapter): nil Function to be called when the user closes the manga reader.

--- Displays the file located in `path` in the KOReader's reader.
--- If a file is already being displayed, it will be replaced.
---
--- @param options MangaReaderOptions
function MangaReader:show(options)
  self.on_return_callback = options.on_return_callback
  self.on_end_of_book_callback = options.on_end_of_book_callback
  self.on_beginning_of_book_callback = options.on_beginning_of_book_callback
  self.on_rtl_changed = options.on_rtl_changed
  self.chapter = options.chapter
  -- Global viewer override takes priority over per-manga/source viewer.
  local global_viewer = G_reader_settings:readSetting('rakuyomi_global_viewer')
  if global_viewer ~= nil and MangaViewer[global_viewer] ~= nil then
    self.viewer = MangaViewer[global_viewer]
  else
    self.viewer = MangaViewer[options.viewer] or MangaViewer.DefaultViewer
  end
  self.state_viewer = options.state_viewer
  self.on_close_book_callback = options.on_close_book_callback
  local c_showing = self.is_showing

  -- move set self.is_showing function Rakuyomi:init call initializeFromReaderUI maybe random call sort
  self.is_showing = true
  if c_showing and ReaderUI.instance ~= nil then
    -- if we're showing, just switch the document
    -- Defer to nextTick to avoid calling switchDocument synchronously from
    -- within an event handler chain (e.g. onEndOfBook → onGotoPageRel),
    -- which would set ReaderUI.document to nil while a pending page-turn
    -- event (from the same tap) is still being processed and tries to
    -- access self.ui.document via getChapterProgress. (issue #__)
    self.is_switching_document = true
    UIManager:nextTick(function()
      if ReaderUI.instance == nil then return end
      ReaderUI.instance:switchDocument(options.path, nil, function()
        self.is_switching_document = false
      end)
    end)
  else
    -- took this from opds reader
    UIManager:broadcastEvent(Event:new("SetupShowReader"))

    ReaderUI:showReader(options.path)
  end

  -- re set because hook end book
  self.is_showing = true
  Testing:emitEvent('manga_reader_shown')
end

--- @param ui unknown The `ReaderUI` instance we're being called from.
function MangaReader:initializeFromReaderUI(ui)
  if self.is_showing then
    ui.menu:registerToMainMenu(MangaReader)
    self:overrideBtnFileManager(ui.menu)

    ui:registerPostInitCallback(function()
      self:hookWithPriorityOntoReaderUiEvents(ui)
    end)
  end
end

--- @private
--- @param ui unknown The currently active `ReaderUI` instance.
function MangaReader:hookWithPriorityOntoReaderUiEvents(ui)
  -- We need to reorder the `ReaderUI` children such that we are the first children,
  -- in order to receive events before all other widgets
  assert(ui.name == "ReaderUI", "expected to be inside ReaderUI")

  local eventListener = WidgetContainer:new({})
  eventListener.onEndOfBook = function()
    -- FIXME this makes `self:onEndOfBook()` get called twice if it does not
    -- return true in the first invocation...
    return self:onEndOfBook()
  end
  eventListener.onCloseWidget = function()
    self:onReaderUiCloseWidget()
  end
  eventListener.onSetRakuViewMode = function()
    self.viewer = ui.document.configurable.rakuyomi_view_mode

    self:applyViewMode(ui)
    self.on_rtl_changed(self.viewer)

    Trapper:wrap(function()
      Backend.setViewer(self.chapter.source_id, self.chapter.manga_id, self.viewer)
    end)
  end

  table.insert(ui, 2, eventListener)

  -- GotoViewRel is handled locally by ReaderPaging via key_events/gestures and
  -- never broadcast through the widget tree, so a child event listener cannot
  -- catch it. Monkey-patch ReaderPaging directly instead.
  -- Guard against re-patching on every chapter switch.
  if ui.paging and not ui.paging._rakuyomi_patched then
    local orig_onGotoViewRel = ui.paging.onGotoViewRel
    ui.paging.onGotoViewRel = function(paging_self, diff, ...)
      if diff < 0 then
        local at_beginning = false
        if paging_self.view and paging_self.view.page_scroll then
          -- Scroll mode: only trigger if the first visible page is page 1
          -- and it is scrolled to its very top (visible_area.y == 0)
          local page_states = paging_self.view.page_states
          if page_states and page_states[1] then
            local first = page_states[1]
            at_beginning = (first.page == 1 and first.visible_area and first.visible_area.y == 0)
          end
        else
          -- Page mode: trigger when on page 1
          at_beginning = (paging_self.current_page == 1)
        end
        if at_beginning then
          if self:onBeginningOfBook() then
            return true
          end
        end
      end
      return orig_onGotoViewRel(paging_self, diff, ...)
    end
    ui.paging._rakuyomi_patched = true
  end
  self:addRakuOptionsToReader(ui)
end

--- Used to add the "Go back to Rakuyomi" menu item. Is called from `ReaderUI`, via the
--- `registerToMainMenu` call done in `initializeFromReaderUI`.
--- @private
function MangaReader:addToMainMenu(menu_items)
  menu_items.go_back_to_rakuyomi = {
    text = _("Go back to Rakuyomi..."),
    sorting_hint = "main",
    callback = function()
      self:onReturn()
    end
  }
end

--- @private
function MangaReader:onReturn()
  self:closeReaderUi(function()
    self.on_return_callback()
  end)
end

function MangaReader:closeReaderUi(done_callback)
  -- Let all event handlers run before closing the ReaderUI, because
  -- some stuff might break if we just remove it ASAP
  UIManager:nextTick(function()
    local FileManager = require("apps/filemanager/filemanager")

    -- we **have** to reopen the `FileManager`, because
    -- apparently this is the only way to get out of the `ReaderUI` without shit
    -- completely breaking (koreader really does not like when there's no `ReaderUI`
    -- nor `FileManager`)
    if ReaderUI.instance ~= nil then
      ReaderUI.instance:onClose()
    end
    if FileManager.instance ~= nil then
      FileManager.instance:reinit()
    else
      FileManager:showFiles()
    end

    (done_callback or function() end)()
  end)
end

--- To be called when the last page of the manga is read.
function MangaReader:onEndOfBook()
  if self.is_showing then
    logger.info("Got end of book")

    self.on_end_of_book_callback(false)
    return true
  end
end

--- To be called when the user navigates before the first page of the manga.
function MangaReader:onBeginningOfBook()
  if self.is_showing and self.on_beginning_of_book_callback ~= nil then
    logger.info("Got beginning of book")

    return self.on_beginning_of_book_callback()
  end
end

--- @private
function MangaReader:onReaderUiCloseWidget()
  if self.is_switching_document then
    return
  end

  if self.on_close_book_callback ~= nil then
    self.on_close_book_callback(self.chapter)
  end

  self.is_showing = false
end

--- @private
function MangaReader:overrideBtnFileManager(menu)
  local old_callback = menu.menu_items.filemanager.callback

  if self.is_showing then
    menu.menu_items.filemanager.callback = function()
      local key = "allow_commaneer_filemanager"
      if G_reader_settings:nilOrFalse(key) then
        local confirm_dialog
        confirm_dialog = ConfirmBox:new {
          text = "どーも" .. "\n" .. _("Do you want Rakuyomi to commandeer this button when you open it?") .. "\n\n" .. _("This setting only affects when you open it with Rakuyomi."),
          dismissable = false,
          ok_text = _("Yes"),
          cancel_text = _("No"),
          ok_callback = function()
            UIManager:close(confirm_dialog)

            G_reader_settings:saveSetting(key, true)
            self:onReturn()
          end,
          cancel_callback = function()
            UIManager:close(confirm_dialog)

            old_callback()
          end
        }

        UIManager:show(confirm_dialog)
      else
        self:onReturn()
      end
    end
  end
end

--- Adds a custom RTL toggle option to the KoptOptions config panel for manga reading.
--- The option appears alongside the existing View Mode toggle in the pageview tab.
--- When enabled, it sets zoom_direction to RTL (Right to Left, Top to Bottom)
--- and enables inverse_reading_order for RTL page turning.
--- @private
function MangaReader:addRakuOptionsToReader(ui)
  if ui == nil or ui.config == nil then
    return
  end

  local config = ui.config

  -- Shallow-copy the options table to avoid mutating the shared KoptOptions module.
  local new_options = shallow_clone(config.options)
  new_options.prefix = config.options.prefix

  -- Find the pageview panel (contains page_scroll / View Mode) and insert our option at position 2.
  for __, panel in ipairs(new_options) do
    if panel.icon == "appbar.pageview" then
      -- Check if the option was already added (e.g. on document switch without closing reader).
      local already_added = false
      for _, opt in ipairs(panel.options) do
        if opt.name == "rakuyomi_view_mode" then
          already_added = true
          break
        end
      end

      if not already_added then
        -- Create a copy of the panel's options array to avoid mutating KoptOptions.
        local new_panel_options = shallow_clone(panel.options)
        table.insert(new_panel_options, 2, {
          name = "rakuyomi_view_mode",
          name_text = _("View Mode"),
          toggle = { _("Default"), _("RTL"), _("LTR"), _("Vertical"), _("Scroll") },
          values = { MangaViewer.DefaultViewer, MangaViewer.Rtl, MangaViewer.Ltr, MangaViewer.Vertical, MangaViewer.Scroll },
          default_value = self.viewer,
          event = "SetRakuViewMode",
          help_text = _([[Choose how pages are displayed and navigated.
Options: Default (follow source), RTL (right-to-left for Japanese manga), LTR (left-to-right for Western comics), Vertical (long-strip webtoons), and Scroll (continuous scroll).]]),
          name_text_hold_callback = function()
            local dialog

            local ButtonDialog = require("ui/widget/buttondialog")
            local current = G_reader_settings:readSetting('rakuyomi_global_viewer')

            local buttons = {
              { {
                text = _("Off") .. ((current == nil or current == '') and " *" or ""),
                callback = function()
                  UIManager:close(dialog)
                  G_reader_settings:saveSetting('rakuyomi_global_viewer', '')
                end
              } },
            }
            for __, name in ipairs({ "Default", "Rtl", "Ltr", "Vertical", "Scroll" }) do
              local label = _(name)
              local suffix = current == name and " *" or ""
              table.insert(buttons, { {
                text = label .. suffix,
                callback = function()
                  UIManager:close(dialog)
                  G_reader_settings:saveSetting('rakuyomi_global_viewer', name)
                  self.viewer = MangaViewer[name]
                  self:applyViewMode(ui)
                  self.on_rtl_changed(self.viewer)
                end
              } })
            end
            dialog = ButtonDialog:new {
              title = _("Global Viewer Override"),
              buttons = buttons,
            }
            UIManager:show(dialog)
          end,
        })
        panel.options = new_panel_options
      end
      break
    end
  end

  config.options = new_options

  local global_viewer_val = G_reader_settings:readSetting('rakuyomi_global_viewer')
  local has_global_override = global_viewer_val ~= nil and MangaViewer[global_viewer_val] ~= nil
  if has_global_override or self.state_viewer or G_reader_settings:nilOrTrue('rakuyomi_auto_viewer_mode') then
    self:applyViewMode(ui)
  else
    ui.document.configurable.rakuyomi_view_mode = 0
  end

  self:patchPressAsDefaultAndAddBtnNext(ui)
end

function MangaReader:applyViewMode(ui)
  -- Set default value on the document configurable.
  -- loadDefaults was already called during ReaderConfig:init() before we added
  -- this option, so we must set it manually.
  local doc = ui.document
  doc.configurable.rakuyomi_view_mode = self.viewer

  local kopt_mode = doc.configurable.page_scroll ~= nil

  if self.viewer == MangaViewer.Rtl or self.viewer == MangaViewer.Ltr then
    doc.configurable._modified = true
    if kopt_mode then
      if doc.configurable.page_scroll ~= 0 then
        ui:handleEvent(Event:new("ConfigChange", "page_scroll", 0))
        ui:handleEvent(Event:new("SetScrollMode", false))
      end
    else
      if doc.configurable.view_mode ~= 0 then
        ui:handleEvent(Event:new("ConfigChange", "view_mode", 0))
        ui:handleEvent(Event:new("SetViewMode", "page"))
      end
    end

    -- reset gap
    if doc.configurable._page_gap_height_changed then
      local gap = G_reader_settings:readSetting('kopt_page_gap_height') or 8
      ui:handleEvent(Event:new("ConfigChange", "page_gap_height", gap))
      ui:handleEvent(Event:new("PageGapUpdate", gap))

      doc.configurable._page_gap_height_changed = false
    end

    local rtl = (not G_reader_settings:isTrue('rakuyomi_never_rtl')) and self.viewer == MangaViewer.Rtl
    if ui.view.inverse_reading_order ~= rtl then
      ui.view:onToggleReadingOrder(rtl)
    end
  elseif self.viewer == MangaViewer.Scroll or self.viewer == MangaViewer.Vertical then
    doc.configurable._modified = true
    if kopt_mode then
      if doc.configurable.page_scroll ~= 1 then
        ui:handleEvent(Event:new("ConfigChange", "page_scroll", 1))
        ui:handleEvent(Event:new("SetScrollMode", true))
      end
    else
      if doc.configurable.view_mode ~= 1 then
        ui:handleEvent(Event:new("ConfigChange", "view_mode", 1))
        ui:handleEvent(Event:new("SetViewMode", "scroll"))
      end
    end

    if self.viewer == MangaViewer.Scroll then
      ui:handleEvent(Event:new("ConfigChange", "page_gap_height", 0))
      ui:handleEvent(Event:new("PageGapUpdate", 0))

      doc.configurable._page_gap_height_changed = true
    elseif doc.configurable._page_gap_height_changed then
      local gap = G_reader_settings:readSetting('kopt_page_gap_height') or 8
      ui:handleEvent(Event:new("ConfigChange", "page_gap_height", gap))
      ui:handleEvent(Event:new("PageGapUpdate", gap))
    end

    if ui.view.inverse_reading_order then
      ui.view:onToggleReadingOrder(false)
    end
  elseif self.viewer == MangaViewer.DefaultViewer then
    if doc.configurable._modified then
      doc.configurable:loadDefaults(ui.config.options)
      doc.configurable.rakuyomi_view_mode = 0
    end
  end

  if self.viewer ~= MangaViewer.DefaultViewer then
    if G_reader_settings:nilOrTrue('rakuyomi_page_margin') and doc.configurable.page_margin > 0 then
      -- -- recommend option
      ui:handleEvent(Event:new("ConfigChange", "page_margin", 0))
      ui:handleEvent(Event:new("MarginUpdate", 0))
    end
    if G_reader_settings:nilOrTrue('rakuyomi_trim_page') and doc.configurable.trim_page ~= 1 then
      ui:handleEvent(Event:new("ConfigChange", "trim_page", 1))
      ui:handleEvent(Event:new("PageCrop", "auto"))
    end
    if G_reader_settings:nilOrTrue('rakuyomi_zoom_mode_type') and doc.configurable.zoom_mode_type ~= 2 then
      ui:handleEvent(Event:new("ConfigChange", "zoom_mode_type", 2))
      ui:handleEvent(Event:new("DefineZoom", "full"))
    end
    if G_reader_settings:nilOrTrue('rakuyomi_zoom_mode_genus') and doc.configurable.zoom_mode_genus ~= 3 then
      ui:handleEvent(Event:new("ConfigChange", "zoom_mode_genus", 3))
      ui:handleEvent(Event:new("DefineZoom", "content"))
    end
  end
end

function MangaReader:patchPressAsDefaultAndAddBtnNext(ui)
  local manga_reader = self
  local ConfigDialog = require("ui/widget/configdialog")
  local T = require("ffi/util").template

  function ui.config:onShowConfigMenu() -- luacheck: ignore self
    --- @patch code
    local options = shallow_clone(self.options)
    options.prefix = self.options.prefix

    local show_btn_prev = G_reader_settings:nilOrTrue('rakuyomi_hide_btn_prev')
    local show_btn_next = G_reader_settings:nilOrTrue('rakuyomi_show_btn_next')
    if show_btn_prev then
      table.insert(options, 1, {
        icon = "chevron.first",
        name = "btn_prev",
        options = {},
      })
    end
    if show_btn_next then
      table.insert(options, {
        icon = "chevron.last",
        name = "btn_next",
        options = {},
      })
    end
    if show_btn_prev and self.last_panel_index == 1 then
      self.last_panel_index = 2
    elseif show_btn_next and self.last_panel_index == #options then
      self.last_panel_index = #options - 1
    end
    --- @/patch code

    --- @original
    self.config_dialog = ConfigDialog:new {
      document = self.document,
      ui = self.ui,
      configurable = self.configurable,
      config_options = options, --- @patch
      is_always_active = true,
      covers_footer = true,
      close_callback = function() self:onCloseCallback() end,
    }
    self.ui.keyselection:stopHighlightIndicator(true) -- stop any text selection in progress, if applicable
    self.ui:handleEvent(Event:new("DisableHinting"))
    --- @/original

    --- @patch code
    --- @description override onShowConfigPanel for listen chevron.right
    local onShowConfigPanel = self.config_dialog.onShowConfigPanel
    function self.config_dialog:onShowConfigPanel(index) -- luacheck: ignore self
      local name = self.config_options[index].name
      if name == "btn_next" then
        manga_reader.on_end_of_book_callback(true)
        return
      elseif name == "btn_prev" then
        manga_reader.on_beginning_of_book_callback()
        return
      end

      return onShowConfigPanel(self, index)
    end
    --- @/patch code

    --- @patch code
    --- @description override press to make as default for rakuyomi_view_mode
    local onMakeDefault = self.config_dialog.onMakeDefault
    function self.config_dialog:onMakeDefault(name, name_text, values, labels, position, ...) -- luacheck: ignore self
      if name == "rakuyomi_view_mode" then
        UIManager:show(ConfirmBox:new {
          text = T(
            _("Set default %1 to %2?"),
            (name_text or ""),
            labels[position]
          ),
          ok_text = T(_("Set as default")),
          ok_callback = function()
            G_reader_settings:saveSetting('rakuyomi_global_viewer', Backend.MangaViewerName[values[position]] or '')

            self:update()
            UIManager:setDirty(self, function()
              return "ui", self.dialog_frame.dimen
            end)
          end,
        })
        return true
      end
      return onMakeDefault(self, name, name_text, values, labels, position, ...)
    end
    --- @/patch

    --- @original
    -- show last used panel when opening config dialog
    self.config_dialog:onShowConfigPanel(self.last_panel_index)
    UIManager:show(self.config_dialog)
    self.ui:handleEvent(Event:new("HandledAsSwipe")) -- cancel any pan scroll made
    --- @/original
    return true
  end
end

return MangaReader
