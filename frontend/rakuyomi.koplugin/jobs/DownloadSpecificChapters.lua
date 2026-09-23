local logger = require('logger')

local Backend = require('Backend')
local Job = require('jobs/Job')

--- @class DownloadSpecificChapters: Job
--- @field private source_id string
--- @field private manga_id string
--- @field private chapter_ranges string
--- @field private job_id string
--- @field private langs string[]
local DownloadSpecificChapters = Job:extend()

--- Creates a new `DownloadSpecificChapters` job.
--- @class DownloadSpecificChaptersParams
--- @field source_id string
--- @field manga_id string
--- @field chapter_ranges string Chapter selectors like "1-4, 10, 12"
--- @field langs string[]

--- @param params DownloadSpecificChaptersParams
--- @return self|nil job A new job, or `nil`, if the job could not be created.
function DownloadSpecificChapters:new(params)
  local o = {
    source_id = params.source_id,
    manga_id = params.manga_id,
    chapter_ranges = params.chapter_ranges,
    langs = params.langs,
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
function DownloadSpecificChapters:start()
  local response = Backend.createDownloadSpecificChaptersJob(
    self.source_id,
    self.manga_id,
    self.chapter_ranges,
    self.langs
  )

  if response.type == 'ERROR' then
    logger.warn('could not create download specific chapters job', response.message)

    return false
  end

  self.job_id = response.body

  return true
end

--- @alias PendingState { type: 'INITIALIZING' }|{ type: 'DOWNLOADING', downloaded: number, total: number }

--- @return SuccessfulResponse<nil>|PendingResponse<PendingState>|ErrorResponse
function DownloadSpecificChapters:poll()
  return Job.poll(self)
end

--- @return SuccessfulResponse<nil>|ErrorResponse
function DownloadSpecificChapters:runUntilCompletion()
  return Job.runUntilCompletion(self)
end

return DownloadSpecificChapters