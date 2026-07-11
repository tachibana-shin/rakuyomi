export interface CookieEntry {
  name: string
  value: string
  domain: string
  path?: string
  secure?: boolean
  httpOnly?: boolean
  sameSite?: string
}

export function parseCookieArray(jsonStr: string): CookieEntry[] | null {
  try {
    const data = JSON.parse(jsonStr)
    if (!Array.isArray(data)) return null
    return data.map((c: Record<string, unknown>) => ({
      name: String(c.name ?? ""),
      value: String(c.value ?? ""),
      domain: String(c.domain ?? "").replace(/^\./, ""),
      path: c.path ? String(c.path) : undefined,
      secure: typeof c.secure === "boolean" ? c.secure : undefined,
      httpOnly: typeof c.httpOnly === "boolean" ? c.httpOnly : undefined,
      sameSite: c.sameSite ? String(c.sameSite) : undefined,
    }))
  } catch {
    return null
  }
}

export function extractUserAgent(text: string): string | null {
  const lines = text.split("\n")
  for (const line of lines) {
    const trimmed = line.trim()
    if (
      trimmed.startsWith("Mozilla/") || trimmed.startsWith("User-Agent:")
    ) {
      return trimmed.replace(/^User-Agent:\s*/i, "").trim()
    }
  }
  return null
}

export function formatCookieMessage(
  domain: string,
  cookies: CookieEntry[],
  userAgent?: string | null,
): string {
  const parts: string[] = []
  parts.push(`#COOKIE: ${domain}`)
  parts.push(JSON.stringify(cookies))
  if (userAgent) {
    parts.push(`#UA: ${userAgent}`)
  }
  return parts.join("\n")
}
