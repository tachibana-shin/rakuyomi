local logger = require('logger')

local Backend = require('Backend')
local Job = require('jobs/Job')

--- @class DownloadUnreadChapters: Job
--- @field private source_id string
--- @field private manga_id string
--- @field private amount number|nil
--- @field private scanlator string|nil
--- @field private job_id string
local DownloadUnreadChapters = Job:extend()

--- Creates a new `DownloadUnreadChapters` job.
--- @class DownloadUnreadChaptersParams
--- @field source_id string
--- @field manga_id string
--- @field amount number|nil
--- @field scanlator string|nil NEW: Optional scanlator filt

--- @param params  DownloadUnreadChaptersParams
--- @return self|nil job A new job, or `nil`, if the job could not be created.
function DownloadUnreadChapters:new(params)
  local o = {
    source_id = params.source_id,
    manga_id = params.manga_id,
    amount = params.amount,
    scanlator = params.scanlator,
  }
  setmetatable(o, self)
  self.__index = self
  
  if not o:start() then
    return nil
  end
  
  return o
end

--- Starts the job. Should be called automatically when instantiating a job with `new()`.
---
--- @private
--- @return boolean success Whether the job started successfully.
function DownloadUnreadChapters:start()
  local response
  
  -- Use scanlator-specific endpoint if scanlator is provided
  if self.scanlator then
    response = Backend.createDownloadScanlatorChaptersJob(
      self.source_id, 
      self.manga_id, 
      self.scanlator, 
      self.amount
    )
  else
    response = Backend.createDownloadUnreadChaptersJob(
      self.source_id, 
      self.manga_id, 
      self.amount
    )
  end
  
  if response.type == 'ERROR' then
    logger.error('could not create download job', response.message)

    return false
  end

  self.job_id = response.body

  return true
end

--- @alias PendingState { type: 'INITIALIZING' }|{ type: 'DOWNLOADING', downloaded: number, total: number }

--- @return SuccessfulResponse<nil>|PendingResponse<PendingState>|ErrorResponse
function DownloadUnreadChapters:poll()
  return Job.poll(self)
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function DownloadUnreadChapters:runUntilCompletion()
  return Job.runUntilCompletion(self)
end

return DownloadUnreadChapters
