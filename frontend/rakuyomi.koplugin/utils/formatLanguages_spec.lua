---@diagnostic disable: undefined-global, undefined-field
local formatLanguages = require('utils/formatLanguages')

describe('formatLanguages', function()
  it('should return nil for an empty list', function()
    assert.is_nil(formatLanguages(nil))
    assert.is_nil(formatLanguages({}))
  end)

  it('should normalise BCP-47 variants and deduplicate', function()
    --- @type string[]
    local languages = { 'en', 'zh-Hans', 'pt-BR', 'zh-Hant', 'EN' }

    assert.equal('en, pt, zh', formatLanguages(languages))
  end)

  it('should sort the codes alphabetically', function()
    --- @type string[]
    local languages = { 'vi', 'de', 'fr' }

    assert.equal('de, fr, vi', formatLanguages(languages))
  end)

  it('should cap the number of listed codes at the default maximum', function()
    --- @type string[]
    local languages = { 'ar', 'de', 'en', 'fr', 'it', 'ja' }

    assert.equal('ar, de, en, fr +2', formatLanguages(languages))
  end)

  it('should honour an explicit maximum', function()
    --- @type string[]
    local languages = { 'ar', 'de', 'en', 'fr', 'it', 'ja' }

    assert.equal('ar, de +4', formatLanguages(languages, 2))
  end)

  it('should keep the full list when it fits under the maximum', function()
    --- @type string[]
    local languages = { 'en', 'vi' }

    assert.equal('en, vi', formatLanguages(languages, 5))
  end)

  it('should keep very long lists short', function()
    local languages = {}
    for i = 1, 50 do
      languages[i] = string.format('c%02d', i)
    end

    assert.equal('c01, c02, c03, c04 +46', formatLanguages(languages))
  end)
end)
