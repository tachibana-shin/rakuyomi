interface PairingEntry {
  chat_id?: number
  device_name?: string
  api_token?: string
  created: number
}

const PAIRING_TTL = 5 * 60 * 1000

// ---------- in-memory fallback when Deno.openKv is unavailable ----------

class MemoryKv {
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

  get<T>(key: unknown[]): Promise<Deno.KvEntryMaybe<T>> {
    const k = JSON.stringify(key)
    const entry = this.store.get(k)
    if (!entry) return Promise.resolve({ value: null } as Deno.KvEntryMaybe<T>)
    if (Date.now() >= entry.expiresAt) {
      this.store.delete(k)
      return Promise.resolve({ value: null } as Deno.KvEntryMaybe<T>)
    }
    return Promise.resolve({ value: entry.value as T } as Deno.KvEntryMaybe<T>)
  }

  delete(key: unknown[]): Promise<void> {
    this.store.delete(JSON.stringify(key))
    return Promise.resolve()
  }

  list<T>({ prefix }: { prefix: unknown[] }) {
    const prefixStr = JSON.stringify(prefix).slice(0, -1)
    const entries: Deno.KvEntry<T>[] = []
    for (const [k, entry] of this.store) {
      if (!k.startsWith(prefixStr)) continue
      if (Date.now() >= entry.expiresAt) {
        this.store.delete(k)
        continue
      }
      entries.push({
        key: JSON.parse(k),
        value: entry.value as T,
        versionstamp: "0",
      })
    }
    let i = 0
    return {
      [Symbol.asyncIterator]() {
        return {
          next(): Promise<IteratorResult<Deno.KvEntry<T>>> {
            if (i < entries.length) {
              return Promise.resolve({ value: entries[i++], done: false })
            }
            return Promise.resolve({
              value: undefined as unknown as Deno.KvEntry<T>,
              done: true,
            })
          },
        }
      },
    }
  }
}

// ---------- KV singleton ----------

let kvImpl: Deno.Kv | MemoryKv | null = null

async function getKv(): Promise<Deno.Kv | MemoryKv> {
  if (kvImpl) return kvImpl
  try {
    kvImpl = await Deno.openKv()
  } catch {
    console.warn("Deno.openKv() not available, using in-memory fallback")
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
  for await (const entry of kv.list<PairingEntry>({ prefix: ["pairing"] })) {
    if (
      entry.value &&
      entry.value.chat_id === chatId &&
      entry.value.device_name === deviceName
    ) {
      if (now - entry.value.created >= PAIRING_TTL) {
        await kv.delete([...entry.key])
        continue
      }
      await kv.delete([...entry.key])
      found = true
    }
  }
  return found
}

export async function getPairingPendingCount(): Promise<number> {
  const kv = await getKv()
  let count = 0
  const now = Date.now()
  for await (const entry of kv.list<PairingEntry>({ prefix: ["pairing"] })) {
    if (
      entry.value && !entry.value.chat_id &&
      now - entry.value.created < PAIRING_TTL
    ) {
      count++
    }
  }
  return count
}
