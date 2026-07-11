import { createClient } from "@libsql/client"
import { getConfig } from "../config.ts"

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
      token   TEXT,
      PRIMARY KEY (chat_id, device)
    )
  `)
  try {
    await db.execute(`ALTER TABLE devices ADD COLUMN token TEXT`)
  } catch {
    // column already exists
  }
  await db.execute(`
    CREATE TABLE IF NOT EXISTS cookie_data (
      chat_id INTEGER NOT NULL,
      device  TEXT    NOT NULL,
      domains TEXT    NOT NULL,
      PRIMARY KEY (chat_id, device)
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
  token?: string,
): Promise<void> {
  const db = await getTurso()
  if (!db) return
  try {
    await db.execute({
      sql: `INSERT INTO devices (chat_id, device, token) VALUES (?, ?, ?)
            ON CONFLICT(chat_id, device) DO UPDATE
              SET token = COALESCE(excluded.token, devices.token)`,
      args: [chatId, device, token ?? null],
    })
  } catch {
    // ignore
  }
}

export async function verifyDeviceToken(
  chatId: number,
  device: string,
  token: string,
): Promise<boolean> {
  const db = await getTurso()
  if (!db) return false
  try {
    const result = await db.execute({
      sql: "SELECT token FROM devices WHERE chat_id = ? AND device = ?",
      args: [chatId, device],
    })
    if (result.rows.length === 0) return false
    const stored = result.rows[0].token as string | null
    return stored !== null && stored === token
  } catch {
    return false
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
