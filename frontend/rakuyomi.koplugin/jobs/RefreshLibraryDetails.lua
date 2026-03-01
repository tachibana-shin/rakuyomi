local logger = require('logger')
local Backend = require('Backend')
local Job = require('jobs/Job')

--- @class RefreshLibraryDetails: Job
local RefreshLibraryDetails = Job:extend()

--- Creates a new `RefreshLibraryDetails` job.
--- @return self|nil job
function RefreshLibraryDetails:new()
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
function RefreshLibraryDetails:start()
  local response = Backend.refreshLibraryDetailsJob()
  if response.type == 'ERROR' then
    logger.error('could not create refresh library details job', response.message)
    return false
  else
    self.job_id = response.body
  end

  return true
end

return RefreshLibraryDetails
