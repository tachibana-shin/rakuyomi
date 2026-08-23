--[[--
PagePrefetcher runs fire-and-forget background tasks in a real OS
subprocess through KOReader's Trapper machinery
(`Trapper:dismissableRunInSubprocess` with an invisible trap widget).

The parent coroutine yields while waiting on the child, so the UI event
loop keeps running: page turns during a prefetch never freeze the reader.
The task closure survives the fork (copy-on-write), so it may capture
anything; only its return values travel back through a pipe, and we
discard them.

Used by the streaming document engine to keep a window of upcoming pages
warm in the backend cache ahead of the reader's position.
]]

local logger = require("logger")
local Trapper = require("ui/trapper")

local PagePrefetcher = {}

--- Runs `task` in a background subprocess. The task runs fully in the child
--- process; its single string return value travels back through a pipe and
--- is handed to `on_done` in the parent. Failures are logged, never thrown.
--- Must be called from outside an existing paint cycle (e.g. from a
--- UIManager-scheduled callback).
--- @param task fun(): string|nil The task to run in the subprocess.
--- @param on_done fun(completed: boolean|nil, result: string|nil)|nil Called in the parent once the subprocess exited. `result` is the task's string return value, when it produced one.
--- @param opts table|nil Options: `{ returns_simple_string = boolean }` (default false: values are buffer-encoded).
function PagePrefetcher.runInBackground(task, on_done, opts)
  local returns_simple_string = not not (opts and opts.returns_simple_string)

  Trapper:wrap(function()
    local ok, err
    local completed, result
    -- NOTE: pcall around a yielding call is fine here: KOReader runs on
    -- LuaJIT, which supports yielding across protected calls (Trapper's own
    -- wrap() relies on yielding across xpcall).
    ok, err = pcall(function()
      completed, result =
        Trapper:dismissableRunInSubprocess(task, nil, returns_simple_string)
    end)

    if not ok then
      logger.warn("PagePrefetcher: background task failed:", err)
      completed, result = nil, nil
    end

    if on_done then
      on_done(completed, result)
    end
  end)
end

return PagePrefetcher
