local TrackingServices = {}

--- @enum TrackingServices.labels
TrackingServices.labels = {
  anilist = "AniList",
  myanimelist = "MyAnimeList",
  shikimori = "Shikimori",
  bangumi = "Bangumi",
  mangabaka = "MangaBaka",
  kavita = "Kavita",
  komga = "Komga",
  suwayomi = "Suwayomi",
}

function TrackingServices.getLabel(service)
  return TrackingServices.labels[service] or service
end

function TrackingServices.getKeys()
  local keys = {}
  for k in pairs(TrackingServices.labels) do
    table.insert(keys, k)
  end
  table.sort(keys)
  return keys
end

return TrackingServices
