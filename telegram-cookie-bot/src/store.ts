import { type CookieEntry } from "./utils/cookie.ts"
import {
  listDevices,
  registerDevice,
  loadDeviceData,
  saveDeviceData,
  deleteDeviceData,
  deleteAllDeviceData,
  storeChatTokenHash,
  getChatTokenHash,
  storeDeviceHash,
  getDeviceHash as getDeviceHashDb,
} from "./turso.ts"

interface CookieData {
  domain: string
  cookies: CookieEntry[]
  user_agent?: string
}

type DeviceMap = Map<string, CookieData>
type ChatStore = Map<string, DeviceMap>

const deviceCookies = new Map<number, ChatStore>()
const loading = new Set<number>()

async function ensureChatLoaded(chatId: number): Promise<void> {
  if (deviceCookies.has(chatId)) return
  if (loading.has(chatId)) {
    while (loading.has(chatId)) await new Promise((r) => setTimeout(r, 10))
    return
  }
  loading.add(chatId)
  try {
    const chatMap = new Map<string, DeviceMap>()
    const devices = await listDevices(chatId)
    for (const device of devices) {
      const deviceMap = new Map<string, CookieData>()
      const json = await loadDeviceData(chatId, device)
      if (json) {
        const obj = JSON.parse(json) as Record<string, CookieData>
        for (const [domain, data] of Object.entries(obj)) {
          deviceMap.set(domain, data)
        }
      }
      chatMap.set(device, deviceMap)
    }
    deviceCookies.set(chatId, chatMap)
  } finally {
    loading.delete(chatId)
  }
}

function persistDevice(chatId: number, device: string): void {
  const deviceMap = deviceCookies.get(chatId)?.get(device)
  if (deviceMap) {
    const json = JSON.stringify(Object.fromEntries(deviceMap))
    saveDeviceData(chatId, device, json)
  }
}

export async function getDeviceCookies(
  chatId: number,
  device: string,
): Promise<Map<string, CookieData>> {
  await ensureChatLoaded(chatId)
  return deviceCookies.get(chatId)?.get(device) ??
    deviceCookies.get(chatId)?.get("/all") ??
    new Map()
}

export async function getDevices(chatId: number): Promise<string[]> {
  await ensureChatLoaded(chatId)
  const chatMap = deviceCookies.get(chatId)
  if (!chatMap) return []
  return Array.from(chatMap.keys())
}

export async function getDeviceDomains(
  chatId: number,
  device: string,
): Promise<string[]> {
  await ensureChatLoaded(chatId)
  const deviceMap = deviceCookies.get(chatId)?.get(device)
  if (!deviceMap) return []
  return Array.from(deviceMap.keys())
}

export async function getDeviceCookieCount(
  chatId: number,
  device: string,
): Promise<{ domains: number; cookies: number }> {
  await ensureChatLoaded(chatId)
  const deviceMap = deviceCookies.get(chatId)?.get(device)
  if (!deviceMap) return { domains: 0, cookies: 0 }
  let total = 0
  for (const data of deviceMap.values()) {
    total += data.cookies.length
  }
  return { domains: deviceMap.size, cookies: total }
}

export async function getDomainCookieCount(
  chatId: number,
  device: string,
  domain: string,
): Promise<number> {
  await ensureChatLoaded(chatId)
  return deviceCookies.get(chatId)?.get(device)?.get(domain)?.cookies.length ?? 0
}

export async function ingestCookies(
  chatId: number,
  device: string,
  rawJson: string,
  userAgent?: string,
): Promise<string[]> {
  const cookies = parseCookieArray(rawJson)
  if (!cookies) return []

  if (!deviceCookies.has(chatId)) deviceCookies.set(chatId, new Map())
  const chatMap = deviceCookies.get(chatId)!

  if (!chatMap.has(device)) chatMap.set(device, new Map())
  const deviceMap = chatMap.get(device)!

  const domainSet = new Set(cookies.map((c: CookieEntry) => c.domain))
  const domains = Array.from(domainSet)
  for (const domain of domains) {
    const domainCookies = cookies.filter((c: CookieEntry) =>
      c.domain === domain
    )
    deviceMap.set(domain, {
      domain,
      cookies: domainCookies,
      user_agent: userAgent,
    })
  }

  if (device !== "/all") registerDevice(chatId, device)
  persistDevice(chatId, device)

  const hash = await sha256(rawJson)
  deviceHashes.set(`${chatId}:${device}`, hash)
  await storeDeviceHash(chatId, device, hash)

  return domains
}

export function clearDeviceCookies(
  chatId: number,
  device: string,
): boolean {
  const ok = deviceCookies.get(chatId)?.delete(device) ?? false
  deviceHashes.delete(`${chatId}:${device}`)
  deleteDeviceData(chatId, device)
  return ok
}

export function clearDeviceDomainCookies(
  chatId: number,
  device: string,
  domain: string,
): boolean {
  const deviceMap = deviceCookies.get(chatId)?.get(device)
  if (!deviceMap) return false
  const ok = deviceMap.delete(domain)
  if (ok) persistDevice(chatId, device)
  return ok
}

export async function clearAllCookies(chatId: number): Promise<boolean> {
  const ok = deviceCookies.delete(chatId)
  // Clear all device hashes for this chat
  for (const key of deviceHashes.keys()) {
    if (key.startsWith(`${chatId}:`)) deviceHashes.delete(key)
  }
  await deleteAllDeviceData(chatId)
  return ok
}

// ── Parser ──

function parseCookieArray(jsonStr: string): CookieEntry[] | null {
  try {
    const data = JSON.parse(jsonStr)
    if (!Array.isArray(data)) return null
    return data.map((c: Record<string, unknown>) => ({
      name: String(c.name ?? ""),
      value: String(c.value ?? ""),
      domain: String(c.domain ?? ""),
      path: c.path ? String(c.path) : undefined,
      secure: typeof c.secure === "boolean" ? c.secure : undefined,
      httpOnly: typeof c.httpOnly === "boolean" ? c.httpOnly : undefined,
      sameSite: c.sameSite ? String(c.sameSite) : undefined,
    }))
  } catch {
    return null
  }
}

async function sha256(plain: string): Promise<string> {
  const encoder = new TextEncoder()
  const data = encoder.encode(plain)
  const hash = await crypto.subtle.digest("SHA-256", data)
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("")
}

const chatTokenHashes = new Map<number, string>()

export async function storeChatToken(chatId: number, token: string): Promise<void> {
  const hash = await sha256(token)
  chatTokenHashes.set(chatId, hash)
  await storeChatTokenHash(chatId, hash)
}


export async function verifyChatToken(chatId: number, token: string): Promise<boolean> {
  const hash = await sha256(token)
  let stored = chatTokenHashes.get(chatId)
  if (!stored) {
    const dbHash = await getChatTokenHash(chatId)
    if (dbHash) {
      chatTokenHashes.set(chatId, dbHash)
      stored = dbHash
    }
  }
  return stored === hash
}

const deviceHashes = new Map<string, string>() // key: "chatId:device"

export async function getDeviceHash(
  chatId: number,
  device: string,
): Promise<string | null> {
  const key = `${chatId}:${device}`
  const hash = deviceHashes.get(key)
  if (hash) return hash
  const dbHash = await getDeviceHashDb(chatId, device)
  if (dbHash) deviceHashes.set(key, dbHash)
  return dbHash
}
