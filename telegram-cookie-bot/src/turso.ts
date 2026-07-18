import { createClient } from "@libsql/client"
import { getConfig } from "./config.ts"

let client: Awaited<ReturnType<typeof createClient>> | null = null

export async function getTurso() {
  if (client) return client

  const { TURSO_DB_URL, TURSO_AUTH_TOKEN } = getConfig()
  if (!TURSO_DB_URL || !TURSO_AUTH_TOKEN) return null

  client = createClient({ url: TURSO_DB_URL, authToken: TURSO_AUTH_TOKEN })
  await migrate(client)
  return client
}

async function migrate(db: Awaited<ReturnType<typeof createClient>>) {
  await db.execute(`
    CREATE TABLE IF NOT EXISTS devices (
      chat_id INTEGER NOT NULL,
      device  TEXT    NOT NULL,
      PRIMARY KEY (chat_id, device)
    )
  `)
  await db.execute(`
    CREATE TABLE IF NOT EXISTS cookie_data (
      chat_id INTEGER NOT NULL,
      device  TEXT    NOT NULL,
      domains TEXT    NOT NULL,
      PRIMARY KEY (chat_id, device)
    )
  `)
  await db.execute(`
    CREATE TABLE IF NOT EXISTS chat_tokens (
      chat_id INTEGER PRIMARY KEY,
      token_hash TEXT NOT NULL
    )
  `)
  await db.execute(`
    CREATE TABLE IF NOT EXISTS cookie_hashes (
      chat_id INTEGER NOT NULL,
      device  TEXT    NOT NULL,
      hash    TEXT    NOT NULL,
      PRIMARY KEY (chat_id, device)
    )
  `)
  await db.execute(`
    CREATE TABLE IF NOT EXISTS chat_languages (
      chat_id INTEGER PRIMARY KEY,
      lang    TEXT NOT NULL DEFAULT 'en'
    )
  `)
}

// ── Devices (the authoritative list of linked devices per chat) ──

export async function listDevices(chatId: number): Promise<string[]> {
  const db = await getTurso()
  if (!db) return []
  try {
    const result = await db.execute({
      sql: "SELECT device FROM devices WHERE chat_id = ? ORDER BY device",
      args: [chatId],
    })
    return result.rows.map((r) => r.device as string)
  } catch {
    return []
  }
}

export async function registerDevice(
  chatId: number,
  device: string,
): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: "INSERT OR IGNORE INTO devices (chat_id, device) VALUES (?, ?)",
      args: [chatId, device],
    })
  } catch {
    // ignore
  }
}

export async function removeDevice(
  chatId: number,
  device: string,
): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: "DELETE FROM devices WHERE chat_id = ? AND device = ?",
      args: [chatId, device],
    })
  } catch {
    // ignore
  }
}

// ── Cookie data ──

export async function loadDeviceData(
  chatId: number,
  device: string,
): Promise<string | null> {
  const db = await getTurso()
  if (!db) return null
  try {
    const result = await db.execute({
      sql: "SELECT domains FROM cookie_data WHERE chat_id = ? AND device = ?",
      args: [chatId, device],
    })
    return result.rows.length > 0 ? (result.rows[0].domains as string) : null
  } catch {
    return null
  }
}

export async function saveDeviceData(
  chatId: number,
  device: string,
  domainsJson: string,
): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: `INSERT INTO cookie_data (chat_id, device, domains) VALUES (?, ?, ?)
            ON CONFLICT(chat_id, device) DO UPDATE SET domains = excluded.domains`,
      args: [chatId, device, domainsJson],
    })
  } catch {
    // ignore
  }
}

export async function deleteDeviceData(
  chatId: number,
  device: string,
): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: "DELETE FROM cookie_data WHERE chat_id = ? AND device = ?",
      args: [chatId, device],
    })
  } catch {
    // ignore
  }
}

export async function deleteAllDeviceData(chatId: number): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: "DELETE FROM cookie_data WHERE chat_id = ?",
      args: [chatId],
    })
    await db.execute({
      sql: "DELETE FROM devices WHERE chat_id = ?",
      args: [chatId],
    })
  } catch {
    // ignore
  }
}

// ── Chat tokens ──

export async function storeChatTokenHash(
  chatId: number,
  hash: string,
): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: `INSERT INTO chat_tokens (chat_id, token_hash) VALUES (?, ?)
            ON CONFLICT(chat_id) DO UPDATE SET token_hash = excluded.token_hash`,
      args: [chatId, hash],
    })
  } catch {
    // ignore
  }
}

export async function getChatTokenHash(
  chatId: number,
): Promise<string | null> {
  const db = await getTurso()
  if (!db) return null
  try {
    const result = await db.execute({
      sql: "SELECT token_hash FROM chat_tokens WHERE chat_id = ?",
      args: [chatId],
    })
    return result.rows.length > 0 ? (result.rows[0].token_hash as string) : null
  } catch {
    return null
  }
}

// ── Cookie content hashes ──

export async function storeDeviceHash(
  chatId: number,
  device: string,
  hash: string,
): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: `INSERT INTO cookie_hashes (chat_id, device, hash) VALUES (?, ?, ?)
            ON CONFLICT(chat_id, device) DO UPDATE SET hash = excluded.hash`,
      args: [chatId, device, hash],
    })
  } catch {
    // ignore
  }
}

export async function getDeviceHash(
  chatId: number,
  device: string,
): Promise<string | null> {
  const db = await getTurso()
  if (!db) return null
  try {
    const result = await db.execute({
      sql: "SELECT hash FROM cookie_hashes WHERE chat_id = ? AND device = ?",
      args: [chatId, device],
    })
    return result.rows.length > 0 ? (result.rows[0].hash as string) : null
  } catch {
    return null
  }
}

// ── Chat language preferences ──

export async function setChatLang(chatId: number, lang: string): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: `INSERT INTO chat_languages (chat_id, lang) VALUES (?, ?)
            ON CONFLICT(chat_id) DO UPDATE SET lang = excluded.lang`,
      args: [chatId, lang],
    })
  } catch {
    // ignore
  }
}

export async function getChatLang(chatId: number): Promise<string | null> {
  const db = await getTurso()
  if (!db) return null
  try {
    const result = await db.execute({
      sql: "SELECT lang FROM chat_languages WHERE chat_id = ?",
      args: [chatId],
    })
    return result.rows.length > 0 ? (result.rows[0].lang as string) : null
  } catch {
    return null
  }
}
