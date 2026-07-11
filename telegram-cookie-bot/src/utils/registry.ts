const REGISTRY_PREFIX = "#REGISTRY_DATA:"

export interface RegistryEntry {
  chat_id: number
  device_code: string
  device_name: string
}

export function parseRegistryMessage(text: string): RegistryEntry | null {
  if (!text.startsWith(REGISTRY_PREFIX)) return null
  try {
    const jsonStr = text.slice(REGISTRY_PREFIX.length).trim()
    return JSON.parse(jsonStr) as RegistryEntry
  } catch {
    return null
  }
}

export function findRegistryEntries(texts: string[]): RegistryEntry[] {
  return texts
    .map((t) => parseRegistryMessage(t))
    .filter((e): e is RegistryEntry => e !== null)
}
