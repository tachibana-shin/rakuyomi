import { strict as assert } from "node:assert"
import {
  clearAllCookies,
  clearDeviceCookies,
  clearDeviceDomainCookies,
  getDeviceCookieCount,
  getDeviceCookies,
  getDeviceDomains,
  getDevices,
  getDomainCookieCount,
  ingestCookies,
} from "../src/store.ts"

const CHAT_ID = 12345

Deno.test("ingestCookies — valid JSON stores cookies grouped by domain", async () => {
  const raw = JSON.stringify([
    { name: "session", value: "abc", domain: "example.com" },
    { name: "token", value: "xyz", domain: "example.com" },
    { name: "cf_clearance", value: "clr", domain: ".cf.com" },
  ])
  const domains = await ingestCookies(CHAT_ID, "/all", raw)

  assert.strictEqual(domains.length, 2)
  assert.ok(domains.includes("example.com"))
  assert.ok(domains.includes(".cf.com"))

  const deviceMap = await getDeviceCookies(CHAT_ID, "/all")
  assert.strictEqual(deviceMap.size, 2)
  assert.strictEqual(deviceMap.get("example.com")!.cookies.length, 2)
  assert.strictEqual(deviceMap.get(".cf.com")!.cookies.length, 1)
})

Deno.test("ingestCookies — invalid JSON returns empty array", async () => {
  const result = await ingestCookies(CHAT_ID, "/all", "not json")
  assert.deepStrictEqual(result, [])
})

Deno.test("ingestCookies — non-array JSON returns empty array", async () => {
  const result = await ingestCookies(CHAT_ID, "/all", '{"name":"x"}')
  assert.deepStrictEqual(result, [])
})

Deno.test("ingestCookies — preserves leading dot from domain", async () => {
  const raw = JSON.stringify([
    { name: "s", value: "v", domain: ".sub.example.com" },
  ])
  await ingestCookies(CHAT_ID, "device_a", raw)

  const domains = await getDeviceDomains(CHAT_ID, "device_a")
  assert.deepStrictEqual(domains, [".sub.example.com"])
})

Deno.test("ingestCookies — stores user agent", async () => {
  const raw = JSON.stringify([
    { name: "s", value: "v", domain: "x.com" },
  ])
  await ingestCookies(CHAT_ID, "device_b", raw, "Mozilla/5.0 Test")

  const deviceMap = await getDeviceCookies(CHAT_ID, "device_b")
  assert.strictEqual(deviceMap.get("x.com")!.user_agent, "Mozilla/5.0 Test")
})

Deno.test("getDevices — returns all device names", async () => {
  const devices = await getDevices(CHAT_ID)
  assert.ok(devices.includes("/all"))
  assert.ok(devices.includes("device_a"))
  assert.ok(devices.includes("device_b"))
})

Deno.test("getDevices — unknown chat returns empty array", async () => {
  assert.deepStrictEqual(await getDevices(99999), [])
})

Deno.test("getDeviceDomains — returns domains for device", async () => {
  const domains = await getDeviceDomains(CHAT_ID, "/all")
  assert.deepStrictEqual(domains.sort(), [".cf.com", "example.com"])
})

Deno.test("getDeviceDomains — unknown device returns empty array", async () => {
  assert.deepStrictEqual(await getDeviceDomains(CHAT_ID, "nonexistent"), [])
})

Deno.test("getDeviceCookieCount — counts domains and cookies", async () => {
  const { domains, cookies } = await getDeviceCookieCount(CHAT_ID, "/all")
  assert.strictEqual(domains, 2)
  assert.strictEqual(cookies, 3)
})

Deno.test("getDeviceCookieCount — unknown device returns zeros", async () => {
  const { domains, cookies } = await getDeviceCookieCount(
    CHAT_ID,
    "nonexistent",
  )
  assert.strictEqual(domains, 0)
  assert.strictEqual(cookies, 0)
})

Deno.test("getDomainCookieCount — counts cookies for a domain", async () => {
  const n = await getDomainCookieCount(CHAT_ID, "/all", "example.com")
  assert.strictEqual(n, 2)
})

Deno.test("getDomainCookieCount — unknown domain returns 0", async () => {
  const n = await getDomainCookieCount(CHAT_ID, "/all", "unknown.com")
  assert.strictEqual(n, 0)
})

Deno.test("getDeviceCookies — fallback from unknown device to /all", async () => {
  const raw = JSON.stringify([
    { name: "fallback", value: "ok", domain: "fallback.com" },
  ])
  await ingestCookies(CHAT_ID, "/all", raw)

  const map = await getDeviceCookies(CHAT_ID, "nonexistent_device")
  assert.ok(map.has("fallback.com"))
  assert.strictEqual(map.get("fallback.com")!.cookies[0].name, "fallback")
})

Deno.test("clearDeviceDomainCookies — removes single domain", async () => {
  const ok = await clearDeviceDomainCookies(CHAT_ID, "/all", "fallback.com")
  assert.strictEqual(ok, true)
  assert.strictEqual(
    await getDomainCookieCount(CHAT_ID, "/all", "fallback.com"),
    0,
  )
})

Deno.test("clearDeviceCookies — removes entire device", async () => {
  const ok = await clearDeviceCookies(CHAT_ID, "device_b")
  assert.strictEqual(ok, true)
  assert.strictEqual((await getDevices(CHAT_ID)).includes("device_b"), false)
})

Deno.test("clearAllCookies — removes all devices for chat", async () => {
  await clearAllCookies(CHAT_ID)
  assert.deepStrictEqual(await getDevices(CHAT_ID), [])
})
