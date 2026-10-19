---@diagnostic disable: undefined-global
local PageOverlap = require('utils/pageOverlap')

describe('PageOverlap.computePixels', function()
  it('returns no overlap for a zero percent', function()
    assert.are.equal(0, PageOverlap.computePixels(1000, 0))
  end)

  it('computes the overlap from the visible height', function()
    assert.are.equal(100, PageOverlap.computePixels(1000, 10))
    assert.are.equal(250, PageOverlap.computePixels(1000, 25))
    assert.are.equal(500, PageOverlap.computePixels(1000, 50))
  end)

  it('rounds the overlap down to a whole pixel', function()
    assert.are.equal(100, PageOverlap.computePixels(1001, 10))
  end)

  it('clamps the percentage to the supported range', function()
    assert.are.equal(0, PageOverlap.computePixels(1000, -10))
    assert.are.equal(500, PageOverlap.computePixels(1000, 80))
  end)

  it('accepts numeric strings', function()
    assert.are.equal(150, PageOverlap.computePixels(1000, '15'))
  end)

  it('falls back to no overlap for missing or invalid values', function()
    assert.are.equal(0, PageOverlap.computePixels(1000, nil))
    assert.are.equal(0, PageOverlap.computePixels(1000, 'not a number'))
    assert.are.equal(0, PageOverlap.computePixels(nil, 10))
  end)
end)
