local logger = require('logger')

local Backend = require('Backend')
local Job = require('jobs/Job')

--- @class DownloadChapter: Job
--- @field private source_id string
--- @field private manga_id string
--- @field private chapter_id string
--- @field private chapter_num number
--- @field private job_id string
--- @field started boolean
local DownloadChapter = Job:extend()

--- Creates a new `DownloadChapter` job.
---
--- @param source_id string
--- @param manga_id string
--- @param chapter_id string
--- @param chapter_num number
--- @return self job A new `DownloadChapter` job, case failed use :start() `nil`, if the job could not be created.
function DownloadChapter:new(source_id, manga_id, chapter_id, chapter_num)
  local o = {
    source_id = source_id,
    manga_id = manga_id,
    chapter_id = chapter_id,
    chapter_num = chapter_num,
    started = false,
  }
  setmetatable(o, self)
  self.__index = self

  return o
end

--- Starts the job. Should be called automatically when instantiating a job with `new()`.
---
--- @publish
--- @return SuccessfulResponse<string>|ErrorResponse
function DownloadChapter:start()
  if self.started == true then
    return self.start_result
  end

  self.started = true

  local response = Backend.createDownloadChapterJob(self.source_id, self.manga_id, self.chapter_id, self.chapter_num)
  if response.type == 'ERROR' then
    logger.error('could not create download chapter job', response.message)
  else
    self.job_id = response.body
  end

  self.start_result = response

  return response
end

--- @return SuccessfulResponse<[string, DownloadError[]]>|ErrorResponse
function DownloadChapter:runUntilCompletion()
  return Job.runUntilCompletion(self)
end

return DownloadChapter
