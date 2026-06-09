local ok, android = pcall(require, "android")

if ok and android then
  return require("platform/android_platform")
end

return require("platform/generic_unix_platform")
