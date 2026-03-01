local logger = require('logger')
local Backend = require('Backend')
local Job = require('jobs/Job')

--- @class RefreshLibraryChapters: Job
local RefreshLibraryChapters = Job:extend()

--- Creates a new `RefreshLibraryChapters` job.
--- @return self|nil job
function RefreshLibraryChapters:new()
  local o = {}
  setmetatable(o, self)
  self.__index = self

  if not o:start() then
    return nil
  end

  return o
end

--- Starts the job.
--- @return boolean success
function RefreshLibraryChapters:start()
  local response = Backend.refreshLibraryChaptersJob()
  if response.type == 'ERROR' then
    logger.error('could not create refresh library chapters job', response.message)
    return false
  else
    self.job_id = response.body
  end

  return true
end

return RefreshLibraryChapters
