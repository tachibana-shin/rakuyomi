local ffiutil = require('ffi/util')

local Backend = require('Backend')

local JOB_POLLING_INTERVAL_SECONDS = 1

-- FIXME this is way too similar to the CompletedJob|PendingJob|ErroredJob thingy
-- maybe make them the same..?
--- @class PendingResponse<T>: { type: 'PENDING', body: T }

--- @class Job
--- @field job_id string
--- @field result unknown|nil
local Job = {}

function Job:extend()
  local o = {}
  setmetatable(o, self)
  self.__index = self

  return o
end

--- @return SuccessfulResponse<unknown>|PendingResponse<unknown>|ErrorResponse
function Job:poll()
  if self.result ~= nil then
    return self.result
  end

  if self.job_id == nil then
    return { type = 'ERROR', message = 'job not started' }
  end

  local response = Backend.getJobDetails(self.job_id)
  if response.type == 'ERROR' then
    self.result = response

    return self.result
  end

  local details = response.body

  if details.type == 'PENDING' then
    return {
      type = 'PENDING',
      body = details.data,
    }
  elseif details.type == 'COMPLETED' then
    self.result = {
      type = 'SUCCESS',
      --- @diagnostic disable-next-line: assign-type-mismatch
      body = details.data
    }
  else
    self.result = {
      type = 'ERROR',
      message = details.data.message
    }
  end

  return self.result
end

--- @return boolean ok Whether the request completed successfully.
function Job:requestCancellation()
  local response = Backend.requestJobCancellation(self.job_id)
  if response.type == 'ERROR' then
    return false
  end

  return true
end

--- @return SuccessfulResponse<unknown>|ErrorResponse
function Job:runUntilCompletion()
  while true do
    local result = self:poll()

    if result.type ~= 'PENDING' then
      return result
    end

    ffiutil.sleep(JOB_POLLING_INTERVAL_SECONDS)
  end
end

return Job
