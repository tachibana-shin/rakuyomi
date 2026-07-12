import { strict as assert } from "node:assert"
import { parseCookieArray, extractUserAgent } from "../src/utils/cookie.ts"
import { parseRegistryMessage } from "../src/utils/registry.ts"

Deno.test("parseCookieArray — parses valid array", () => {
  const json = JSON.stringify([
    { name: "a", value: "1", domain: ".x.com" },
    { name: "b", value: "2", domain: "y.com", path: "/", secure: true },
  ])
  const result = parseCookieArray(json)
  assert.strictEqual(result!.length, 2)
  assert.strictEqual(result![0].domain, ".x.com")
  assert.strictEqual(result![1].domain, "y.com")
  assert.strictEqual(result![1].secure, true)
  assert.strictEqual(result![1].path, "/")
})

Deno.test("parseCookieArray — preserves leading dot in domain", () => {
  const result = parseCookieArray('[{"name":"a","value":"1","domain":".example.com"}]')
  assert.strictEqual(result![0].domain, ".example.com")
})

Deno.test("parseCookieArray — returns null for invalid JSON", () => {
  assert.strictEqual(parseCookieArray("not json"), null)
})

Deno.test("parseCookieArray — returns null for non-array", () => {
  assert.strictEqual(parseCookieArray('{"name":"a"}'), null)
})

Deno.test("parseCookieArray — fills missing fields with defaults", () => {
  const result = parseCookieArray('[{"name":"a","value":"1","domain":"x.com"}]')
  assert.strictEqual(result![0].path, undefined)
  assert.strictEqual(result![0].secure, undefined)
  assert.strictEqual(result![0].httpOnly, undefined)
  assert.strictEqual(result![0].sameSite, undefined)
})

Deno.test("extractUserAgent — extracts Mozilla UA", () => {
  const ua = extractUserAgent("some text\nMozilla/5.0 (Linux; Android 14)\nmore text")
  assert.strictEqual(ua, "Mozilla/5.0 (Linux; Android 14)")
})

Deno.test("extractUserAgent — extracts User-Agent header", () => {
  const ua = extractUserAgent("User-Agent: Mozilla/5.0 Test\ncookie data")
  assert.strictEqual(ua, "Mozilla/5.0 Test")
})

Deno.test("extractUserAgent — returns null if not found", () => {
  assert.strictEqual(extractUserAgent("just plain text"), null)
})

Deno.test("parseRegistryMessage — parses valid registry data", () => {
  const msg = '#REGISTRY_DATA:{"chat_id":1,"device_code":"ABC123","device_name":"kindle"}'
  const result = parseRegistryMessage(msg)
  assert.ok(result !== null)
  assert.strictEqual(result.chat_id, 1)
  assert.strictEqual(result.device_code, "ABC123")
  assert.strictEqual(result.device_name, "kindle")
})

Deno.test("parseRegistryMessage — returns null for non-registry text", () => {
  assert.strictEqual(parseRegistryMessage("random text"), null)
})

Deno.test("parseRegistryMessage — returns null for invalid JSON", () => {
  assert.strictEqual(parseRegistryMessage("#REGISTRY_DATA:not-json"), null)
})
