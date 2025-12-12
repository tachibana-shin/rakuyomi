local logger = require('logger')

local Backend = require('Backend')
local Job = require('jobs/Job')

--- @class DownloadChapter: Job
--- @field private source_id string
--- @field private manga_id string
--- @field private chapter_id string
--- @field private chapter_num number
--- @field private job_id string
local DownloadChapter = Job:extend()

--- Creates a new `DownloadChapter` job.
---
--- @param source_id string
--- @param manga_id string
--- @param chapter_id string
--- @param chapter_num number
--- @return self|nil job A new `DownloadChapter` job, or `nil`, if the job could not be created.
function DownloadChapter:new(source_id, manga_id, chapter_id, chapter_num)
  local o = {
    source_id = source_id,
    manga_id = manga_id,
    chapter_id = chapter_id,
    chapter_num = chapter_num,
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
function DownloadChapter:start()
  local response = Backend.createDownloadChapterJob(self.source_id, self.manga_id, self.chapter_id, self.chapter_num)
  if response.type == 'ERROR' then
    logger.error('could not create download chapter job', response.message)

    return false
  end

  self.job_id = response.body

  return true
end

--- @return SuccessfulResponse<[string, DownloadError[]]>|ErrorResponse
function DownloadChapter:runUntilCompletion()
  return Job.runUntilCompletion(self)
end

return DownloadChapter
