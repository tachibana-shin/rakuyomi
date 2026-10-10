/**
 * Key/value store for ephemeral data: pairing codes and OAuth sessions.
 *
 * The public API mirrors the subset of `Deno.Kv` that this app used on Deno
 * Deploy, so `oauth_kv.ts` and the route handlers are unchanged. Two backends
 * implement it:
 *
 * - a Durable Object (see `src/kv/do.ts`) in production,
 * - an in-memory map for local development and tests, where no Durable Object
 *   binding exists.
 */
import { getEnv, type Env } from "./env.ts"

const PAIRING_TTL = 5 * 60 * 1000

/** Stored payload of a pairing code. */
interface PairingEntry {
  chat_id?: number
  device_name?: string
  api_token?: string
  created: number
}

/** The slice of the Deno KV API used by this app. */
export interface KvStore {
  get<T>(key: unknown[]): Promise<{ value: T | null }>
  set(key: unknown[], value: unknown, opts?: { expireIn?: number }): Promise<void>
  delete(key: unknown[]): Promise<void>
  list<T>(opts: {
    prefix: unknown[]
  }): Promise<Array<{ key: unknown[]; value: T }>>
}

// ---------- in-memory backend (local development and tests) ----------

class MemoryKv implements KvStore {
  private store = new Map<string, { value: unknown; expiresAt: number }>()

  set(
    key: unknown[],
    value: unknown,
    opts?: { expireIn?: number },
  ): Promise<void> {
    const k = JSON.stringify(key)
    this.store.set(k, {
      value,
      expiresAt: opts?.expireIn ? Date.now() + opts.expireIn : Infinity,
    })
    return Promise.resolve()
  }

  get<T>(key: unknown[]): Promise<{ value: T | null }> {
    const k = JSON.stringify(key)
    const entry = this.store.get(k)
    if (!entry) return Promise.resolve({ value: null })
    if (Date.now() >= entry.expiresAt) {
      this.store.delete(k)
      return Promise.resolve({ value: null })
    }
    return Promise.resolve({ value: entry.value as T })
  }

  delete(key: unknown[]): Promise<void> {
    this.store.delete(JSON.stringify(key))
    return Promise.resolve()
  }

  list<T>({ prefix }: { prefix: unknown[] }) {
    const prefixStr = JSON.stringify(prefix).slice(0, -1) + ","
    const entries: Array<{ key: unknown[]; value: T }> = []
    for (const [k, entry] of this.store) {
      if (!k.startsWith(prefixStr)) continue
      if (Date.now() >= entry.expiresAt) {
        this.store.delete(k)
        continue
      }
      entries.push({
        key: JSON.parse(k) as unknown[],
        value: entry.value as T,
      })
    }
    return Promise.resolve(entries)
  }
}

// ---------- Durable Object backend ----------

/** Durable Object namespace from the `KV_DO` binding. */
type KvDurableObjectNamespace = NonNullable<Env["KV_DO"]>

class DurableObjectKv implements KvStore {
  private readonly ns: KvDurableObjectNamespace

  constructor(ns: KvDurableObjectNamespace) {
    this.ns = ns
  }

  // Every key lives behind the same object name, so reads and writes are
  // serialized and always see each other.
  private async post(
    op: string,
    body: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    const id = this.ns.idFromName("global")
    const stub = this.ns.get(id)
    const response = await stub.fetch(`http://kv/${op}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    })
    if (!response.ok) {
      throw new Error(`KV operation ${op} failed: HTTP ${response.status}`)
    }
    return await response.json() as Record<string, unknown>
  }

  async get<T>(key: unknown[]): Promise<{ value: T | null }> {
    const res = await this.post("get", { key })
    return { value: (res.value ?? null) as T | null }
  }

  async set(
    key: unknown[],
    value: unknown,
    opts?: { expireIn?: number },
  ): Promise<void> {
    await this.post("set", { key, value, expireIn: opts?.expireIn })
  }

  async delete(key: unknown[]): Promise<void> {
    await this.post("delete", { key })
  }

  async list<T>(opts: {
    prefix: unknown[]
  }): Promise<Array<{ key: unknown[]; value: T }>> {
    const res = await this.post("list", { prefix: opts.prefix })
    return (res.entries ?? []) as Array<{ key: unknown[]; value: T }>
  }
}

// ---------- store selection ----------

let kvImpl: KvStore | null = null

/**
 * Returns the ephemeral key/value store. Uses the Durable Object binding when
 * available and falls back to memory otherwise, which only happens outside
 * Cloudflare (local development and tests).
 */
export async function getKv(): Promise<KvStore> {
  if (kvImpl) return kvImpl

  const ns = getEnv().KV_DO
  if (ns) {
    kvImpl = new DurableObjectKv(ns)
  } else {
    console.warn("KV_DO binding missing, using in-memory fallback")
    kvImpl = new MemoryKv()
  }
  return kvImpl
}

// ---------- pairing ----------

export async function createPairingCode(code: string) {
  const kv = await getKv()
  await kv.set(["pairing", code], { created: Date.now() } as PairingEntry, {
    expireIn: PAIRING_TTL,
  })
}

export async function resolvePairingCode(
  code: string,
  chat_id: number,
  device_name: string,
): Promise<string | null> {
  const kv = await getKv()
  const res = await kv.get<PairingEntry>(["pairing", code])
  if (!res.value) return null
  if (Date.now() - res.value.created >= PAIRING_TTL) {
    await kv.delete(["pairing", code])
    return null
  }
  const tokenBytes = new Uint8Array(32)
  crypto.getRandomValues(tokenBytes)
  const api_token = Array.from(tokenBytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("")
  await kv.set(["pairing", code], {
    ...res.value,
    chat_id,
    device_name,
    api_token,
  } as PairingEntry, { expireIn: PAIRING_TTL })
  return api_token
}

export async function getPairingStatus(
  code: string,
): Promise<
  {
    paired: boolean
    chat_id?: number
    device_name?: string
    api_token?: string
  }
> {
  const kv = await getKv()
  const res = await kv.get<PairingEntry>(["pairing", code])
  if (!res.value) return { paired: false }
  if (Date.now() - res.value.created >= PAIRING_TTL) {
    await kv.delete(["pairing", code])
    return { paired: false }
  }
  if (res.value.chat_id) {
    return {
      paired: true,
      chat_id: res.value.chat_id,
      device_name: res.value.device_name,
      api_token: res.value.api_token,
    }
  }
  return { paired: false }
}

export async function removePairingByDevice(
  chatId: number,
  deviceName: string,
): Promise<boolean> {
  const kv = await getKv()
  let found = false
  const now = Date.now()
  const pending = await kv.list<PairingEntry>({ prefix: ["pairing"] })
  for (const entry of pending) {
    if (
      entry.value &&
      entry.value.chat_id === chatId &&
      entry.value.device_name === deviceName
    ) {
      await kv.delete([...entry.key])
      if (now - entry.value.created < PAIRING_TTL) found = true
    }
  }
  return found
}

export async function getPairingPendingCount(): Promise<number> {
  const kv = await getKv()
  let count = 0
  const now = Date.now()
  const pending = await kv.list<PairingEntry>({ prefix: ["pairing"] })
  for (const entry of pending) {
    if (
      entry.value && !entry.value.chat_id &&
      now - entry.value.created < PAIRING_TTL
    ) {
      count++
    }
  }
  return count
}
