/**
 * Ephemeral key/value storage backed by a single Durable Object.
 *
 * This replaces the `Deno.openKv()` store that was used on Deno Deploy. A
 * Durable Object was chosen over Workers KV because pairing codes and OAuth
 * sessions are written by the Telegram webhook and read back immediately by a
 * KOReader device - potentially from another edge location - which requires
 * the read-after-write consistency KV does not offer.
 *
 * Durable Object storage has no native TTL (unlike KV), so each entry carries
 * its own `expiresAt` timestamp. Expired entries are dropped lazily whenever
 * they are read or listed, and a periodic alarm sweeps whatever was never
 * touched again.
 */
import { DurableObject } from "cloudflare:workers"

/** Stored payload together with its absolute expiry time in epoch millis. */
interface StoredEntry {
  v: unknown
  /** Absolute expiry; `undefined` means the entry never expires. */
  e?: number
}

/** How often the alarm sweeps entries that were never read again. */
const SWEEP_INTERVAL_MS = 60_000

// Keys are arrays in the Deno KV style. They are encoded as JSON so that
// prefix scans work with Durable Object storage, which only supports string
// keys.
function encodeKey(key: unknown[]): string {
  return JSON.stringify(key)
}

function decodeKey(key: string): unknown[] {
  return JSON.parse(key) as unknown[]
}

// A prefix has to stop on an array element boundary, otherwise `["pairing"]`
// would also match `["pairings", ...]`.
function encodePrefix(prefix: unknown[]): string {
  return encodeKey(prefix).slice(0, -1) + ","
}

export class KvDurableObject extends DurableObject {
  /**
   * Handles one storage operation. The request path is the operation name and
   * the body carries its arguments:
   *
   * - `get`    `{ key }`                   -> `{ value }`
   * - `set`    `{ key, value, expireIn }`  -> `{ ok: true }`
   * - `delete` `{ key }`                   -> `{ ok: true }`
   * - `list`   `{ prefix }`                -> `{ entries }`
   */
  async fetch(request: Request): Promise<Response> {
    if (request.method !== "POST") {
      return new Response("Method Not Allowed", { status: 405 })
    }

    const op = new URL(request.url).pathname.slice(1)
    let body: Record<string, unknown>
    try {
      body = await request.json() as Record<string, unknown>
    } catch {
      return json({ error: "Malformed JSON body" }, 400)
    }

    try {
      switch (op) {
        case "get": {
          const entry = await this.readEntry(
            encodeKey(body.key as unknown[]),
          )
          return json({ value: entry ? entry.v : null })
        }
        case "set": {
          await this.writeEntry(
            encodeKey(body.key as unknown[]),
            body.value,
            body.expireIn as number | undefined,
          )
          return json({ ok: true })
        }
        case "delete": {
          await this.ctx.storage.delete(encodeKey(body.key as unknown[]))
          return json({ ok: true })
        }
        case "list": {
          const prefix = encodePrefix(body.prefix as unknown[])
          const entries: Array<{ key: unknown[]; value: unknown }> = []
          const expired: string[] = []
          const now = Date.now()

          for (const [storageKey, entry] of await this.ctx.storage.list<
            StoredEntry
          >({ prefix })) {
            if (entry.e !== undefined && entry.e <= now) {
              expired.push(storageKey)
              continue
            }
            entries.push({
              key: decodeKey(storageKey),
              value: entry.v,
            })
          }
          if (expired.length > 0) await this.ctx.storage.delete(expired)
          return json({ entries })
        }
        default:
          return new Response("Unknown operation", { status: 404 })
      }
    } catch (e) {
      console.error(`KV durable object: ${op} failed`, e)
      return json({ error: String(e) }, 500)
    }
  }

  /** Reads an entry, removing it first when it has already expired. */
  private async readEntry(storageKey: string): Promise<StoredEntry | null> {
    const entry = await this.ctx.storage.get<StoredEntry>(storageKey)
    if (!entry) return null
    if (this.isExpired(entry)) {
      await this.ctx.storage.delete(storageKey)
      return null
    }
    return entry
  }

  private async writeEntry(
    storageKey: string,
    value: unknown,
    expireIn: number | undefined,
  ): Promise<void> {
    const entry: StoredEntry = { v: value }
    if (expireIn !== undefined) entry.e = Date.now() + expireIn
    await this.ctx.storage.put(storageKey, entry)
    await this.scheduleSweep()
  }

  private isExpired(entry: StoredEntry): boolean {
    return entry.e !== undefined && entry.e <= Date.now()
  }

  /** Makes sure a sweep runs soon; entries usually expire long before then. */
  private async scheduleSweep(): Promise<void> {
    const scheduled = await this.ctx.storage.getAlarm()
    if (scheduled === null || scheduled > Date.now() + SWEEP_INTERVAL_MS) {
      await this.ctx.storage.setAlarm(Date.now() + SWEEP_INTERVAL_MS)
    }
  }

  /** Deletes every expired entry, then keeps sweeping while data remains. */
  async alarm(): Promise<void> {
    const expired: string[] = []
    for (const [storageKey, entry] of await this.ctx.storage.list<
      StoredEntry
    >()) {
      if (this.isExpired(entry)) expired.push(storageKey)
    }
    if (expired.length > 0) await this.ctx.storage.delete(expired)

    const remaining = await this.ctx.storage.list<StoredEntry>({ limit: 1 })
    if (remaining.size > 0) {
      await this.ctx.storage.setAlarm(Date.now() + SWEEP_INTERVAL_MS)
    }
  }
}

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  })
}
