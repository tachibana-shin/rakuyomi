local InfoMessage = require("ui/widget/infomessage")
local ConfirmBox = require("ui/widget/confirmbox")
local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")
local _ = require("gettext+")
local ffi = require("ffi")
local bit = require("bit")

-- register shared memory for tracking progress
pcall(ffi.cdef, [[
  typedef struct {
    int processed;
    int total;
    int updated;
  } ko_progress_shm_t;
  void *mmap(void *addr, size_t length, int prot, int flags, int fd, long offset);
  int munmap(void *addr, size_t length);
]])

-- set consts for mmap
local PROT_READ = 1
local PROT_WRITE = 2
local MAP_SHARED = 1
local MAP_ANONYMOUS = 0x20 -- standard value on ARM/x86 Linux

local LoadingDialog = {}

--- Shows a message in a info dialog, while running the given `runnable` function.
--- Must be called from inside a function wrapped with `Trapper:wrap()`.
---
--- @generic T: any
--- @param message string The message to be shown on the dialog.
--- @param runnable fun(): T The function to be ran while showing the dialog.
--- @param onCancel fun()?: T An optional function to be called if the dialog is dismissed/cancelled.
--- @param onConfirmCancel (fun(any): any) | nil
--- @return T, boolean
function LoadingDialog:showAndRun(message, runnable, onCancel, onConfirmCancel)
  assert(Trapper:isWrapped(), "expected to be called inside a function wrapped with `Trapper:wrap()`")

  local cancelled = false
  local conConfirmCancel = nil
  local message_dialog = onCancel == nil and InfoMessage:new {
    text = message,
    dismissable = false,
  } or ConfirmBox:new {
    text = message,
    icon = "notice-info",
    no_ok_button = true,
    -- dismissable = false,
    cancel_callback = function()
      local cancel = function()
        cancelled = true
        if onCancel ~= nil then
          onCancel()
        end
      end

      if onConfirmCancel ~= nil then
        conConfirmCancel = onConfirmCancel(cancel)
      else
        cancel()
      end
    end
  }

  UIManager:show(message_dialog)
  UIManager:forceRePaint()

  local completed, return_values = Trapper:dismissableRunInSubprocess(runnable, message_dialog)
  if onCancel == nil then
    assert(completed, "Expected runnable to run to completion")
  end

  if conConfirmCancel ~= nil then
    UIManager:close(conConfirmCancel)
  end
  UIManager:close(message_dialog)

  return return_values, cancelled
end

--- Shows a message with progress updates in a dialog, while running the given `runnable` function.
--- Must be called from inside a function wrapped with `Trapper:wrap()`.
---
--- @generic T: any
--- @param message string The message to be shown on the dialog.
--- @param runnable fun(onProgress: fun(progress: { type: string, processed: number?, total: number? })): T The function to be ran while showing the dialog. Receives a callback to report progress.
--- @param onCancel fun()?: T An optional function to be called if the dialog is dismissed/cancelled.
--- @param onConfirmCancel (fun(any): any) | nil
--- @return T, boolean
function LoadingDialog:showAndRunWithProgress(message, runnable, onCancel, onConfirmCancel)
  assert(Trapper:isWrapped(), "expected to be called inside a function wrapped with `Trapper:wrap()`")

  local cancelled = false
  local conConfirmCancel = nil
  local message_dialog

  -- allocate shared memory directly in RAM
  local shm_size = ffi.sizeof("ko_progress_shm_t")
  local raw_shm = ffi.C.mmap(nil, shm_size, bit.bor(PROT_READ, PROT_WRITE), bit.bor(MAP_SHARED, MAP_ANONYMOUS), -1, 0)

  if raw_shm == ffi.cast("void *", -1) then
    error("can't allocate shared memory")
  end

  -- initialize shared memory with default values
  local shm = ffi.cast("ko_progress_shm_t *", raw_shm)
  shm.processed = 0
  shm.total = 0
  shm.updated = 0

  local function handleCancel()
    local cancel = function()
      cancelled = true
      if onCancel ~= nil then
        onCancel()
      end
    end
    if onConfirmCancel ~= nil then
      conConfirmCancel = onConfirmCancel(cancel)
    else
      cancel()
    end
  end

  local function createDialog(text)
    message_dialog = ConfirmBox:new {
      text = text,
      icon = "notice-info",
      no_ok_button = true,
      cancel_callback = function()
        if message_dialog.dismiss_callback then
          message_dialog.dismiss_callback()
        else
          handleCancel()
        end
      end
    }
  end

  createDialog(message)
  UIManager:show(message_dialog)
  UIManager:forceRePaint()

  -- Polling Action: parent process reads directly from `shm` variable in RAM
  local last_text = message
  local poll_action
  poll_action = function()
    -- If child process signals a new update (updated == 1)
    if shm.updated == 1 then
      local processed = shm.processed
      local total = shm.total

      if total > 0 then
        local percentage = math.floor(processed / total * 100)
        local new_text = message .. "\n\n" .. percentage .. "% (" .. processed .. "/" .. total .. ")"

        if new_text ~= last_text then
          last_text = new_text
          local current_dismiss_cb = message_dialog.dismiss_callback

          UIManager:close(message_dialog)
          createDialog(new_text)

          message_dialog.dismiss_callback = current_dismiss_cb
          UIManager:show(message_dialog)
          UIManager:forceRePaint()
        end
      end
      -- reset update flag for next update from child
      shm.updated = 0
    end
    UIManager:scheduleIn(0.1, poll_action)
  end

  UIManager:scheduleIn(0.1, poll_action)

  -- Define task to run in subprocess
  local task = function()
    local child_progress = function(progress)
      if not progress or progress.type ~= 'DOWNLOADING' then return end
      -- child process writes directly to shared RAM
      shm.processed = progress.processed
      shm.total = progress.total
      shm.updated = 1
    end
    return runnable(child_progress)
  end

  -- activate subprocess via Trapper
  local trapper_results = table.pack(Trapper:dismissableRunInSubprocess(task, message_dialog, false))

  -- cleanup
  UIManager:unschedule(poll_action)
  ffi.C.munmap(raw_shm, shm_size)

  local completed = trapper_results[1]
  if not completed then
    handleCancel()
  end

  if onCancel == nil then
    assert(completed, "Expected runnable to run to completion")
  end

  if conConfirmCancel ~= nil then
    UIManager:close(conConfirmCancel)
  end
  UIManager:close(message_dialog)

  if completed then
    return unpack(trapper_results, 2, trapper_results.n), cancelled
  else
    return nil, cancelled
  end
end

--- @param message string The message to be shown on the dialog.
--- @param onCancel fun()?: T An optional function to be called if the dialog is dismissed/cancelled.
--- @param onConfirmCancel (fun(any): any)?
--- @diagnostic disable-next-line: undefined-doc-name
--- @return InfoMessage|ConfirmBox, InfoMessage|ConfirmBox|nil
function LoadingDialog:simple(message, onCancel, onConfirmCancel)
  local conConfirmCancel = nil
  local message_dialog = onCancel == nil and InfoMessage:new {
    text = message,
    dismissable = false,
  } or ConfirmBox:new {
    text = message,
    icon = "notice-info",
    no_ok_button = true,
    dismissable = false,
    cancel_callback = function()
      local cancel = function()
        if onCancel ~= nil then
          onCancel()
        end
      end

      if onConfirmCancel ~= nil then
        conConfirmCancel = onConfirmCancel(cancel)
      else
        cancel()
      end
    end
  }

  UIManager:show(message_dialog)

  return message_dialog, conConfirmCancel
end

return LoadingDialog
