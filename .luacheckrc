allow_defined_top_level_globals = true
stds.lua51 = {
  read_globals = { "self" }
}

ignore = {
  "212/self",
  "__"
}

max_line_length = 300
globals = { "G_defaults", "G_reader_settings", "PublishingStatus", "MangaContentRating", "MangaViewer" }
unused_variable_not_ignore_pattern = "^[^_]" 
read_globals = {
  "describe",
  "it",
  "assert"
}
exclude_files = {
  "frontend/rakuyomi.koplugin/platform/_meta.lua"
}
