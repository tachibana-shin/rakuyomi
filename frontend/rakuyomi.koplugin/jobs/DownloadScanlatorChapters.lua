local logger = require('logger')
local Backend = require('Backend')
local Job = require('jobs/Job')

--- @class DownloadScanlatorChapters: Job
--- @field private source_id string
--- @field private manga_id string
--- @field private scanlator string
--- @field private amount number|nil
--- @field private job_id string
--- @field private langs string[]
local DownloadScanlatorChapters = Job:extend()

--- Creates a new `DownloadScanlatorChapters` job.
--- @class DownloadScanlatorChaptersParams
--- @field source_id string
--- @field manga_id string
--- @field scanlator string
--- @field amount number|nil
--- @field langs string[]

--- @param params DownloadScanlatorChaptersParams
--- @return self|nil job A new job, or `nil`, if the job could not be created.
function DownloadScanlatorChapters:new(params)
  local o = {
    source_id = params.source_id,
    manga_id = params.manga_id,
    scanlator = params.scanlator,
    amount = params.amount,
    langs = params.langs,
  }
  setmetatable(o, self)
  self.__index = self

  if not o:start() then
    return nil
  end

  return o
end

--- Starts the job.
--- @private
--- @return boolean success
function DownloadScanlatorChapters:start()
  local response = Backend.createDownloadScanlatorChaptersJob(
    self.source_id,
    self.manga_id,
    self.scanlator,
    self.amount,
    self.langs
  )

  if response.type == 'ERROR' then
    logger.error('could not create download scanlator chapters job', response.message)
    return false
  end

  self.job_id = response.body
  return true
end

return DownloadScanlatorChapters
